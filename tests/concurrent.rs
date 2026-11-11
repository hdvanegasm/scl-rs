//! End-to-end tests of concurrent composition (`protocol::concurrent`) on the deterministic
//! simulator: Phase 2 of `docs/concurrent-composition-plan.md`, tests T1, T2, T3, T4, T5, T7 and
//! T10 of its §8.
//!
//! - Secret-shared outputs are checked by what they open to, never against the shares of a
//!   sequential run: forking draws from the parent's RNG, so the same seeds give different shares
//!   under a combinator.
//! - The bandwidth tree and the trace `Display` are not asserted on. Until Phase 4, the tree parser
//!   keeps one stack of protocol scopes per party, so interleaved branches nest wrongly in it.
//!   Elapsed times (`metrics()`) and raw `Event` lists are already correct under concurrency.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use proptest::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::{
        simulation::{
            channel::SimpleNetworkConfig, event::Event, network::SimNetwork, simulator::simulate,
        },
        Network, Packet, PartyId,
    },
    protocol::{
        concurrent::{Join, JoinAll},
        passive_shamir::{
            double_rand_share::PassiveRandDoubleShr,
            mul::PassiveShamirMul,
            rand_share::PassiveRandShr,
            triple::{PassiveTriple, ShamirTriple},
        },
        Environment, Error, GeneralEnvironment, Protocol, ProtocolId, RandEnvironment,
    },
    ss::{
        shamir::{DoubleShare, ShamirSS},
        LinearShare,
    },
};

type F = Mersenne61;
type Share = ShamirSS<1, F>;
type Env = GeneralEnvironment<SimNetwork, ChaCha20Rng>;

/// Number of parties and corruption threshold of the secret-sharing tests: DN07 needs
/// `n >= 2t + 1`, and each run of `PassiveRandShr` or `PassiveRandDoubleShr` yields `n - t = 3`
/// sharings.
const N: usize = 5;
const T: usize = 2;

fn party(index: usize) -> PartyId {
    PartyId::from(index)
}

fn parties() -> Vec<PartyId> {
    (0..N).map(PartyId::from).collect()
}

/// Gives every party its own fixed seed, so that two runs with the same parties are identical.
fn seeded_env(party: PartyId, network: SimNetwork) -> Env {
    GeneralEnvironment::new(network, ChaCha20Rng::seed_from_u64(party.as_usize() as u64))
}

/// Reconstructs the secret behind the share that `get` picks from each party's output.
fn open<O>(outputs: &HashMap<PartyId, O>, get: impl Fn(&O) -> Share) -> F {
    let shares: Vec<Share> = parties().iter().map(|p| get(&outputs[p])).collect();
    <Share as LinearShare>::secret_from_shares(&shares, &parties()).unwrap()
}

/// Returns the message of the panic that `run` raises.
fn panic_message_of<R>(run: impl FnOnce() -> R + std::panic::UnwindSafe) -> String {
    let Err(payload) = std::panic::catch_unwind(run) else {
        panic!("the simulation should have deadlocked");
    };
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(_) => panic!("the deadlock panic should carry a formatted message"),
    }
}

/// Returns each `(protocol, session)` that opened a scope in `events`, sorted, for the protocols
/// named in `names`.
fn scopes(events: &[Event], names: &[&str]) -> Vec<(String, String)> {
    let mut scopes: Vec<(String, String)> = events
        .iter()
        .filter_map(|event| match event {
            Event::ProtocolBegin {
                protocol_name,
                session,
                ..
            } if names.contains(&protocol_name.to_string().as_str()) => {
                Some((protocol_name.to_string(), session.to_string()))
            }
            _ => None,
        })
        .collect();
    scopes.sort();
    scopes
}

fn u64_packet(value: u64) -> Result<Packet, Error> {
    let mut packet = Packet::empty();
    packet.write(&value)?;
    Ok(packet)
}

/// Generates `n - t` multiplication triples, one sub-protocol after another.
async fn triples<E: RandEnvironment>(
    king: PartyId,
    env: &mut E,
) -> Result<Vec<ShamirTriple<1, F>>, Error> {
    let a = PassiveRandShr::<1, F>::new(T, parties())?
        .execute(env)
        .await?;
    let b = PassiveRandShr::<1, F>::new(T, parties())?
        .execute(env)
        .await?;
    let doubles = PassiveRandDoubleShr::<1, F>::new(T, parties())?
        .execute(env)
        .await?;
    PassiveTriple::new(king, parties(), a, b, doubles)?
        .execute(env)
        .await
}

