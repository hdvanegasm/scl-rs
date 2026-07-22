//! Secure **covariance** between two parties' private datasets — an arithmetic circuit with real
//! secure multiplication, evaluated with the DN07 protocols in
//! [`scl_rs::protocol::passive_shamir`].
//!
//! Two organisations (think two hospitals, or a bank and a retailer) each hold a private vector of
//! measurements over the same six subjects. They want the covariance of the two vectors and nothing
//! else: neither the other party's raw values, nor its mean. Three further parties take part in the
//! computation without contributing data, giving `n = 5` parties tolerating `t = 2` passive
//! corruptions (DN07 needs `n >= 2t + 1`).
//!
//! # Why this circuit needs secure multiplication
//!
//! `examples/secure_stats_flamegraph.rs` computes mean and variance while *dodging* multiplication:
//! each party squares its own input before sharing it, which is sound only because a party knows its
//! own value. That dodge is unavailable here. Covariance multiplies `xᵢ` (held by one party) with
//! `yᵢ` (held by the other), so **nobody** can compute the product locally — the operands live on
//! different machines. It has to be an interactive multiplication on secret-shared values, which is
//! exactly what DN07 provides.
//!
//! # The circuit
//!
//! With `x̄` and `ȳ` the two means, the covariance is `(1/ℓ) · Σ (xᵢ − x̄)(yᵢ − ȳ)`:
//!
//! | step                                                   | cost                          |
//! |--------------------------------------------------------|-------------------------------|
//! | each owner deals its `ℓ` values                        | one dealing round each        |
//! | `x̄ = (Σ[xᵢ]) · ℓ⁻¹`, `[uᵢ] = [xᵢ] − [x̄]` (same for `y`) | **free** — linear, no messages |
//! | `ℓ` products `[uᵢ · vᵢ]`                               | **one** round, whatever `ℓ` is |
//! | `Σ[uᵢvᵢ]`, then `· ℓ⁻¹`                                | **free**                      |
//! | open the covariance                                    | one round                     |
//!
//! Additions, subtractions and multiplications *by a public constant* (including `ℓ⁻¹`, the field
//! inverse of the vector length) are all local: Shamir sharing is linear, so they cost nothing. Only
//! the `ℓ` share-by-share products need communication, and because they all sit at the same depth of
//! the circuit they are multiplied in a **single** batch — one round for six products, and it would
//! still be one round for six thousand.
//!
//! # Offline / online
//!
//! Beaver multiplication spends one triple per product, and triples do not depend on the inputs. The
//! example makes that split explicit — a `Preprocessing` phase generating six triples (`Random`,
//! `Random`, `Double-Random`, then triple generation), and an online phase that spends them. The
//! bandwidth flamegraph this example writes puts numbers on the split: of the 3,633 bytes that cross
//! the wire, `Preprocessing` is the largest phase at 57%, against 25% for the online
//! `PassiveShamirMul` that spends its output (13% goes to sharing the inputs, 5% to revealing the
//! result). The point is not that the online phase is negligible — at `n = 5` it plainly is not —
//! but that the majority of the traffic depends on **nothing**, so it can be generated before the
//! data exists and lifted off the critical path entirely.
//!
//! Run it with `cargo run --example secure_covariance`.

mod preprocessing;

use std::fs;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::field::{mersenne61::Mersenne61, FiniteField},
    net::{simulation::channel::SimpleNetworkConfig, Network, PartyId},
    prelude::{simulate, Error, GeneralEnv, Protocol, ProtocolId, RandEnvironment},
    protocol::{
        passive_shamir::{mul::PassiveShamirMul, triple::ShamirTriple},
        share::{deal::PassiveDealShr, open::PassiveOpenShr},
    },
    ss::shamir::ShamirSS,
};

/// Five parties tolerating two passive corruptions: `n >= 2t + 1`, as DN07 requires.
const N_PARTIES: usize = 5;
const THRESHOLD: usize = 2;

/// How many measurements each data owner holds — and therefore how many secure multiplications the
/// circuit needs, all of them in one round.
const VECTOR_LEN: usize = 6;

type Share = ShamirSS<1, Mersenne61>;
type Triple = ShamirTriple<1, Mersenne61>;

/// The two parties that actually hold data. The other three take part without contributing.
const OWNER_X: usize = 0;
const OWNER_Y: usize = 1;

/// The two private datasets. Chosen so the means (15 and 6) and the covariance (9) are whole
/// numbers, which lets the harness cross-check the MPC result against plain integer arithmetic.
const DATA_X: [u64; VECTOR_LEN] = [10, 12, 14, 16, 18, 20];
const DATA_Y: [u64; VECTOR_LEN] = [2, 4, 5, 7, 8, 10];

/// Deals one owner's whole vector: the owner secret-shares each of its `VECTOR_LEN` values, and
/// every other party receives a share of each. The `id` is a constructor argument so the two owners'
/// dealing phases appear as *distinct* towers in the flamegraph — identical ids would merge them.
struct DealVector {
    owner: PartyId,
    /// The private data — `Some` only at the owner.
    values: Option<Vec<Mersenne61>>,
    parties: Vec<PartyId>,
    id: ProtocolId,
}

impl<E: RandEnvironment> Protocol<E> for DealVector {
    /// One share of each of the owner's values.
    type Output = Vec<Share>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();
        let mut shares = Vec::with_capacity(VECTOR_LEN);

