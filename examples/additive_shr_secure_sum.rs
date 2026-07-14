//! This example implements *secure summation*, the "hello world" of secure multiparty computation
//! (MPC): every party holds a private input, and together they compute the sum of all inputs without
//! any party revealing its own value to the others. At the end, everybody learns the sum — and
//! nothing else.
//!
//! The protocol is built on **additive secret sharing**. A secret `x` is split into `n` random
//! shares that add up to `x`; any `n - 1` of them reveal nothing about `x`, but all `n` together
//! reconstruct it. Crucially, addition of shares is *local and free*: if every party adds up the
//! shares it holds, the results are themselves a valid sharing of the sum of the original secrets.
//! That is why secure summation needs no multiplication protocol — it is purely linear.
//!
//! The computation is composed of two smaller protocols, which `SecureAddition` calls in sequence:
//!
//! 1. `DistrAdditiveShr` — each party splits its own input into shares and sends one share to every
//!    party (including itself), then collects the share that every party sent to it. Afterwards each
//!    party holds exactly one share of *every* input.
//! 2. `ReconstrAdditiveShr` — each party locally adds the shares it holds into a single share of the
//!    sum, reveals that summed share to everyone, and reconstructs the total from the revealed
//!    shares.
//!
//! Like every protocol in scl-rs, these are written generic over `E: Environment` (and therefore
//! over any `Network` the environment wraps), so the very same code runs on the deterministic
//! simulator used in `main` and over a real TLS deployment, unchanged.
//!
//! The two sub-protocols are written by hand here to show how protocols are built and composed.
//! scl-rs also ships generic, scheme-agnostic versions of these building blocks in
//! `scl_rs::protocol::share` — `PassiveDealShr` (a single dealer distributes shares of its
//! secret) and `PassiveOpenShr` (the parties reveal their shares and reconstruct, as
//! `ReconstrAdditiveShr` does below) — which work over any `LinearShare` scheme, not just
//! additive sharing.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::field::secp256k1_scalar::Secp256k1ScalarField,
    net::{simulation::channel::SimpleNetworkConfig, Network, Packet, PartyId},
    prelude::{simulate, Environment, Error, GeneralEnv, Protocol, Ring},
    protocol::ProtocolId,
    ss::additive::AdditiveSS,
};

// First sub-protocol: distribute an additive sharing of this party's secret and gather, in return,
// one share of every other party's secret. Its output is the vector of shares this party ends up
// holding — one per party in the computation.
struct DistrAdditiveShr<F> {
    secret: F,
}

#[async_trait::async_trait]
impl<E, R> Protocol<E> for DistrAdditiveShr<R>
where
    E: Environment,
    R: Ring + Sync + Send,
{
    // The output is the collection of shares this party holds after the exchange: one share of
    // every party's input.
    type Output = Vec<AdditiveSS<R>>;

    fn id(&self) -> ProtocolId {
        ProtocolId::from("DistAdditiveShr")
    }

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let n_parties = env.network().party_ids().len();

        // Split this party's secret into `n_parties` random additive shares. Because the shares are
        // secret material, they must come from a cryptographically secure RNG. `ChaCha20Rng` is a
        // `Send` CSPRNG, so it can be held across the `.await`s below (a `ThreadRng` cannot, as it is
        // `!Send`). Seed it from `rand::rng()`, which is itself a CSPRNG seeded from OS entropy.
        let mut rng = ChaCha20Rng::from_rng(&mut rand::rng());
        let shares =
            AdditiveSS::shares_from_secret(self.secret, &env.network().party_ids(), &mut rng);

        // Hand share `i` to party `i`. We send to *every* party, including ourselves (a party
        // sending to itself is just an in-process delivery, which keeps this uniform). Building the
        // scatter as one `send_many` lets a real network dispatch the sends concurrently — one socket
        // per peer — instead of serializing them; on the simulator the two are equivalent.
        let mut messages = Vec::with_capacity(n_parties);
        for (party_id, share) in env.network().party_ids().iter().zip(shares) {
            let mut packet = Packet::empty();
            packet.write_labeled(&share)?;
            messages.push((*party_id, packet));
        }
        env.network_mut().send_many(&messages).await?;

        // Symmetrically, receive the one share that each party sent to us. After this loop we hold
        // exactly one share of every party's secret.
        let mut shares_others = Vec::with_capacity(n_parties);
        for party_id in env.network().party_ids() {
            let mut share_packet = env.network_mut().recv_from(party_id).await?;
            let share = share_packet.pop()?;
            shares_others.push(share);
        }

        Ok(shares_others)
    }
}