/// A protocol that does nothing: the branch of a party with no part in a sub-protocol.
struct Noop;

impl<E: Environment> Protocol<E> for Noop {
    type Output = ();

    async fn run(self, _env: &mut E) -> Result<(), Error> {
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Noop")
    }
}

/// `rounds` rounds of all-to-all exchange: in each round, every party sends a 32-byte packet to
/// every other party, then receives one from each. All packets have the same size, so every hop
/// takes the same time, and the parties stay in lockstep.
struct Exchange {
    rounds: u32,
}

impl<E: Environment> Protocol<E> for Exchange {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let network = env.network_mut();
        let local = network.local_party();
        let others: Vec<PartyId> = network
            .party_ids()
            .into_iter()
            .filter(|&p| p != local)
            .collect();
        for _ in 0..self.rounds {
            let mut packet = Packet::empty();
            packet.write(&[0u8; 32])?;
            for &other in &others {
                network.send_to(other, &packet).await?;
            }
            for &other in &others {
                network.recv_from(other).await?;
            }
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Exchange")
    }
}

// ---------------------------------------------------------------------------------------------
// T1: correctness.
// ---------------------------------------------------------------------------------------------

/// T1 (a): a `Join` of three protocols of different types, one of which consumes sharings
/// computed before it. The factors and the triples are prepared one after another; then fresh
/// random sharings, double sharings and the products are computed in one `Join`.
struct JoinOfDifferentTypes {
    king: PartyId,
}

/// The factors, the products, the fresh random sharings, and the two halves of each double
/// sharing.
type JoinOfDifferentTypesOutput = (
    Vec<Share>,
    Vec<Share>,
    Vec<Share>,
    Vec<Share>,
    Vec<(Share, Share)>,
);

impl<E: RandEnvironment + scl_rs::protocol::ForkEnvironment> Protocol<E> for JoinOfDifferentTypes {
    type Output = JoinOfDifferentTypesOutput;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let x = PassiveRandShr::<1, F>::new(T, parties())?
            .execute(env)
            .await?;
        let y = PassiveRandShr::<1, F>::new(T, parties())?
            .execute(env)
            .await?;
        let triples = triples(self.king, env).await?;

        let (random, doubles, products) = Join((
            PassiveRandShr::<1, F>::new(T, parties())?,
            PassiveRandDoubleShr::<1, F>::new(T, parties())?,
            PassiveShamirMul::new(self.king, parties(), x.clone(), y.clone(), triples)?,
        ))
        .execute(env)
        .await?;

        let doubles = doubles.into_iter().map(DoubleShare::into_parts).collect();
        Ok((x, y, products, random, doubles))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("JoinOfDifferentTypes")
    }
}

#[test]
fn t1_join_of_different_types_opens_correctly() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties(),
        |_| JoinOfDifferentTypes { king: party(0) },
        seeded_env,
        vec![],
    );

    let count = outcome.outputs[&party(0)].2.len();
    assert_eq!(count, N - T);
    for k in 0..count {
        let x = open(&outcome.outputs, |o| o.0[k].clone());
        let y = open(&outcome.outputs, |o| o.1[k].clone());
        let product = open(&outcome.outputs, |o| o.2[k].clone());
        assert_eq!(product, x * &y, "product {k}: [x · y] != x · y");
        assert_ne!(x, F::from(0u64));
    }

    for k in 0..N - T {
        let random = open(&outcome.outputs, |o| o.3[k].clone());
        assert_ne!(random, F::from(0u64), "random sharing {k} opens to 0");
        assert_eq!(outcome.outputs[&party(0)].3[k].degree(), T);

        let low = open(&outcome.outputs, |o| o.4[k].0.clone());
        let high = open(&outcome.outputs, |o| o.4[k].1.clone());
        assert_eq!(
            low, high,
            "double sharing {k}: the two halves hide different secrets"
        );
    }
}

/// T1 (b): a `JoinAll` of 8 multiplications, each on its own factors and its own triple.
struct JoinAllOfMultiplications {
    king: PartyId,
}

