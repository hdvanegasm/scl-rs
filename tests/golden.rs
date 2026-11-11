//! Golden snapshot of the simulator's observable behaviour for purely sequential protocols.
//!
//! This is Phase 0 of `docs/concurrent-composition-plan.md`: the baseline that proves goal G6
//! ("code that never forks keeps its behaviour") across the later phases. Every scenario runs with
//! fixed seeds and fixed inputs, and its rendering is compared byte-for-byte against
//! `tests/golden/<scenario>.txt`.
//!
//! Each golden file holds, per scenario:
//!
//! - the `metrics()` table;
//! - per party, in `PartyId` order:
//!   - the trace as rendered by `SimulationTrace`'s `Display`;
//!   - the **exact** timestamp of every event, in nanoseconds;
//!   - the folded bandwidth tree (`bandwidth_tree_for` + `write_folded`).
//!
//! Exact timestamps are recorded because the rendered trace and the metrics table round to the
//! millisecond, which would hide sub-millisecond timing changes.
//!
//! The scenarios are the seven simulator examples, copied here because tests cannot import code
//! from examples, plus four timing-sensitive scenarios: a bandwidth-bound transfer, a quorum
//! collection, a straggler delivered after quorum, and timed receives. The copies differ from the
//! examples in only two ways. Every RNG is seeded. And the two example protocols that drew
//! randomness from a fresh OS-seeded RNG inside `run` now draw it from the environment's RNG.
//! Protocol ids, message sizes and message patterns are unchanged.
//!
//! To regenerate after an *intended* change:
//!
//! ```text
//! SCL_UPDATE_GOLDEN=1 cargo test --test golden
//! ```
//!
//! Then review the diff of `tests/golden/` before committing it.

use std::fs::{create_dir_all, read_to_string, write};
use std::path::PathBuf;
use std::time::Duration;
use std::{env::var_os, fmt::Write as _};

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::{
        ec::secp256k1::Secp256k1,
        field::{mersenne61::Mersenne61, secp256k1_scalar::Secp256k1ScalarField},
    },
    net::{
        simulation::{
            channel::{
                Bandwidth, ChannelConfig, ChannelConfigBuilder, Link, NetworkConfig, NetworkType,
                Rtt, SimpleNetworkConfig,
            },
            network::SimNetwork,
        },
        NetworkError,
    },
    prelude::*,
    protocol::{
        passive_shamir::{
            double_rand_share::PassiveRandDoubleShr,
            mul::PassiveShamirMul,
            rand_share::PassiveRandShr,
            triple::{PassiveTriple, ShamirTriple},
        },
        share::{deal::PassiveDealShr, open::PassiveOpenShr},
    },
    ss::{additive::AdditiveSS, shamir::ShamirSS},
};

// -------------------------------------------------------------------------------------------------
// Harness
// -------------------------------------------------------------------------------------------------

/// Environment variable that switches the harness from comparing to (re)writing golden files.
const UPDATE_VAR: &str = "SCL_UPDATE_GOLDEN";

/// The environment factory every scenario uses: party `p` of scenario `scenario` gets a ChaCha20
/// RNG seeded with `scenario * 1000 + p`, so no two parties or scenarios share a stream.
fn seeded_env(
    scenario: u64,
) -> impl Fn(PartyId, SimNetwork) -> GeneralEnvironment<SimNetwork, ChaCha20Rng> {
    move |party, net| {
        GeneralEnvironment::new(
            net,
            ChaCha20Rng::seed_from_u64(scenario * 1000 + party.as_usize() as u64),
        )
    }
}

