//! Running independent sub-protocols concurrently with [`Join`], and what it does to a party's
//! running time.
//!
//! A protocol that calls its sub-protocols with [`execute`](Protocol::execute) runs them one after
//! another, because `execute` borrows the environment until the sub-protocol returns. When the
//! sub-protocols are independent, a party then waits for the *sum* of their latencies. [`Join`]
//! runs them concurrently instead, each in a session of its own, so the party waits only for the
//! slowest of them: the critical path.
//!
//! This example runs the preprocessing of a Beaver multiplication twice on the simulator, once with
//! its steps one after another and once with the independent steps in one `Join`, and prints the
//! two runs side by side. The preprocessing is five sharing protocols from DN07, which do not
//! depend on each other, followed by triple generation, which needs three of them:
//!
//! 1. four runs of `Random` ([`PassiveRandShr`]): the factors `x` and `y`, and the `a` and `b` of
//!    the triples. Each takes one hop.
//! 2. one run of `Double-Random` ([`PassiveRandDoubleShr`]): the double sharings that mask `a · b`.
//!    It takes two hops.
//! 3. triple generation ([`PassiveTriple`]): the parties open the masked products to the king, and
//!    the king sends the result back. Two hops for every party but the king, which needs only the
//!    first.
//!
//! On the WAN preset, a hop takes 50 ms, so a party other than the king needs 4 × 50 + 100 + 100 =
//! 400 ms one after another, and max(50, 100) + 100 = 200 ms under the `Join`; the king needs 50 ms
//! less in both. The bytes do not change: the same messages are sent either way, only earlier. The
//! packets are small, so their serialization over 100 Mbps adds well under a millisecond.
//!
//! `Join` is not the only way to save rounds. When one protocol handles a vector of inputs in the
//! same rounds, one instance over all the inputs beats a `Join` of one instance per input, since it
//! sends one message per link instead of one per instance. Here the steps have different types and
//! round counts, so they cannot be batched into one protocol; that is the case `Join` is for.
//!
//! Run it with `cargo run --example concurrent_preprocessing`.

use std::time::Duration;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::{
        simulation::{
            channel::SimpleNetworkConfig,
            network::SimNetwork,
            simulator::{SimulationMetrics, SimulationOutcome},
        },
        PartyId,
    },
    prelude::{
        simulate, Error, ForkEnvironment, GeneralEnvironment, Join, Protocol, ProtocolId,
        RandEnvironment,
    },
    protocol::passive_shamir::{
        double_rand_share::PassiveRandDoubleShr,
        rand_share::PassiveRandShr,
        triple::{PassiveTriple, ShamirTriple},
    },
    ss::{shamir::ShamirSS, LinearShare},
};

/// The field of every sharing: integers modulo the prime `2^61 - 1`.
type F = Mersenne61;
/// A Shamir share over `F`, whose elements take one limb.
type Share = ShamirSS<1, F>;
/// What every party outputs: its shares of `x` and `y`, and of each triple's `(a, b, a · b)`.
type Output = (Vec<Share>, Vec<Share>, Vec<(Share, Share, Share)>);

/// The number of parties `n`.
const N: usize = 5;
/// The corruption threshold `t`. DN07 needs `n >= 2t + 1`.
const T: usize = 2;

/// The ids of the `N` parties.
fn parties() -> Vec<PartyId> {
    (0..N).map(PartyId::from).collect()
}

/// The king of triple generation: the party the masked products are opened to.
fn king() -> PartyId {
    PartyId::from(0)
}

/// The preprocessing of a Beaver multiplication: the random factors `x` and `y`, and the triples
/// to spend on `x · y`.
struct Preprocessing {
    /// Whether the five sharing protocols run in one `Join` or one after another.
    concurrent: bool,
}

