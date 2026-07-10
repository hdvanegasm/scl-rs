//! A multi-level bandwidth flamegraph: secure mean and variance over Shamir shares.
//!
//! `examples/bandwidth_flamegraph.rs` introduces the post-hoc bandwidth profiler
//! ([`SimulationOutcome::bandwidth_tree_for`](scl_rs::net::simulation::simulator::SimulationOutcome::bandwidth_tree_for)
//! and the folded-stacks export). This example composes protocols one level deeper, so the
//! rendered flamegraph shows the classic *two-tower* shape — six frame levels, with the cost of
//! each phase attributed to the exact call path that spent it:
//!
//! ```text
//! <simulation>;SecStats;SumValues;InputPhase;PassiveDealLinearShr   ...
//! <simulation>;SecStats;SumValues;PassiveOpenLinearShr              ...
//! <simulation>;SecStats;SumSquares;InputPhase;PassiveDealLinearShr  ...
//! <simulation>;SecStats;SumSquares;PassiveOpenLinearShr             ...
//! ```
//!
//! The computation is *secure mean and variance*: each of the five parties holds a private value
//! (think of a salary benchmark), and together they learn the mean and variance of the values —
//! and nothing else. The interesting design point is that `Var(x) = E[x²] − E[x]²` seems to need
//! secure multiplication (squaring), which requires an interactive protocol (e.g. Beaver
//! triples). It doesn't here: **each party knows its own input, so it can secret-share the square
//! itself.** The whole computation then stays linear — two secure sums (one over the values, one
//! over the squares), both opened, and the mean/variance arithmetic finishes in the clear on the
//! two public sums.
//!
//! One profiling subtlety worth imitating: the two summation phases run the *same* logic, but are
//! wrapped in two protocols with **distinct ids** (`SumValues`, `SumSquares`). Identical ids would
//! produce identical call paths, and the renderer would merge the towers into one (correct
//! totals, but the phase structure disappears). Distinct ids keep the two phases visible as
//! separate towers — the reason to profile in the first place.
//!
//! Inputs are kept below 2²⁰ so that the sum of squares stays far from the Mersenne-61 modulus:
//! the opened field sums then equal the plain-integer sums, which the harness cross-checks.

use std::fs;

use rand::RngExt;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::{simulation::channel::SimpleNetworkConfig, Network, PartyId},
    prelude::{simulate, Environment, Error, GeneralEnv, Protocol, ProtocolId},
    protocol::share::{deal::PassiveDealShr, open::PassiveOpenShr},
    ss::shamir::ShamirSS,
};

const N_PARTIES: usize = 5;
/// Inputs stay below this bound so Σx² cannot wrap around the Mersenne-61 modulus.
const INPUT_BOUND: u64 = 1 << 20;

/// Shamir secret sharing over the Mersenne-61 field (one limb).
type Share = ShamirSS<1, Mersenne61>;

// Innermost composition layer: every party, in turn, deals a Shamir sharing of `value`. After the
// loop each party holds one share of every party's contribution.
struct InputPhase {
    value: Mersenne61,
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
                PassiveDealShr::dealer(me, self.value, env.network().party_ids())
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

// The summation logic both phases share: distribute sharings of `value`, add the collected shares
// locally (free — Shamir sharing is linear), and open the sum. The callers wrap this in protocols
// with distinct ids so the two phases stay distinguishable in the flamegraph.
async fn share_sum_open<E: Environment>(
    env: &mut E,
    value: Mersenne61,
) -> Result<Mersenne61, Error> {
    let shares: Vec<Share> = InputPhase { value }.execute(env).await?;

    let sum_share = shares
        .into_iter()
        .reduce(|acc, share| acc + &share)
        .expect("there is at least one share to sum");

    PassiveOpenShr::new(sum_share).execute(env).await
}

// First tower: the sum of the parties' values.
struct SumValues {
    input: Mersenne61,
}

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SumValues {
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        share_sum_open(env, self.input).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SumValues")
    }
}

// Second tower: the sum of the parties' *squared* values. The square is computed locally on the
// party's own input before sharing — no secure multiplication needed.
struct SumSquares {
    input: Mersenne61,
}

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SumSquares {
    type Output = Mersenne61;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        share_sum_open(env, self.input * &self.input).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SumSquares")
    }
}

// Top level: run the two summation phases in sequence. The output is the pair of opened sums;
// mean and variance are derived from them in the clear.
struct SecStats {
    input: Mersenne61,
}

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SecStats {
    // (Σ values, Σ squares), learned by everyone.
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

fn main() {
    let parties: Vec<PartyId> = (0..N_PARTIES).map(PartyId::from).collect();

    // The harness draws the private inputs so it can cross-check the MPC result in the clear
    // below; inside the protocol, each party only ever shares its own value.
    let mut rng = rand::rng();
    let inputs: Vec<u64> = (0..N_PARTIES)
        .map(|_| rng.random_range(0..INPUT_BOUND))
        .collect();

    let outcome = simulate(
        SimpleNetworkConfig,
        parties.clone(),
        |pid| SecStats {
            input: Mersenne61::from(inputs[pid.as_usize()]),
        },
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // Cross-check: with inputs below 2^20 nothing wraps mod 2^61 - 1, so the opened field sums
    // must equal the plain-integer sums.
    let sum: u64 = inputs.iter().sum();
    let sum_sq: u64 = inputs.iter().map(|x| x * x).sum();
    for party in &parties {
        assert_eq!(
            outcome.outputs[party],
            (Mersenne61::from(sum), Mersenne61::from(sum_sq)),
            "every party must open the same two sums"
        );
    }

    let n = N_PARTIES as f64;
    let mean = sum as f64 / n;
    let variance = sum_sq as f64 / n - mean * mean;
    println!("every party learned: mean = {mean:.2}, variance = {variance:.2}\n");

    // Export every party's bandwidth call tree into one folded-stacks file; renderers sum
    // duplicate paths, so the graph shows network-wide totals per call path.
    let mut folded = Vec::new();
    for party in &parties {
        outcome
            .bandwidth_tree_for(*party)
            .expect("the party was part of the simulation")
            .write_folded(&mut folded)
            .expect("writing to a Vec cannot fail");
    }
    fs::write("stats_bandwidth.folded", &folded).expect("stats_bandwidth.folded must be writable");

    println!(
        "wrote stats_bandwidth.folded:\n{}",
        String::from_utf8_lossy(&folded)
    );
    println!("render it as an SVG flamegraph with inferno (https://github.com/jonhoo/inferno):");
    println!("    cargo install inferno");
    println!(
        "    inferno-flamegraph --countname bytes --title \"Secure stats: bandwidth by protocol\" \\"
    );
    println!("        < stats_bandwidth.folded > stats_bandwidth.svg");
}