/// Renders everything the golden file pins for one simulation outcome.
fn render<O>(outcome: &SimulationOutcome<O>) -> String {
    let mut parties: Vec<PartyId> = outcome.traces.keys().copied().collect();
    parties.sort();

    let mut out = String::new();
    writeln!(out, "## metrics\n{}", outcome.metrics()).unwrap();

    for party in parties {
        let trace = &outcome.traces[&party];
        writeln!(out, "\n## party {}", party.as_usize()).unwrap();

        writeln!(out, "### trace\n{trace}").unwrap();

        writeln!(out, "### exact timestamps (ns)").unwrap();
        for (idx, event) in trace.events().iter().enumerate() {
            writeln!(
                out,
                "{idx:>4} {:<14} {}",
                format!("{:?}", event.event_type()),
                event.timestamp().as_nanos()
            )
            .unwrap();
        }

        writeln!(out, "### folded bandwidth tree").unwrap();
        let mut folded = Vec::new();
        outcome
            .bandwidth_tree_for(party)
            .expect("the party has a trace")
            .write_folded(&mut folded)
            .expect("writing to a Vec cannot fail");
        out.push_str(&String::from_utf8(folded).expect("folded output is UTF-8"));
    }
    out
}

/// Compares `actual` against `tests/golden/<name>.txt`, or writes it there when `UPDATE_VAR` is set.
fn check_golden(name: &str, actual: &str) {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "golden",
        &format!("{name}.txt"),
    ]
    .iter()
    .collect();

    if var_os(UPDATE_VAR).is_some() {
        create_dir_all(path.parent().expect("the path has a parent"))
            .expect("the golden directory must be creatable");
        write(&path, actual).expect("the golden file must be writable");
        return;
    }

    let expected = read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "cannot read golden file {}: {err}. Create it with `{UPDATE_VAR}=1 cargo test --test golden`.",
            path.display()
        )
    });

    if expected != actual {
        let first_diff = expected
            .lines()
            .zip(actual.lines())
            .position(|(e, a)| e != a)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        panic!(
            "golden mismatch for `{name}` at line {}:\n  expected: {:?}\n  actual:   {:?}\n\
             ({} vs {} lines). If the change is intended, regenerate with \
             `{UPDATE_VAR}=1 cargo test --test golden` and review the diff.",
            first_diff + 1,
            expected.lines().nth(first_diff).unwrap_or("<end of file>"),
            actual.lines().nth(first_diff).unwrap_or("<end of file>"),
            expected.lines().count(),
            actual.lines().count(),
        );
    }
}

// -------------------------------------------------------------------------------------------------
// examples/simple_send_recv.rs
// -------------------------------------------------------------------------------------------------

struct SimpleSendRecv;

impl<E: Environment> Protocol<E> for SimpleSendRecv {
    type Output = usize;

    async fn run(self, environment: &mut E) -> Result<usize, Error> {
        let mut packet = Packet::empty();
        packet.write(&environment.network().local_party().as_usize())?;
        let other = environment.network().other()?;
        environment.network_mut().send_to(other, &packet).await?;
        let received_packet = environment.network_mut().recv_from(other).await?;
        environment.network_mut().close().await?;
        let their_id: usize = received_packet.read(0)?;
        Ok(their_id)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SendRecvProtocol")
    }
}

#[test]
fn golden_simple_send_recv() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        vec![PartyId::from(0), PartyId::from(1)],
        |_| SimpleSendRecv,
        seeded_env(1),
        vec![],
    );
    check_golden("simple_send_recv", &render(&outcome));
}

// -------------------------------------------------------------------------------------------------
// examples/send_different_types.rs
// -------------------------------------------------------------------------------------------------

/// Copy of the example's protocol. The example draws its randomness from a fresh OS-seeded RNG
/// inside `run`; here it comes from the environment's seeded RNG instead.
struct SendDifferentTypes;

impl<E: RandEnvironment> Protocol<E> for SendDifferentTypes {
    type Output = (
        Secp256k1ScalarField,
        Secp256k1,
        Vec<AdditiveSS<Secp256k1ScalarField>>,
    );