impl<E: RandEnvironment + ForkEnvironment> Protocol<E> for Preprocessing {
    /// The factors `x` and `y`, and the triples, each a `Vec` with one entry per output of
    /// `Random`. A triple is given as its three sharings `([a], [b], [a · b])`.
    type Output = Output;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let random = || PassiveRandShr::<1, F>::new(T, parties());
        let double_random = || PassiveRandDoubleShr::<1, F>::new(T, parties());

        let (x, y, a, b, doubles) = if self.concurrent {
            // Each protocol runs in a child session of its own, and the party waits for the
            // slowest of them.
            Join((random()?, random()?, random()?, random()?, double_random()?))
                .execute(env)
                .await?
        } else {
            // Each `execute` returns before the next protocol starts, so the latencies add up.
            (
                random()?.execute(env).await?,
                random()?.execute(env).await?,
                random()?.execute(env).await?,
                random()?.execute(env).await?,
                double_random()?.execute(env).await?,
            )
        };

        // Triple generation needs `a`, `b` and `doubles`, so it waits for all three either way.
        let triples = PassiveTriple::new(king(), parties(), a, b, doubles)?
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

/// Runs the preprocessing on the WAN preset, every party with its own seeded RNG.
fn run(concurrent: bool) -> SimulationOutcome<Output> {
    simulate(
        SimpleNetworkConfig::wan(),
        parties(),
        move |_| Preprocessing { concurrent },
        |party, network: SimNetwork| {
            GeneralEnvironment::new(network, ChaCha20Rng::seed_from_u64(party.as_usize() as u64))
        },
        vec![],
    )
}

/// Checks that every triple the parties hold opens to some `(a, b, a · b)`.
///
/// Running the sharings under a `Join` forks the parties' RNGs, so the two runs sample different
/// values. What they must agree on is that the triples are correct, not what is in them.
fn check_triples(outcome: &SimulationOutcome<Output>) {
    let open = |part: fn(&(Share, Share, Share)) -> &Share, k: usize| -> F {
        let shares: Vec<Share> = parties()
            .iter()
            .map(|party| part(&outcome.outputs[party].2[k]).clone())
            .collect();
        <Share as LinearShare>::secret_from_shares(&shares, &parties())
            .expect("every party holds a share of every triple")
    };
    let n_triples = outcome.outputs[&king()].2.len();
    for k in 0..n_triples {
        let a = open(|triple| &triple.0, k);
        let b = open(|triple| &triple.1, k);
        let c = open(|triple| &triple.2, k);
        assert_eq!(c, a * &b, "triple {k} does not open to (a, b, a · b)");
    }
}

/// Renders a duration in milliseconds, to two decimals.
fn ms(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1000.0)
}

/// Prints the two runs' elapsed times side by side, one row per party.
fn print_comparison(sequential: &SimulationMetrics, concurrent: &SimulationMetrics) {
    println!("elapsed time per party:");
    println!(
        "    {:<16}{:>18}{:>14}{:>12}",
        "", "one after another", "under Join", "saved"
    );
    for party in parties() {
        let before = sequential.elapsed_time_for(party).expect("party took part");
        let after = concurrent.elapsed_time_for(party).expect("party took part");
        let label = if party == king() {
            format!("party {party} (king)")
        } else {
            format!("party {party}")
        };
        println!(
            "    {label:<16}{:>18}{:>14}{:>12}",
            ms(before),
            ms(after),
            ms(before.saturating_sub(after))
        );
    }
}

fn main() {
    let sequential = run(false);
    let concurrent = run(true);
    check_triples(&sequential);
    check_triples(&concurrent);

    let (sequential, concurrent) = (sequential.metrics(), concurrent.metrics());
    println!("one after another:\n{sequential}\n");
    println!("under Join:\n{concurrent}\n");
    print_comparison(&sequential, &concurrent);

    // The same messages are sent either way, only at different times.
    for party in parties() {
        assert_eq!(
            sequential.sent_bytes_for(party),
            concurrent.sent_bytes_for(party),
            "party {party} sent a different number of bytes"
        );
    }
    println!("\nevery party sent the same bytes in both runs; every triple opens correctly");
}