// Second sub-protocol: reveal a sharing and reconstruct the underlying value. Each party contributes
// its own share (here, the share of the *sum* computed by `SecureAddition`) and learns the
// reconstructed secret.
struct ReconstrAdditiveShr<R> {
    share: AdditiveSS<R>,
}

#[async_trait::async_trait]
impl<E, R> Protocol<E> for ReconstrAdditiveShr<R>
where
    E: Environment,
    R: Ring + Send + Sync,
{
    // The output is the reconstructed secret, a single ring element.
    type Output = R;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let n_parties = env.network().party_ids().len();

        // Reveal our own share by sending it to every party (ourselves included, as before) — the
        // same packet scattered to all of them in one call.
        let mut packet = Packet::empty();
        packet.write_labeled(&self.share)?;
        let messages: Vec<(PartyId, Packet)> = env
            .network()
            .party_ids()
            .into_iter()
            .map(|party| (party, packet.clone()))
            .collect();
        env.network_mut().send_many(&messages).await?;

        // Collect everyone's revealed share. Once we have all `n` shares of the same secret, their
        // sum is the secret itself.
        let mut shares: Vec<AdditiveSS<R>> = Vec::with_capacity(n_parties);
        for party in env.network().party_ids() {
            let mut share_packet = env.network_mut().recv_from(party).await?;
            let share = share_packet.pop()?;
            shares.push(share);
        }

        let secret = AdditiveSS::secret_from_shares(&shares);
        Ok(secret)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("ReconstrAdditiveShr")
    }
}

// Top-level protocol: secure summation. It composes the two sub-protocols above by calling them
// inline and passing their *typed* outputs directly from one to the next — no manual serialization.
struct SecureAddition<R> {
    input: R,
}

#[async_trait::async_trait]
impl<E, R> Protocol<E> for SecureAddition<R>
where
    R: Ring + Send + Sync,
    E: Environment,
{
    // The output is the sum of every party's input.
    type Output = R;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        // Step 1: distribute a sharing of our input and gather a share of every party's input.
        let shares: Vec<AdditiveSS<R>> =
            DistrAdditiveShr { secret: self.input }.execute(env).await?;

        // Step 2: locally add the shares we hold. Because additive sharing is linear, this sum is a
        // valid share of the sum of all the inputs — computed with no communication at all.
        let share_sum = shares
            .into_iter()
            .reduce(|acc, elem| acc + &elem)
            .expect("there is at least one share to sum");

        // Step 3: reconstruct the sum from everyone's summed share.
        let result = ReconstrAdditiveShr { share: share_sum }
            .execute(env)
            .await?;
        Ok(result)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SecSumAdditiveShr")
    }
}

fn main() {
    // Run the secure summation among three parties.
    let p0 = PartyId::from(0);
    let p1 = PartyId::from(1);
    let p2 = PartyId::from(2);

    let parties = vec![p0, p1, p2];

    // `simulate` drives every party on the deterministic simulator. The two closures it takes are
    // per-party factories: the first builds each party's protocol instance — here giving every party
    // a fresh random `Secp256k1ScalarField` element as its private input — and the second builds the
    // `Environment` that the protocol runs in. Because the parties' inputs are random, the printed
    // sum changes from run to run, but all three parties always agree on it.
    let outcome = simulate(
        SimpleNetworkConfig,
        parties,
        |_| {
            let mut rng = rand::rng();
            SecureAddition {
                input: Secp256k1ScalarField::random(&mut rng),
            }
        },
        |_, net| GeneralEnv::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    // Each party's event trace and its typed output (the sum) are available on the outcome. All
    // three outputs are equal, which is exactly the correctness property of secure summation.
    println!(
        "==================== P0 trace: ====================\n{}",
        outcome.traces[&p0]
    );
    println!(
        "==================== P1 trace: ====================\n{}",
        outcome.traces[&p1]
    );
    println!(
        "==================== P2 trace: ====================\n{}",
        outcome.traces[&p2]
    );
    println!("P0 output: {:?}", outcome.outputs[&p0]);
    println!("P1 output: {:?}", outcome.outputs[&p1]);
    println!("P2 output: {:?}", outcome.outputs[&p2]);
}