    async fn run(self, environment: &mut E) -> Result<Self::Output, Error> {
        let mut packet = Packet::empty();
        let rnd_scalar = Secp256k1ScalarField::random(environment.rng_mut());
        let rnd_ec = Secp256k1::gen().scalar_mul(&rnd_scalar);
        let shares = AdditiveSS::shares_from_secret(
            rnd_scalar,
            &[PartyId::from(0), PartyId::from(1)],
            environment.rng_mut(),
        );
        packet.write_labeled(&rnd_scalar)?;
        packet.write_labeled(&rnd_ec)?;
        packet.write_many_labeled(&shares)?;

        let other = environment.network().other()?;
        environment.network_mut().send_to(other, &packet).await?;
        let mut received_packet = environment.network_mut().recv_from(other).await?;
        environment.network_mut().close().await?;

        let mut recv_shares: Vec<AdditiveSS<Secp256k1ScalarField>> = Vec::with_capacity(2);
        for _ in 0..2 {
            recv_shares.push(received_packet.pop()?);
        }
        let ec: Secp256k1 = received_packet.pop()?;
        let scalar: Secp256k1ScalarField = received_packet.pop()?;
        Ok((scalar, ec, recv_shares))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SendRecvProtocol")
    }
}

#[test]
fn golden_send_different_types() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        vec![PartyId::from(0), PartyId::from(1)],
        |_| SendDifferentTypes,
        seeded_env(2),
        vec![],
    );
    check_golden("send_different_types", &render(&outcome));
}

// -------------------------------------------------------------------------------------------------
// examples/additive_shr_secure_sum.rs
// -------------------------------------------------------------------------------------------------

/// Copy of the example's protocol. The example draws its randomness from a fresh OS-seeded RNG
/// inside `run`; here it comes from the environment's seeded RNG instead.
struct DistrAdditiveShr<R> {
    secret: R,
}

impl<E, R> Protocol<E> for DistrAdditiveShr<R>
where
    E: RandEnvironment,
    R: Ring + Sync + Send,
{
    type Output = Vec<AdditiveSS<R>>;

    fn id(&self) -> ProtocolId {
        ProtocolId::from("DistAdditiveShr")
    }

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let n_parties = env.network().party_ids().len();
        let party_ids = env.network().party_ids();
        let shares = AdditiveSS::shares_from_secret(self.secret, &party_ids, env.rng_mut());

        let mut messages = Vec::with_capacity(n_parties);
        for (party_id, share) in env.network().party_ids().iter().zip(shares) {
            let mut packet = Packet::empty();
            packet.write_labeled(&share)?;
            messages.push((*party_id, packet));
        }
        env.network_mut().send_many(&messages).await?;

        let mut shares_others = Vec::with_capacity(n_parties);
        for party_id in env.network().party_ids() {
            let mut share_packet = env.network_mut().recv_from(party_id).await?;
            let share = share_packet.pop()?;
            shares_others.push(share);
        }
        Ok(shares_others)
    }
}

struct ReconstrAdditiveShr<R> {
    share: AdditiveSS<R>,
}

impl<E, R> Protocol<E> for ReconstrAdditiveShr<R>
where
    E: Environment,
    R: Ring + Send + Sync,
{
    type Output = R;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let n_parties = env.network().party_ids().len();
        let mut packet = Packet::empty();
        packet.write_labeled(&self.share)?;
        let messages: Vec<(PartyId, Packet)> = env
            .network()
            .party_ids()
            .into_iter()
            .map(|party| (party, packet.clone()))
            .collect();
        env.network_mut().send_many(&messages).await?;

        let mut shares: Vec<AdditiveSS<R>> = Vec::with_capacity(n_parties);
        for party in env.network().party_ids() {
            let mut share_packet = env.network_mut().recv_from(party).await?;
            let share = share_packet.pop()?;
            shares.push(share);
        }
        Ok(AdditiveSS::secret_from_shares(&shares))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("ReconstrAdditiveShr")
    }
}

struct SecureAddition<R> {
    input: R,
}

impl<E, R> Protocol<E> for SecureAddition<R>
where
    R: Ring + Send + Sync,
    E: RandEnvironment,
{
    type Output = R;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let shares: Vec<AdditiveSS<R>> =
            DistrAdditiveShr { secret: self.input }.execute(env).await?;
        let share_sum = shares
            .into_iter()
            .reduce(|acc, elem| acc + &elem)
            .expect("there is at least one share to sum");
        ReconstrAdditiveShr { share: share_sum }.execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SecSumAdditiveShr")
    }
}