        for i in 0..VECTOR_LEN {
            let deal = if me == self.owner {
                let values = self.values.as_ref().ok_or(Error::Input)?;
                PassiveDealShr::dealer(self.owner, values[i], self.parties.clone(), THRESHOLD)
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

/// Centres a vector of sharings on its own (secret) mean: `[uᵢ] = [xᵢ] − [x̄]`.
///
/// Entirely local. The mean is never revealed — it stays a sharing, and subtracting one sharing from
/// another is a local operation on Shamir shares. Scaling by `inv_len` (a *public* constant, the
/// field inverse of the vector length) is local too.
fn centre(shares: &[Share], inv_len: &Mersenne61) -> Vec<Share> {
    let sum = shares
        .iter()
        .cloned()
        .reduce(|acc, share| acc + &share)
        .expect("the dataset is not empty");
    let mean = sum * inv_len;

    shares.iter().map(|share| share.clone() - &mean).collect()
}

/// The whole computation, top to bottom.
struct SecureCovariance {
    parties: Vec<PartyId>,
    king: PartyId,
    /// This party's dataset, if it owns one.
    x: Option<Vec<Mersenne61>>,
    y: Option<Vec<Mersenne61>>,
}

impl<E: RandEnvironment> Protocol<E> for SecureCovariance {
    /// The covariance — the one and only value anybody learns.
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        // --- Input phase: each owner shares its vector. -------------------------------------
        let x_shares = DealVector {
            owner: self.parties[OWNER_X],
            values: self.x,
            parties: self.parties.clone(),
            id: ProtocolId::from("ShareX"),
        }
        .execute(env)
        .await?;

        let y_shares = DealVector {
            owner: self.parties[OWNER_Y],
            values: self.y,
            parties: self.parties.clone(),
            id: ProtocolId::from("ShareY"),
        }
        .execute(env)
        .await?;

        // --- Offline phase: triples, independent of the data above. --------------------------
        let triples = preprocessing::Preprocessing {
            parties: self.parties.clone(),
            king: self.king,
        }
        .execute(env)
        .await?;

        // --- Local phase: centre both vectors on their secret means. Free. -------------------
        let inv_len = Mersenne61::from(VECTOR_LEN as u64)
            .inverse()
            .expect("the vector length is non-zero in the field");
        let centred_x = centre(&x_shares, &inv_len);
        let centred_y = centre(&y_shares, &inv_len);

        // --- Online phase: every product in a single round. -----------------------------------
        let products = PassiveShamirMul::new(
            self.king,
            self.parties.clone(),
            centred_x,
            centred_y,
            triples,
        )?
        .execute(env)
        .await?;

        // --- Local phase: sum the products and scale. Free. -----------------------------------
        let scatter = products
            .into_iter()
            .reduce(|acc, product| acc + &product)
            .expect("there is at least one product");
        let covariance = scatter * &inv_len;

        // --- Output phase: the only thing anyone learns. ---------------------------------------
        PassiveOpenShr::new(covariance).execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SecureCovariance")
    }
}

fn main() {
    let parties: Vec<PartyId> = (0..N_PARTIES).map(PartyId::from).collect();
    let king = parties[0];

    let data_x: Vec<Mersenne61> = DATA_X.iter().copied().map(Mersenne61::from).collect();
    let data_y: Vec<Mersenne61> = DATA_Y.iter().copied().map(Mersenne61::from).collect();

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties.clone(),
        |pid| SecureCovariance {
            parties: (0..N_PARTIES).map(PartyId::from).collect(),
            king,
            x: (pid.as_usize() == OWNER_X).then(|| data_x.clone()),
            y: (pid.as_usize() == OWNER_Y).then(|| data_y.clone()),
        },
        |_, net| GeneralEnv::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    // Cross-check in the clear. The datasets were chosen so that both means and the covariance are
    // whole numbers, so the field result must equal the plain integer one exactly.
    let len = VECTOR_LEN as i64;
    let x: Vec<i64> = DATA_X.iter().map(|&v| v as i64).collect();
    let y: Vec<i64> = DATA_Y.iter().map(|&v| v as i64).collect();
    let mean_x = x.iter().sum::<i64>() / len;
    let mean_y = y.iter().sum::<i64>() / len;
    let scatter: i64 = x
        .iter()
        .zip(&y)
        .map(|(xi, yi)| (xi - mean_x) * (yi - mean_y))
        .sum();
    let expected = scatter / len;

    for party in &parties {
        assert_eq!(
            outcome.outputs[party],
            Mersenne61::from(expected as u64),
            "every party must learn the same covariance"
        );
    }

    println!("party {OWNER_X}'s data: {DATA_X:?}  (mean {mean_x}, private)");
    println!("party {OWNER_Y}'s data: {DATA_Y:?}  (mean {mean_y}, private)");
    println!("\nall {N_PARTIES} parties learned: covariance = {expected} — and nothing else.");
    println!(
        "{VECTOR_LEN} secure multiplications, all in a single online round; \
         everything else was local.\n"
    );

    // Export every party's bandwidth call tree into one folded-stacks file; renderers sum duplicate
    // paths, so the graph shows network-wide totals per call path.
    let mut folded = Vec::new();
    for party in &parties {
        outcome
            .bandwidth_tree_for(*party)
            .expect("the party was part of the simulation")
            .write_folded(&mut folded)
            .expect("writing to a Vec cannot fail");
    }
    fs::write("covariance_bandwidth.folded", &folded)
        .expect("covariance_bandwidth.folded must be writable");

    println!(
        "wrote covariance_bandwidth.folded:\n{}",
        String::from_utf8_lossy(&folded)
    );
    println!("render it as an SVG flamegraph with inferno (https://github.com/jonhoo/inferno):");
    println!("    cargo install inferno");
    println!(
        "    inferno-flamegraph --countname bytes --title \"Secure covariance: bandwidth by protocol\" \\"
    );
    println!("        < covariance_bandwidth.folded > covariance_bandwidth.svg");
}
