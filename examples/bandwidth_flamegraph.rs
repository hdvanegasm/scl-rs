//! Profiling an MPC protocol's bandwidth and rendering it as a flamegraph.
//!
//! Every simulation records, per party, a time-ordered trace of events — including a
//! `ProtocolBegin`/`ProtocolEnd` bracket around every (sub-)protocol call and one `SendData` event
//! per packet handed to the network. From that trace alone,
//! [`SimulationOutcome::bandwidth_tree_for`](scl_rs::net::simulation::simulator::SimulationOutcome::bandwidth_tree_for)
//! reconstructs the *call tree* of protocols after the run, attributing every sent byte to the
//! innermost protocol that was running when it was sent. The protocols themselves need no
//! instrumentation, and only real bandwidth is counted: a party sending to itself is excluded,
//! since that never touches the wire.
//!
//! [`ProtocolBandwidthTree::write_folded`](scl_rs::net::simulation::simulator::ProtocolBandwidthTree::write_folded)
//! then serializes the tree in the folded-stacks format of Brendan Gregg's flamegraph tooling
//! (<https://www.brendangregg.com/flamegraphs.html>) — one line per call path, holding the bytes
//! that path sent itself:
//!
//! ```text
//! <simulation>;SecSumShamirShr;InputPhase;PassiveDealLinearShr 20
//! ```
//!
//! Any standard renderer turns that file into an interactive SVG. This example runs a three-party
//! secure summation over Shamir secret shares, writes every party's stacks into one
//! `bandwidth.folded` (renderers sum duplicate paths, so the graph shows network-wide totals), and
//! prints the [`inferno`](https://github.com/jonhoo/inferno) command that renders it.
//!
//! The computation is deliberately composed of nested protocols so the flamegraph has depth:
//!
//! 1. `SecSumShamirShr` — the top level: share the inputs, add the shares locally (free — Shamir
//!    sharing is linear), and open the sum.
//! 2. `InputPhase` — every party in turn acts as a dealer, distributing a Shamir sharing of its
//!    private input with the library's `PassiveDealShr`.
//! 3. `PassiveOpenLinearShr` — the parties reveal their shares of the sum and reconstruct it.

use std::fs;

use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::{simulation::channel::SimpleNetworkConfig, Network, PartyId},
    prelude::{simulate, Environment, Error, GeneralEnv, Protocol, ProtocolId, Ring},
    protocol::share::{deal::PassiveDealShr, open::PassiveOpenShr},
    ss::shamir::ShamirSS,
};

/// Shamir secret sharing over the Mersenne-61 field (one limb).
type Share = ShamirSS<1, Mersenne61>;

// Middle layer: every party, in turn, deals a Shamir sharing of its private input. After the loop
// each party holds one share of every party's input. This exists as its own protocol (rather than
// inlined in the top level) so the "input distribution" cost shows up as a distinct frame in the
// flamegraph, with the individual deals nested under it.
struct InputPhase {
    input: Mersenne61,
}

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for InputPhase {
    // One share per party in the computation.
    type Output = Vec<Share>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();
        let mut shares = Vec::new();
        for dealer in env.network().party_ids() {
            let deal = if dealer == me {
                PassiveDealShr::dealer(me, self.input, env.network().party_ids())
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

// Top level: secure summation over Shamir shares. Sharing the inputs and opening the result are
// the only steps that communicate; the addition itself is local, because the sum of shares is a
// valid share of the sum.
struct SecSumShamirShr {
    input: Mersenne61,
}

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SecSumShamirShr {
    // The sum of every party's input, learned by everyone.
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let shares: Vec<Share> = InputPhase { input: self.input }.execute(env).await?;

        let sum_share = shares
            .into_iter()
            .reduce(|acc, share| acc + &share)
            .expect("there is at least one share to sum");

        PassiveOpenShr::new(sum_share).execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SecSumShamirShr")
    }
}

fn main() {
    let parties: Vec<PartyId> = (0..3).map(PartyId::from).collect();

    // Run the summation on the deterministic simulator, giving every party a fresh random field
    // element as its private input. No hooks are registered: the bandwidth profile below is
    // reconstructed purely from the traces the simulator records anyway.
    let outcome = simulate(
        SimpleNetworkConfig,
        parties.clone(),
        |_| SecSumShamirShr {
            input: Mersenne61::random(&mut rand::rng()),
        },
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    println!(
        "every party learned the sum: {:?}\n",
        outcome.outputs[&parties[0]]
    );

    // Serialize every party's bandwidth call tree into one folded-stacks buffer. `write_folded`
    // takes any `io::Write` sink — a `Vec<u8>` here, but a `File` or `Stdout` work the same — and
    // concatenating parties is fine: renderers sum duplicate paths, yielding network-wide totals.
    let mut folded = Vec::new();
    for party in &parties {
        outcome
            .bandwidth_tree_for(*party)
            .expect("the party was part of the simulation")
            .write_folded(&mut folded)
            .expect("writing to a Vec cannot fail");
    }
    fs::write("bandwidth.folded", &folded).expect("bandwidth.folded must be writable");

    println!(
        "wrote bandwidth.folded:\n{}",
        String::from_utf8_lossy(&folded)
    );
    println!("render it as an SVG flamegraph with inferno (https://github.com/jonhoo/inferno):");
    println!("    cargo install inferno");
    println!(
        "    inferno-flamegraph --countname bytes --title \"Secure sum: bandwidth by protocol\" \\"
    );
    println!("        < bandwidth.folded > bandwidth.svg");
}