#[test]
fn golden_additive_shr_secure_sum() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        (0..3).map(PartyId::from).collect(),
        |party| SecureAddition {
            input: Secp256k1ScalarField::from(100 + party.as_usize() as u64),
        },
        seeded_env(3),
        vec![],
    );
    check_golden("additive_shr_secure_sum", &render(&outcome));
}

// -------------------------------------------------------------------------------------------------
// examples/bandwidth_flamegraph.rs
// -------------------------------------------------------------------------------------------------

type ShamirM61 = ShamirSS<1, Mersenne61>;

/// Every party deals its input to everyone, at full degree. Shared by the two flamegraph examples,
/// whose `InputPhase` protocols are identical apart from a field name.
struct InputPhase {
    value: Mersenne61,
}

impl<E: RandEnvironment> Protocol<E> for InputPhase {
    type Output = Vec<ShamirM61>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();
        let mut shares = Vec::new();
        for dealer in env.network().party_ids() {
            let deal = if dealer == me {
                let receivers = env.network().party_ids();
                let degree = receivers.len() - 1;
                PassiveDealShr::dealer(me, self.value, receivers, degree)
            } else {
                PassiveDealShr::receiver(dealer)
            };
            shares.push(deal.execute(env).await?);
        }
        Ok(shares)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("InputPhase")
    }
}

/// Shares `value`, sums every party's share locally and opens the sum.
async fn share_sum_open<E: RandEnvironment>(
    env: &mut E,
    value: Mersenne61,
) -> Result<Mersenne61, Error> {
    let shares: Vec<ShamirM61> = InputPhase { value }.execute(env).await?;
    let sum_share = shares
        .into_iter()
        .reduce(|acc, share| acc + &share)
        .expect("there is at least one share to sum");
    PassiveOpenShr::new(sum_share).execute(env).await
}

struct SecSumShamirShr {
    input: Mersenne61,
}

impl<E: RandEnvironment> Protocol<E> for SecSumShamirShr {
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        share_sum_open(env, self.input).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SecSumShamirShr")
    }
}

#[test]
fn golden_bandwidth_flamegraph() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        (0..3).map(PartyId::from).collect(),
        |party| SecSumShamirShr {
            input: Mersenne61::from(7 + party.as_usize() as u64),
        },
        seeded_env(4),
        vec![],
    );
    check_golden("bandwidth_flamegraph", &render(&outcome));
}

// -------------------------------------------------------------------------------------------------
// examples/secure_stats_flamegraph.rs
// -------------------------------------------------------------------------------------------------

struct SumValues {
    input: Mersenne61,
}

impl<E: RandEnvironment> Protocol<E> for SumValues {
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        share_sum_open(env, self.input).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SumValues")
    }
}

struct SumSquares {
    input: Mersenne61,
}

impl<E: RandEnvironment> Protocol<E> for SumSquares {
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        share_sum_open(env, self.input * &self.input).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SumSquares")
    }
}

struct SecStats {
    input: Mersenne61,
}

impl<E: RandEnvironment> Protocol<E> for SecStats {
    type Output = (Mersenne61, Mersenne61);

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let sum = SumValues { input: self.input }.execute(env).await?;
        let sum_sq = SumSquares { input: self.input }.execute(env).await?;
        Ok((sum, sum_sq))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SecStats")
    }
}

#[test]
fn golden_secure_stats_flamegraph() {
    // The example samples five inputs below 2^20; these are fixed values in that range.
    const INPUTS: [u64; 5] = [31_415, 92_653, 58_979, 32_384, 62_643];
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        (0..INPUTS.len()).map(PartyId::from).collect(),
        |party| SecStats {
            input: Mersenne61::from(INPUTS[party.as_usize()]),
        },
        seeded_env(5),
        vec![],
    );
    check_golden("secure_stats_flamegraph", &render(&outcome));
}