impl<E: RandEnvironment + scl_rs::protocol::ForkEnvironment> Protocol<E>
    for JoinAllOfMultiplications
{
    /// The factors and the products, in input order.
    type Output = (Vec<Share>, Vec<Share>, Vec<Share>);

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        const COUNT: usize = 8;
        let (mut xs, mut ys, mut spare) = (Vec::new(), Vec::new(), Vec::new());
        while xs.len() < COUNT {
            xs.extend(
                PassiveRandShr::<1, F>::new(T, parties())?
                    .execute(env)
                    .await?,
            );
            ys.extend(
                PassiveRandShr::<1, F>::new(T, parties())?
                    .execute(env)
                    .await?,
            );
        }
        while spare.len() < COUNT {
            spare.extend(triples(self.king, env).await?);
        }
        xs.truncate(COUNT);
        ys.truncate(COUNT);

        let multiplications = xs
            .iter()
            .zip(&ys)
            .zip(spare)
            .map(|((x, y), triple)| {
                PassiveShamirMul::new(
                    self.king,
                    parties(),
                    vec![x.clone()],
                    vec![y.clone()],
                    vec![triple],
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let products = JoinAll(multiplications).execute(env).await?;
        let products = products.into_iter().map(|mut p| p.remove(0)).collect();
        Ok((xs, ys, products))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("JoinAllOfMultiplications")
    }
}

#[test]
fn t1_join_all_of_multiplications_opens_correctly() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties(),
        |_| JoinAllOfMultiplications { king: party(0) },
        seeded_env,
        vec![],
    );

    assert_eq!(outcome.outputs[&party(0)].2.len(), 8);
    for k in 0..8 {
        let x = open(&outcome.outputs, |o| o.0[k].clone());
        let y = open(&outcome.outputs, |o| o.1[k].clone());
        let product = open(&outcome.outputs, |o| o.2[k].clone());
        assert_eq!(product, x * &y, "product {k}: [x · y] != x · y");
    }
}

/// T1 (c): `Join`s nested three levels deep.
struct NestedJoins;

/// The random sharings and the halves of the double sharings, from the outermost `Join` inwards.
type NestedJoinsOutput = (
    Vec<Share>,
    Vec<(Share, Share)>,
    Vec<Share>,
    Vec<(Share, Share)>,
);

impl<E: RandEnvironment + scl_rs::protocol::ForkEnvironment> Protocol<E> for NestedJoins {
    type Output = NestedJoinsOutput;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let (outer, (middle, (inner_random, inner_doubles))) = Join((
            PassiveRandShr::<1, F>::new(T, parties())?,
            Join((
                PassiveRandDoubleShr::<1, F>::new(T, parties())?,
                Join((
                    PassiveRandShr::<1, F>::new(T, parties())?,
                    PassiveRandDoubleShr::<1, F>::new(T, parties())?,
                )),
            )),
        ))
        .execute(env)
        .await?;

        let halves =
            |d: Vec<DoubleShare<1, F>>| d.into_iter().map(DoubleShare::into_parts).collect();
        Ok((outer, halves(middle), inner_random, halves(inner_doubles)))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("NestedJoins")
    }
}

/// The outputs open correctly, and each sub-protocol runs in the session that the fork order
/// gives it (`ForkNetwork`, *Session assignment*): the `i`-th fork of a handle, counting from 0, is
/// on its session's child `i`, and a child handle starts its own count at 0.
#[test]
fn t1_nested_joins_open_correctly_in_the_sessions_of_the_fork_order() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties(),
        |_| NestedJoins,
        seeded_env,
        vec![],
    );

    for k in 0..N - T {
        for (name, random) in [
            ("outer", open(&outcome.outputs, |o| o.0[k].clone())),
            ("inner", open(&outcome.outputs, |o| o.2[k].clone())),
        ] {
            assert_ne!(
                random,
                F::from(0u64),
                "{name} random sharing {k} opens to 0"
            );
        }
        for (name, low, high) in [
            (
                "middle",
                open(&outcome.outputs, |o| o.1[k].0.clone()),
                open(&outcome.outputs, |o| o.1[k].1.clone()),
            ),
            (
                "inner",
                open(&outcome.outputs, |o| o.3[k].0.clone()),
                open(&outcome.outputs, |o| o.3[k].1.clone()),
            ),
        ] {
            assert_eq!(low, high, "{name} double sharing {k}: the halves differ");
        }
    }

    let expected: Vec<(String, String)> = {
        let mut expected: Vec<(String, String)> = [
            ("Join", "root"),
            ("PassiveRandShr", "0"),
            ("Join", "1"),
            ("PassiveRandDoubleShr", "1.0"),
            ("Join", "1.1"),
            ("PassiveRandShr", "1.1.0"),
            ("PassiveRandDoubleShr", "1.1.1"),
        ]
        .iter()
        .map(|(name, session)| (name.to_string(), session.to_string()))
        .collect();
        expected.sort();
        expected
    };
    for p in parties() {
        assert_eq!(
            scopes(
                outcome.traces[&p].events(),
                &["Join", "PassiveRandShr", "PassiveRandDoubleShr"]
            ),
            expected,
            "party {p}: sub-protocols ran in unexpected sessions"
        );
    }
}

