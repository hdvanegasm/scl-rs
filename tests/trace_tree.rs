//! Tests of session-aware tracing: how a party's trace is read back into a tree of protocol scopes
//! when the branches of a concurrent combinator interleave in it.
//!
//! - The bandwidth tree gives each branch of a `Join` its own bytes, under the `Join` node.
//! - A `ProtocolEnd` that does not close the scope open in its session makes the trace malformed.
//! - A branch dropped before it finished is closed together with its `Join`.
//! - The trace's `Display` renders each branch as its own block, labelled with its session, and
//!   `SimulationTrace::session` keeps one branch.
//!
//! That a sequential trace still renders exactly as before is checked by `tests/golden.rs`.
//!
//! The bandwidth tree's fields are private, so these tests read it through `write_folded`.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::{
        session::SessionId,
        simulation::{
            channel::{ChannelConfigBuilder, Link, NetworkType, SimpleNetworkConfig},
            event::Event,
            network::SimNetwork,
            simulator::{simulate, SimulationOutcome, SIMULATION_ROOT_TAG},
            SimulationError, SimulationTrace,
        },
        Network, Packet, PartyId,
    },
    protocol::{
        concurrent::Join,
        passive_shamir::{double_rand_share::PassiveRandDoubleShr, rand_share::PassiveRandShr},
        Environment, Error, ForkEnvironment, GeneralEnvironment, Protocol, ProtocolId,
        RandEnvironment,
    },
};

type F = Mersenne61;
type Env = GeneralEnvironment<SimNetwork, ChaCha20Rng>;

/// Number of parties and corruption threshold of the secret-sharing protocols: DN07 needs
/// `n >= 2t + 1`.
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

/// The root session of a simulation.
fn root() -> SessionId {
    SessionId::root(SIMULATION_ROOT_TAG)
}

/// The folded bandwidth tree of `party`, as a map from each path to its bytes.
fn folded<O>(outcome: &SimulationOutcome<O>, party: PartyId) -> BTreeMap<String, usize> {
    let mut out = Vec::new();
    outcome
        .bandwidth_tree_for(party)
        .unwrap()
        .write_folded(&mut out)
        .unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| {
            let (path, bytes) = line.rsplit_once(' ').unwrap();
            (path.to_string(), bytes.parse().unwrap())
        })
        .collect()
}

/// Runs the two preprocessing protocols in one `Join`.
struct Both;

impl<E: RandEnvironment + ForkEnvironment> Protocol<E> for Both {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        Join((
            PassiveRandShr::<1, F>::new(T, parties())?,
            PassiveRandDoubleShr::<1, F>::new(T, parties())?,
        ))
        .execute(env)
        .await?;
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Both")
    }
}

/// Runs one of the two preprocessing protocols alone, discarding its output: `PassiveRandShr`
/// when it holds `true`, `PassiveRandDoubleShr` when it holds `false`.
struct Alone(bool);

impl<E: RandEnvironment> Protocol<E> for Alone {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        if self.0 {
            PassiveRandShr::<1, F>::new(T, parties())?
                .execute(env)
                .await?;
        } else {
            PassiveRandDoubleShr::<1, F>::new(T, parties())?
                .execute(env)
                .await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Alone")
    }
}

/// In a `Join`, the tree gives each branch the bytes that protocol sends when it runs alone,
/// under the `Join` node: the tree of the joined run is the two trees of the runs alone, side by
/// side.
#[test]
fn each_branch_of_a_join_gets_its_own_bytes() {
    let config = SimpleNetworkConfig::default;
    let joined = simulate(config(), parties(), |_| Both, seeded_env, vec![]);
    let first = simulate(config(), parties(), |_| Alone(true), seeded_env, vec![]);
    let second = simulate(config(), parties(), |_| Alone(false), seeded_env, vec![]);

    for p in parties() {
        let mut expected = BTreeMap::new();
        for alone in [&first, &second] {
            for (path, bytes) in folded(alone, p) {
                let below_alone = path.strip_prefix("<simulation>;Alone;").unwrap();
                expected.insert(format!("<simulation>;Both;Join;{below_alone}"), bytes);
            }
        }
        for name in ["PassiveRandShr", "PassiveRandDoubleShr"] {
            let prefix = format!("<simulation>;Both;Join;{name}");
            assert!(
                expected.keys().any(|path| path.starts_with(&prefix)),
                "party {p}: {name} sent nothing when run alone"
            );
        }
        assert_eq!(folded(&joined, p), expected, "party {p}");
    }
}

fn begin(session: &SessionId, name: &'static str) -> Event {
    Event::ProtocolBegin {
        timestamp: Duration::ZERO,
        session: session.clone(),
        protocol_name: ProtocolId::from(name),
    }
}