// -------------------------------------------------------------------------------------------------
// examples/secure_covariance/{main.rs, preprocessing.rs}
// -------------------------------------------------------------------------------------------------

const COV_N_PARTIES: usize = 5;
const COV_THRESHOLD: usize = 2;
const COV_VECTOR_LEN: usize = 6;
const COV_OWNER_X: usize = 0;
const COV_OWNER_Y: usize = 1;
const COV_DATA_X: [u64; COV_VECTOR_LEN] = [10, 12, 14, 16, 18, 20];
const COV_DATA_Y: [u64; COV_VECTOR_LEN] = [2, 4, 5, 7, 8, 10];

type TripleM61 = ShamirTriple<1, Mersenne61>;

struct DealVector {
    owner: PartyId,
    values: Option<Vec<Mersenne61>>,
    parties: Vec<PartyId>,
    id: ProtocolId,
}

impl<E: RandEnvironment> Protocol<E> for DealVector {
    type Output = Vec<ShamirM61>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();
        let mut shares = Vec::with_capacity(COV_VECTOR_LEN);
        for i in 0..COV_VECTOR_LEN {
            let deal = if me == self.owner {
                let values = self.values.as_ref().ok_or(Error::Input)?;
                PassiveDealShr::dealer(self.owner, values[i], self.parties.clone(), COV_THRESHOLD)
            } else {
                PassiveDealShr::receiver(self.owner)
            };
            shares.push(deal.execute(env).await?);
        }
        Ok(shares)
    }

    fn id(&self) -> ProtocolId {
        self.id
    }
}

struct Preprocessing {
    parties: Vec<PartyId>,
    king: PartyId,
}

impl<E: RandEnvironment> Protocol<E> for Preprocessing {
    type Output = Vec<TripleM61>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let mut triples: Vec<TripleM61> = Vec::with_capacity(COV_VECTOR_LEN);
        while triples.len() < COV_VECTOR_LEN {
            let a = PassiveRandShr::<1, Mersenne61>::new(COV_THRESHOLD, self.parties.clone())?
                .execute(env)
                .await?;
            let b = PassiveRandShr::<1, Mersenne61>::new(COV_THRESHOLD, self.parties.clone())?
                .execute(env)
                .await?;
            let doubles =
                PassiveRandDoubleShr::<1, Mersenne61>::new(COV_THRESHOLD, self.parties.clone())?
                    .execute(env)
                    .await?;
            let batch = PassiveTriple::new(self.king, self.parties.clone(), a, b, doubles)?
                .execute(env)
                .await?;
            triples.extend(batch);
        }
        triples.truncate(COV_VECTOR_LEN);
        Ok(triples)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Preprocessing")
    }
}

fn centre(shares: &[ShamirM61], inv_len: &Mersenne61) -> Vec<ShamirM61> {
    let sum = shares
        .iter()
        .cloned()
        .reduce(|acc, share| acc + &share)
        .expect("the dataset is not empty");
    let mean = sum * inv_len;
    shares.iter().map(|share| share.clone() - &mean).collect()
}

struct SecureCovariance {
    parties: Vec<PartyId>,
    king: PartyId,
    x: Option<Vec<Mersenne61>>,
    y: Option<Vec<Mersenne61>>,
}

impl<E: RandEnvironment> Protocol<E> for SecureCovariance {
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let x_shares = DealVector {
            owner: self.parties[COV_OWNER_X],
            values: self.x,
            parties: self.parties.clone(),
            id: ProtocolId::from("ShareX"),
        }
        .execute(env)
        .await?;
        let y_shares = DealVector {
            owner: self.parties[COV_OWNER_Y],
            values: self.y,
            parties: self.parties.clone(),
            id: ProtocolId::from("ShareY"),
        }
        .execute(env)
        .await?;

        let triples = Preprocessing {
            parties: self.parties.clone(),
            king: self.king,
        }
        .execute(env)
        .await?;

        let inv_len = Mersenne61::from(COV_VECTOR_LEN as u64)
            .inverse()
            .expect("the vector length is non-zero in the field");
        let centred_x = centre(&x_shares, &inv_len);
        let centred_y = centre(&y_shares, &inv_len);