/// Protocols of different types in one `JoinAll`, through `boxed`.
struct JoinAllOfBoxed;

impl<E: RandEnvironment + scl_rs::protocol::ForkEnvironment> Protocol<E> for JoinAllOfBoxed {
    /// The number of sharings each protocol produced, scaled so the two can be told apart.
    type Output = Vec<usize>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        JoinAll(vec![
            PassiveRandShr::<1, F>::new(T, parties())?.boxed(|shares| shares.len()),
            PassiveRandDoubleShr::<1, F>::new(T, parties())?.boxed(|doubles| 10 * doubles.len()),
        ])
        .execute(env)
        .await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("JoinAllOfBoxed")
    }
}

#[test]
fn t1_join_all_of_boxed_protocols_maps_each_output() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties(),
        |_| JoinAllOfBoxed,
        seeded_env,
        vec![],
    );
    for p in parties() {
        assert_eq!(outcome.outputs[&p], vec![N - T, 10 * (N - T)], "party {p}");
    }
}

/// Each protocol of a `JoinAll` opens its own trace scope, in the session of its fork: `JoinAll`
/// calls `execute` on its protocols, as `Join` does. A boxed protocol opens exactly one scope,
/// under the name of the protocol it was made from.
#[test]
fn t1_join_all_runs_each_protocol_in_its_own_scope_and_session() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties(),
        |_| JoinAllOfBoxed,
        seeded_env,
        vec![],
    );

    let expected: Vec<(String, String)> = {
        let mut expected: Vec<(String, String)> = [
            ("JoinAll", "root"),
            ("PassiveRandShr", "0"),
            ("PassiveRandDoubleShr", "1"),
        ]
        .iter()
        .map(|(name, session)| (name.to_string(), session.to_string()))
        .collect();
        expected.sort();
        expected
    };
    for p in parties() {
        assert_eq!(
            scopes(
                outcome.traces[&p].events(),
                &["JoinAll", "PassiveRandShr", "PassiveRandDoubleShr"]
            ),
            expected,
            "party {p}: the protocols of the `JoinAll` opened unexpected scopes"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// T2: timing.
// ---------------------------------------------------------------------------------------------

/// The preprocessing of `tests/passive_shamir.rs::MulRandomPairs`: four random sharings and one
/// double sharing, then triple generation. Run one after another, or with the five independent
/// steps in one `Join`.
struct Preprocessing {
    king: PartyId,
    concurrent: bool,
}

impl<E: RandEnvironment + scl_rs::protocol::ForkEnvironment> Protocol<E> for Preprocessing {
    /// The factors and the triples, each triple as its three sharings.
    type Output = (Vec<Share>, Vec<Share>, Vec<(Share, Share, Share)>);

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let random = || PassiveRandShr::<1, F>::new(T, parties());
        let (x, y, a, b, doubles) = if self.concurrent {
            Join((
                random()?,
                random()?,
                random()?,
                random()?,
                PassiveRandDoubleShr::<1, F>::new(T, parties())?,
            ))
            .execute(env)
            .await?
        } else {
            (
                random()?.execute(env).await?,
                random()?.execute(env).await?,
                random()?.execute(env).await?,
                random()?.execute(env).await?,
                PassiveRandDoubleShr::<1, F>::new(T, parties())?
                    .execute(env)
                    .await?,
            )
        };
        let triples = PassiveTriple::new(self.king, parties(), a, b, doubles)?
            .execute(env)
            .await?;
        Ok((
            x,
            y,
            triples.into_iter().map(ShamirTriple::into_parts).collect(),
        ))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Preprocessing")
    }
}

/// T2 (a): on the WAN preset (50 ms per hop), the preprocessing takes the sum of its steps'
/// latencies when they run one after another, and its critical path under a `Join`:
///
/// - each `PassiveRandShr` is one hop (50 ms), and `PassiveRandDoubleShr` two (100 ms);
/// - `PassiveTriple` opens to the king and back: two hops for every party but the king, which
///   finishes after the first;
/// - so a party other than the king takes 4 × 50 + 100 + 100 = 400 ms one after another, and
///   max(50, 100) + 100 = 200 ms under a `Join`; the king takes 50 ms less in both.
///
/// The serialization of these small packets over 100 Mbps adds well under 1 ms.
#[test]
fn t2_join_cuts_preprocessing_time_to_the_critical_path() {
    let elapsed = |concurrent: bool| {
        let outcome = simulate(
            SimpleNetworkConfig::wan(),
            parties(),
            move |_| Preprocessing {
                king: party(0),
                concurrent,
            },
            seeded_env,
            vec![],
        );
        for k in 0..N - T {
            let a = open(&outcome.outputs, |o| o.2[k].0.clone());
            let b = open(&outcome.outputs, |o| o.2[k].1.clone());
            let c = open(&outcome.outputs, |o| o.2[k].2.clone());
            assert_eq!(c, a * &b, "triple {k}: c != a · b");
        }
        let metrics = outcome.metrics();
        parties()
            .into_iter()
            .map(|p| metrics.elapsed_time_for(p).unwrap())
            .collect::<Vec<Duration>>()
    };

    let close_to = |measured: Duration, expected_ms: u64| {
        let expected = Duration::from_millis(expected_ms);
        measured >= expected && measured < expected + Duration::from_millis(1)
    };
    // Party 0 is the king.
    for (p, time) in elapsed(false).into_iter().enumerate() {
        let expected = if p == 0 { 350 } else { 400 };
        assert!(close_to(time, expected), "sequential, party {p}: {time:?}");
    }
    for (p, time) in elapsed(true).into_iter().enumerate() {
        let expected = if p == 0 { 150 } else { 200 };
        assert!(close_to(time, expected), "concurrent, party {p}: {time:?}");
    }
}

/// A composition tree for T2 (b): leaves are all-to-all exchanges, inner nodes run their children
/// one after another (`Seq`) or concurrently (`Par`).
#[derive(Clone, Debug)]
enum Tree {
    Leaf(u32),
    Seq(Vec<Tree>),
    Par(Vec<Tree>),
}

impl Tree {
    /// The tree's length in hops: its critical path.
    fn rounds(&self) -> u32 {
        match self {
            Tree::Leaf(rounds) => *rounds,
            Tree::Seq(children) => children.iter().map(Tree::rounds).sum(),
            Tree::Par(children) => children.iter().map(Tree::rounds).max().unwrap_or(0),
        }
    }
}

impl Protocol<Env> for Tree {
    type Output = ();

    fn run(self, env: &mut Env) -> impl Future<Output = Result<(), Error>> + Send {
        // A tree's future contains its subtrees' futures. Boxing it keeps the type finite.
        let future: Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>> =
            Box::pin(async move {
                match self {
                    Tree::Leaf(rounds) => Exchange { rounds }.execute(env).await,
                    Tree::Seq(children) => {
                        for child in children {
                            child.execute(env).await?;
                        }
                        Ok(())
                    }
                    // Two and three children go through `Join`, more through `JoinAll`.
                    Tree::Par(mut children) => match children.len() {
                        2 => {
                            let b = children.pop().unwrap();
                            let a = children.pop().unwrap();
                            Join((a, b)).execute(env).await.map(|_| ())
                        }
                        3 => {
                            let c = children.pop().unwrap();
                            let b = children.pop().unwrap();
                            let a = children.pop().unwrap();
                            Join((a, b, c)).execute(env).await.map(|_| ())
                        }
                        _ => JoinAll(children).execute(env).await.map(|_| ()),
                    },
                }
            });
        future
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Tree")
    }
}

fn trees() -> impl Strategy<Value = Tree> {
    (1u32..=3)
        .prop_map(Tree::Leaf)
        .prop_recursive(3, 24, 4, |subtree| {
            prop_oneof![
                prop::collection::vec(subtree.clone(), 1..=3).prop_map(Tree::Seq),
                prop::collection::vec(subtree, 2..=4).prop_map(Tree::Par),
            ]
        })
}

/// Every party's elapsed time running `tree` on three parties.
fn elapsed_times(tree: &Tree) -> Vec<Duration> {
    let parties: Vec<PartyId> = (0..3).map(PartyId::from).collect();
    let tree = tree.clone();
    let outcome = simulate(
        SimpleNetworkConfig::wan(),
        parties.clone(),
        move |_| tree.clone(),
        seeded_env,
        vec![],
    );
    let metrics = outcome.metrics();
    parties
        .into_iter()
        .map(|p| metrics.elapsed_time_for(p).unwrap())
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// T2 (b): with every hop taking the same time `hop`, a party's elapsed time is exactly the
    /// tree's critical path: `Seq` adds its children's rounds, `Par` takes their maximum.
    #[test]
    fn t2_elapsed_time_is_the_critical_path(tree in trees()) {
        let hop = elapsed_times(&Tree::Leaf(1))[0];
        prop_assert!(hop > Duration::ZERO);
        for (p, time) in elapsed_times(&tree).into_iter().enumerate() {
            prop_assert_eq!(time, hop * tree.rounds(), "party {}", p);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// T3, T4, T5: session routing, end to end through `Join`.
// ---------------------------------------------------------------------------------------------

/// Branch A of T3. Party 0 waits for a ping from party 1 in this session, then sends 1. Party 1
/// sends the ping, then waits for the value.
struct IsolationA;

impl<E: Environment> Protocol<E> for IsolationA {
    type Output = u64;

    async fn run(self, env: &mut E) -> Result<u64, Error> {
        let network = env.network_mut();
        if network.local_party() == party(0) {
            network.recv_from(party(1)).await?;
            network.send_to(party(1), &u64_packet(1)?).await?;
            Ok(0)
        } else {
            network.send_to(party(0), &Packet::empty()).await?;
            Ok(network.recv_from(party(0)).await?.read(0)?)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("IsolationA")
    }
}

/// Branch B of T3. Party 0 sends 2 at once; party 1 waits for it.
struct IsolationB;

impl<E: Environment> Protocol<E> for IsolationB {
    type Output = u64;

    async fn run(self, env: &mut E) -> Result<u64, Error> {
        let network = env.network_mut();
        if network.local_party() == party(0) {
            network.send_to(party(1), &u64_packet(2)?).await?;
            Ok(0)
        } else {
            Ok(network.recv_from(party(0)).await?.read(0)?)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("IsolationB")
    }
}

/// T3: B's value reaches party 1 first, while party 1's branch A is already waiting on the same
/// link. Both values are `u64`, so a mix-up would deserialize without error; each branch must
/// still get its own value.
#[test]
fn t3_each_branch_receives_only_its_own_session() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        vec![party(0), party(1)],
        |_| Join((IsolationA, IsolationB)),
        seeded_env,
        vec![],
    );
    assert_eq!(outcome.outputs[&party(1)], (1, 2));
}

/// One branch of T4. Party 0 sends `value`; party 1 receives it, with `recv_any` or `recv_from`.
struct WakeBranch {
    value: u64,
    any: bool,
}

impl<E: Environment> Protocol<E> for WakeBranch {
    type Output = u64;

    async fn run(self, env: &mut E) -> Result<u64, Error> {
        let network = env.network_mut();
        if network.local_party() == party(0) {
            network.send_to(party(1), &u64_packet(self.value)?).await?;
            return Ok(self.value);
        }
        let packet = if self.any {
            network.recv_any().await?.1
        } else {
            network.recv_from(party(0)).await?
        };
        Ok(packet.read(0)?)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("WakeBranch")
    }
}

/// Runs a `recv_any` branch and a `recv_from` branch concurrently, in both poll orders.
struct LostWakeup;

impl<E: Environment + scl_rs::protocol::ForkEnvironment> Protocol<E> for LostWakeup {
    type Output = ((u64, u64), (u64, u64));

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let any_first = Join((
            WakeBranch {
                value: 10,
                any: true,
            },
            WakeBranch {
                value: 20,
                any: false,
            },
        ))
        .execute(env)
        .await?;
        let from_first = Join((
            WakeBranch {
                value: 30,
                any: false,
            },
            WakeBranch {
                value: 40,
                any: true,
            },
        ))
        .execute(env)
        .await?;
        Ok((any_first, from_first))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("LostWakeup")
    }
}

/// T4: both receives park on the same link, in different sessions, before either packet arrives.
/// Resolving one must not drop the other's wake-up, whichever is polled first.
#[test]
fn t4_concurrent_receives_on_one_link_are_both_woken() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        vec![party(0), party(1)],
        |_| LostWakeup,
        seeded_env,
        vec![],
    );
    assert_eq!(outcome.outputs[&party(1)], ((10, 20), (30, 40)));
}

/// The fourth branch of T5. Party 0 sends 7; party 1 receives it.
struct EarlyBranch;

impl<E: Environment> Protocol<E> for EarlyBranch {
    type Output = u64;

    async fn run(self, env: &mut E) -> Result<u64, Error> {
        let network = env.network_mut();
        if network.local_party() == party(0) {
            network.send_to(party(1), &u64_packet(7)?).await?;
            Ok(7)
        } else {
            Ok(network.recv_from(party(0)).await?.read(0)?)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("EarlyBranch")
    }
}

/// Party 0 sends in session 3 and then in the root session. Party 1 receives in the root session
/// first, and forks session 3 only afterwards.
struct EarlyArrival;

impl<E: Environment + scl_rs::protocol::ForkEnvironment> Protocol<E> for EarlyArrival {
    type Output = u64;

    async fn run(self, env: &mut E) -> Result<u64, Error> {
        let branches = || Join((Noop, Noop, Noop, EarlyBranch));
        if env.network().local_party() == party(0) {
            let (_, _, _, value) = branches().execute(env).await?;
            env.network_mut()
                .send_to(party(1), &u64_packet(99)?)
                .await?;
            Ok(value)
        } else {
            let done: u64 = env.network_mut().recv_from(party(0)).await?.read(0)?;
            assert_eq!(done, 99);
            let (_, _, _, value) = branches().execute(env).await?;
            Ok(value)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("EarlyArrival")
    }
}

/// T5: the link is first-in, first-out, so party 0's packet in session 3 reaches party 1 before
/// the root-session packet that party 1 waits for before it forks. The packet waits in a mailbox
/// of a session party 1 has not forked yet, and branch 3 takes it once it does. Both parties fork
/// four times in the root session, so the symmetric-forking rule holds.
#[test]
fn t5_a_packet_for_a_session_not_yet_forked_waits_for_the_fork() {
    let outcome = simulate(
        SimpleNetworkConfig::default(),
        vec![party(0), party(1)],
        |_| EarlyArrival,
        seeded_env,
        vec![],
    );
    assert_eq!(outcome.outputs[&party(1)], 7);
}

// ---------------------------------------------------------------------------------------------
// T7: determinism.
// ---------------------------------------------------------------------------------------------

/// Triple generation with its three independent steps in one `Join`.
struct JoinTriples {
    king: PartyId,
}

impl<E: RandEnvironment + scl_rs::protocol::ForkEnvironment> Protocol<E> for JoinTriples {
    type Output = Vec<(Share, Share, Share)>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let (a, b, doubles) = Join((
            PassiveRandShr::<1, F>::new(T, parties())?,
            PassiveRandShr::<1, F>::new(T, parties())?,
            PassiveRandDoubleShr::<1, F>::new(T, parties())?,
        ))
        .execute(env)
        .await?;
        let triples = PassiveTriple::new(self.king, parties(), a, b, doubles)?
            .execute(env)
            .await?;
        Ok(triples.into_iter().map(ShamirTriple::into_parts).collect())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("JoinTriples")
    }
}

/// T7: the same seeds give the same events and outputs, including through a `JoinAll` of more
/// than 30 protocols, where `try_join_all` switches to a `FuturesOrdered` with one waker per child.
#[test]
fn t7_same_seeds_give_the_same_events_and_outputs() {
    const COUNT: usize = 40;
    let run = || {
        simulate(
            SimpleNetworkConfig::default(),
            parties(),
            |_| JoinAll((0..COUNT).map(|_| JoinTriples { king: party(0) }).collect()),
            seeded_env,
            vec![],
        )
    };
    let (first, second) = (run(), run());

    for p in parties() {
        assert_eq!(
            first.traces[&p].events(),
            second.traces[&p].events(),
            "party {p}: the events differ between two runs"
        );
        assert_eq!(first.outputs[&p], second.outputs[&p], "party {p}");
    }

    // The outputs are also correct, not only equal.
    assert_eq!(first.outputs[&party(0)].len(), COUNT);
    for i in 0..COUNT {
        for k in 0..N - T {
            let a = open(&first.outputs, |o| o[i][k].0.clone());
            let b = open(&first.outputs, |o| o[i][k].1.clone());
            let c = open(&first.outputs, |o| o[i][k].2.clone());
            assert_eq!(c, a * &b, "instance {i}, triple {k}: c != a · b");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// T10: violations of symmetric forking.
// ---------------------------------------------------------------------------------------------

/// Only party 0, the king, runs the two exchanges in a `Join`; the others run them one after
/// another in the root session.
struct OnlyTheKingForks;

impl<E: Environment + scl_rs::protocol::ForkEnvironment> Protocol<E> for OnlyTheKingForks {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        if env.network().local_party() == party(0) {
            Join((Exchange { rounds: 1 }, Exchange { rounds: 1 }))
                .execute(env)
                .await?;
        } else {
            Exchange { rounds: 1 }.execute(env).await?;
            Exchange { rounds: 1 }.execute(env).await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("OnlyTheKingForks")
    }
}

/// T10 (a): the king's packets land in sessions 0 and 1, which the other parties never fork, and
/// theirs land in the king's root session, which the king has left for sessions 0 and 1. The
/// report shows both.
#[test]
fn t10_only_the_king_forking_deadlocks_with_the_orphaned_sessions_named() {
    let message = panic_message_of(|| {
        simulate(
            SimpleNetworkConfig::default(),
            (0..3).map(PartyId::from).collect(),
            |_| OnlyTheKingForks,
            seeded_env,
            vec![],
        )
    });

    assert!(message.starts_with("scheduler: "), "{message}");
    for line in [
        "\tparty 1 in session 0: 1 packet(s) from party 0; party 1 is waiting in other sessions\n",
        "\tparty 2 in session 1: 1 packet(s) from party 0; party 2 is waiting in other sessions\n",
        "\tparty 0 in session root: 1 packet(s) from party 1; party 0 is waiting in other sessions\n",
    ] {
        assert!(message.contains(line), "missing {line:?} in:\n{message}");
    }
}

/// One exchange between parties 0 and 1 only.
struct PairExchange;

impl<E: Environment> Protocol<E> for PairExchange {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let network = env.network_mut();
        let other = if network.local_party() == party(0) {
            party(1)
        } else {
            party(0)
        };
        network.send_to(other, &Packet::empty()).await?;
        network.recv_from(other).await?;
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PairExchange")
    }
}

/// Parties 0 and 1 run a first `Join` that party 2, a bystander, skips; then all three run a
/// second `Join`.
struct BystanderSkipsAJoin;

impl<E: Environment + scl_rs::protocol::ForkEnvironment> Protocol<E> for BystanderSkipsAJoin {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        if env.network().local_party() != party(2) {
            Join((PairExchange, PairExchange)).execute(env).await?;
        }
        Join((Exchange { rounds: 1 }, Exchange { rounds: 1 }))
            .execute(env)
            .await?;
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("BystanderSkipsAJoin")
    }
}

/// T10 (b): the first `Join` completes, since only its participants communicate in it. The second
/// one deadlocks: the participants run it in sessions 2 and 3, the bystander in sessions 0 and 1.
#[test]
fn t10_a_bystander_skipping_a_join_deadlocks_the_next_join() {
    let message = panic_message_of(|| {
        simulate(
            SimpleNetworkConfig::default(),
            (0..3).map(PartyId::from).collect(),
            |_| BystanderSkipsAJoin,
            seeded_env,
            vec![],
        )
    });

    assert!(message.starts_with("scheduler: "), "{message}");
    for line in [
        "\tparty 2 in session 2: 1 packet(s) from party 0; party 2 is waiting in other sessions\n",
        "\tparty 2 in session 3: 1 packet(s) from party 1; party 2 is waiting in other sessions\n",
        "\tparty 0 in session 0: 1 packet(s) from party 2; party 0 is waiting in other sessions\n",
        "\tparty 1 in session 1: 1 packet(s) from party 2; party 1 is waiting in other sessions\n",
    ] {
        assert!(message.contains(line), "missing {line:?} in:\n{message}");
    }
}