fn end(session: &SessionId, name: &'static str) -> Event {
    Event::ProtocolEnd {
        timestamp: Duration::ZERO,
        session: session.clone(),
        protocol_name: ProtocolId::from(name),
    }
}

/// A send of `size` bytes from party 0 to party 1.
fn send(session: &SessionId, size: usize) -> Event {
    Event::SendData {
        timestamp: Duration::ZERO,
        session: session.clone(),
        link: Link::new(party(0), party(1)),
        size,
        content_count: vec![("unknown elem.", 1)],
    }
}

/// An outcome holding one hand-built trace, for party 0. A hand-built trace gives exact control
/// over the order of events.
fn outcome_of(events: Vec<Event>) -> SimulationOutcome<()> {
    SimulationOutcome {
        outputs: HashMap::new(),
        traces: HashMap::from([(party(0), SimulationTrace::new(events))]),
    }
}

/// A `ProtocolEnd` whose name is not that of the scope open in its session, or that ends in a
/// session with no scope open, is reported as a malformed trace.
#[test]
fn an_end_with_the_wrong_name_is_a_malformed_trace() {
    let outcome = outcome_of(vec![
        begin(&root(), "A"),
        send(&root(), 1),
        end(&root(), "B"),
    ]);
    let result = outcome.bandwidth_tree_for(party(0));
    assert!(
        matches!(
            result,
            Err(SimulationError::MalformedTrace { event_idx: 2, .. })
        ),
        "expected MalformedTrace at event 2"
    );

    let outcome = outcome_of(vec![end(&root(), "A")]);
    assert!(matches!(
        outcome.bandwidth_tree_for(party(0)),
        Err(SimulationError::MalformedTrace { event_idx: 0, .. })
    ));
}

/// A branch that a fail-fast `Join` dropped never records its end. Closing the `Join` closes it,
/// so what the party sends afterwards is not counted under the dropped branch.
#[test]
fn a_cancelled_branch_is_closed_with_its_join() {
    let (a, b) = (root().with_child(0), root().with_child(1));
    let outcome = outcome_of(vec![
        begin(&root(), "Join"),
        begin(&a, "A"),
        send(&a, 10),
        begin(&b, "B"),
        send(&b, 20),
        end(&b, "B"),
        // `A` is dropped here: no `ProtocolEnd`.
        end(&root(), "Join"),
        begin(&root(), "C"),
        send(&root(), 40),
        end(&root(), "C"),
    ]);
    assert_eq!(
        folded(&outcome, party(0)),
        BTreeMap::from([
            ("<simulation>;Join;A".to_string(), 10),
            ("<simulation>;Join;B".to_string(), 20),
            ("<simulation>;C".to_string(), 40),
        ])
    );
}

/// One round: sends one small packet to every other party, then receives one from each.
struct Round(&'static str);

impl<E: Environment> Protocol<E> for Round {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let network = env.network_mut();
        let local = network.local_party();
        let others: Vec<PartyId> = network
            .party_ids()
            .into_iter()
            .filter(|&p| p != local)
            .collect();
        let mut packet = Packet::empty();
        packet.write(&0u64)?;
        for &other in &others {
            network.send_to(other, &packet).await?;
        }
        for &other in &others {
            network.recv_from(other).await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from(self.0)
    }
}

/// `Left` and `Right` in one `Join`, then `After` alone.
struct JoinThenAfter;

impl<E: Environment + ForkEnvironment> Protocol<E> for JoinThenAfter {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        Join((Round("Left"), Round("Right"))).execute(env).await?;
        Round("After").execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("JoinThenAfter")
    }
}

/// The trace of a `Join` renders each branch as its own block, labelled with its session, with
/// that branch's events inside it; and `session` keeps one branch. The links are instantaneous,
/// so every timestamp is zero and the expected text does not depend on the delay model.
#[test]
fn a_join_renders_one_labelled_block_per_branch() {
    let instant = ChannelConfigBuilder::default()
        .net_type(NetworkType::Instant)
        .build()
        .unwrap();
    let outcome = simulate(
        SimpleNetworkConfig::from_channel_config(instant),
        vec![party(0), party(1)],
        |_| JoinThenAfter,
        seeded_env,
        vec![],
    );
    let trace = &outcome.traces[&party(0)];
    let expected = "\
[     0.000s] START
JoinThenAfter {
    Join {
        Left [session 0] {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
            [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
        }
        Right [session 1] {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
            [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
        }
    }
    After {
        [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
        [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
    }
}
[     0.000s] OUTPUT         []
[     0.000s] STOP";
    assert_eq!(trace.to_string(), expected);

    let right = trace.session(&root().with_child(1));
    let expected_right = "\
Right [session 1] {
    [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
    [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
}";
    assert_eq!(right.to_string(), expected_right);
}