        let products = PassiveShamirMul::new(
            self.king,
            self.parties.clone(),
            centred_x,
            centred_y,
            triples,
        )?
        .execute(env)
        .await?;

        let scatter = products
            .into_iter()
            .reduce(|acc, product| acc + &product)
            .expect("there is at least one product");
        let covariance = scatter * &inv_len;
        PassiveOpenShr::new(covariance).execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SecureCovariance")
    }
}

#[test]
fn golden_secure_covariance() {
    let parties: Vec<PartyId> = (0..COV_N_PARTIES).map(PartyId::from).collect();
    let king = parties[0];
    let data_x: Vec<Mersenne61> = COV_DATA_X.iter().copied().map(Mersenne61::from).collect();
    let data_y: Vec<Mersenne61> = COV_DATA_Y.iter().copied().map(Mersenne61::from).collect();

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties.clone(),
        |party| SecureCovariance {
            parties: (0..COV_N_PARTIES).map(PartyId::from).collect(),
            king,
            x: (party.as_usize() == COV_OWNER_X).then(|| data_x.clone()),
            y: (party.as_usize() == COV_OWNER_Y).then(|| data_y.clone()),
        },
        seeded_env(6),
        vec![],
    );
    check_golden("secure_covariance", &render(&outcome));
}

// -------------------------------------------------------------------------------------------------
// examples/schnorr_pok.rs (the example's own two-party `SchnorrPok`, not `protocol::zk::schnorr`)
// -------------------------------------------------------------------------------------------------

struct ExampleSchnorrPok<const LIMBS: usize, C: EllipticCurve<LIMBS>> {
    x: Option<C::ScalarField>,
    h: C,
}

impl<const LIMBS: usize, C, E> Protocol<E> for ExampleSchnorrPok<LIMBS, C>
where
    C: EllipticCurve<LIMBS> + Send + Sync + Abbreviate,
    C::ScalarField: Send + Sync + Abbreviate,
    E: RandEnvironment,
{
    type Output = Option<bool>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let other = env.network().other()?;
        if let Some(x) = self.x {
            let r = C::ScalarField::random(env.rng_mut());
            let u = C::gen().scalar_mul(&r);
            let mut pkt = Packet::empty();
            pkt.write_labeled(&u)?;
            env.network_mut().send_to(other, &pkt).await?;

            let mut pkt_c = env.network_mut().recv_from(other).await?;
            let c: C::ScalarField = pkt_c.pop()?;

            let z = r + &(c * &x);
            let mut pkt_z = Packet::empty();
            pkt_z.write_labeled(&z)?;
            env.network_mut().send_to(other, &pkt_z).await?;
            Ok(None)
        } else {
            let mut recv_u_pkt = env.network_mut().recv_from(other).await?;
            let u: C = recv_u_pkt.pop()?;

            let c = C::ScalarField::random(env.rng_mut());
            let mut pkt_c = Packet::empty();
            pkt_c.write_labeled(&c)?;
            env.network_mut().send_to(other, &pkt_c).await?;

            let mut pkt_z = env.network_mut().recv_from(other).await?;
            let z: C::ScalarField = pkt_z.pop()?;

            let left_side = C::gen().scalar_mul(&z);
            let right_side = u.add(&self.h.scalar_mul(&c));
            Ok(Some(left_side == right_side))
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SchnorrPok")
    }
}

#[test]
fn golden_schnorr_pok() {
    let prover = PartyId::from(0);
    let verifier = PartyId::from(1);

    let mut rng = ChaCha20Rng::seed_from_u64(7_000_000);
    let x = Secp256k1ScalarField::random(&mut rng);
    let h = Secp256k1::gen().scalar_mul(&x);

    let outcome = simulate(
        SimpleNetworkConfig::wan(),
        vec![prover, verifier],
        |party| ExampleSchnorrPok {
            x: (party == prover).then_some(x),
            h,
        },
        seeded_env(7),
        vec![],
    );
    check_golden("schnorr_pok", &render(&outcome));
}

// -------------------------------------------------------------------------------------------------
// Timing-sensitive scenarios (from tests/simulator.rs and tests/recv_timeout.rs)
// -------------------------------------------------------------------------------------------------

/// Builds a channel: `Instant` on a self-link, otherwise TCP with the given bandwidth and RTT.
fn tcp_channel(link: Link, bandwidth_bps: usize, rtt_ms: usize) -> ChannelConfig {
    let builder = ChannelConfigBuilder::default();
    let builder = if link.sender() == link.recipient() {
        builder.net_type(NetworkType::Instant)
    } else {
        builder
            .net_type(NetworkType::Tcp)
            .bandwidth(Bandwidth::new(bandwidth_bps))
            .rtt(Rtt::new(rtt_ms))
    };
    // SAFETY: every value above is valid, so the build does not fail.
    builder.build().unwrap()
}

/// `tests/simulator.rs::SlowNetworkConfig`: 50 kbit/s, 500 ms RTT.
#[derive(Debug, Clone)]
struct SlowNetworkConfig;

impl NetworkConfig for SlowNetworkConfig {
    fn channel_config(&self, link: Link) -> ChannelConfig {
        tcp_channel(link, 50_000, 500)
    }
}

/// `tests/simulator.rs::BulkTransferProtocol`: both parties exchange a 200 kB payload.
struct BulkTransfer;

impl<E: Environment> Protocol<E> for BulkTransfer {
    type Output = Vec<u8>;

    async fn run(self, environment: &mut E) -> Result<Vec<u8>, Error> {
        let other = environment.network().other()?;
        let mut packet = Packet::empty();
        packet.write(&vec![0u8; 200_000])?;
        environment.network_mut().send_to(other, &packet).await?;
        let received = environment.network_mut().recv_from(other).await?;
        environment.network_mut().close().await?;
        Ok(received.read(0)?)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("BulkTransferProtocol")
    }
}

#[test]
fn golden_bulk_transfer() {
    let outcome = simulate(
        SlowNetworkConfig,
        vec![PartyId::from(0), PartyId::from(1)],
        |_| BulkTransfer,
        seeded_env(8),
        vec![],
    );
    check_golden("bulk_transfer", &render(&outcome));
}

/// `tests/simulator.rs::QuorumCollect`: party `collector` waits for `quorum` messages through
/// `recv_any`; the parties in `senders` send their id; everyone else stays silent.
struct QuorumCollect {
    collector: usize,
    quorum: usize,
    senders: Vec<usize>,
}

impl<E: Environment> Protocol<E> for QuorumCollect {
    type Output = Vec<usize>;

    async fn run(self, env: &mut E) -> Result<Vec<usize>, Error> {
        let me = env.network().local_party().as_usize();
        let mut heard = Vec::new();
        if me == self.collector {
            for _ in 0..self.quorum {
                let (sender, _packet) = env.network_mut().recv_any().await?;
                heard.push(sender.as_usize());
            }
        } else if self.senders.contains(&me) {
            let mut packet = Packet::empty();
            packet.write(&me)?;
            env.network_mut()
                .send_to(PartyId::from(self.collector), &packet)
                .await?;
        }
        env.network_mut().close().await?;
        heard.sort_unstable();
        Ok(heard)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("QuorumCollect")
    }
}

#[test]
fn golden_quorum_collect() {
    // Four senders and a quorum of three: one message is left queued at the collector.
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        (0..5).map(PartyId::from).collect(),
        |_| QuorumCollect {
            collector: 0,
            quorum: 3,
            senders: vec![1, 2, 3, 4],
        },
        seeded_env(9),
        vec![],
    );
    check_golden("quorum_collect", &render(&outcome));
}

/// `tests/simulator.rs::StragglerNetworkConfig`: links touching party 4 have a 20 s RTT, the
/// others 100 ms; 1 Mbit/s everywhere.
#[derive(Debug, Clone)]
struct StragglerNetworkConfig;

const STRAGGLER: usize = 4;

impl NetworkConfig for StragglerNetworkConfig {
    fn channel_config(&self, link: Link) -> ChannelConfig {
        let slow =
            link.sender().as_usize() == STRAGGLER || link.recipient().as_usize() == STRAGGLER;
        tcp_channel(link, 1_000_000, if slow { 20_000 } else { 100 })
    }
}

/// `tests/simulator.rs::StragglerScenario`: party 0 collects a quorum of 3 from the fast parties
/// 1..=3, while the straggler (4) is delivered later. Party 5 waits specifically for the straggler,
/// which keeps the run alive until then.
struct StragglerScenario;

impl<E: Environment> Protocol<E> for StragglerScenario {
    type Output = Vec<usize>;

    async fn run(self, env: &mut E) -> Result<Vec<usize>, Error> {
        let me = env.network().local_party().as_usize();
        let mut packet = Packet::empty();
        packet.write(&me)?;
        let output = match me {
            0 => {
                let mut heard = Vec::new();
                for _ in 0..3 {
                    let (sender, _packet) = env.network_mut().recv_any().await?;
                    heard.push(sender.as_usize());
                }
                heard.sort_unstable();
                heard
            }
            1..=3 => {
                env.network_mut().send_to(PartyId::from(0), &packet).await?;
                Vec::new()
            }
            STRAGGLER => {
                env.network_mut().send_to(PartyId::from(0), &packet).await?;
                env.network_mut().send_to(PartyId::from(5), &packet).await?;
                Vec::new()
            }
            _ => {
                let received = env
                    .network_mut()
                    .recv_from(PartyId::from(STRAGGLER))
                    .await?;
                vec![received.read(0)?]
            }
        };
        env.network_mut().close().await?;
        Ok(output)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("StragglerScenario")
    }
}

#[test]
fn golden_straggler() {
    let outcome = simulate(
        StragglerNetworkConfig,
        (0..6).map(PartyId::from).collect(),
        |_| StragglerScenario,
        seeded_env(10),
        vec![],
    );
    check_golden("straggler", &render(&outcome));
}

/// Timed receives (`tests/recv_timeout.rs` shapes, combined). Party 1 sends one value at once.
/// Party 0 then makes three timed receives:
///
/// 1. On *any* party for 1 ms. It times out because the link delay is longer, and the late packet
///    stays queued.
/// 2. From party 1 with a 60 s timeout. It gets the late packet; its timer is left stale in the
///    event heap.
/// 3. From party 2, which never sends, for 100 ms. It times out.
struct TimedReceives;

impl<E: Environment> Protocol<E> for TimedReceives {
    /// Party 0: (first receive timed out, the value received, last receive timed out).
    type Output = (bool, usize, bool);

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party().as_usize();
        let output = if me == 0 {
            let first = env
                .network_mut()
                .recv_any_with_timeout(Duration::from_millis(1))
                .await;
            let late = env
                .network_mut()
                .recv_from_with_timeout(PartyId::from(1), Duration::from_secs(60))
                .await?;
            let last = env
                .network_mut()
                .recv_from_with_timeout(PartyId::from(2), Duration::from_millis(100))
                .await;
            (
                matches!(first, Err(NetworkError::Timeout(None))),
                late.read(0)?,
                matches!(last, Err(NetworkError::Timeout(Some(_)))),
            )
        } else if me == 1 {
            let mut packet = Packet::empty();
            packet.write(&42_usize)?;
            env.network_mut().send_to(PartyId::from(0), &packet).await?;
            (false, 0, false)
        } else {
            (false, 0, false)
        };
        env.network_mut().close().await?;
        Ok(output)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("TimedReceives")
    }
}

#[test]
fn golden_timed_receives() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        (0..3).map(PartyId::from).collect(),
        |_| TimedReceives,
        seeded_env(11),
        vec![],
    );
    assert_eq!(
        outcome.outputs[&PartyId::from(0)],
        (true, 42, true),
        "the scenario must exercise both timeouts and the late packet"
    );
    check_golden("timed_receives", &render(&outcome));
}
