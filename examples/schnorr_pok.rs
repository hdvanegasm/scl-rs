//! **Schnorr's protocol**: an interactive zero-knowledge *proof of knowledge* of a discrete
//! logarithm, run between two parties over secp256k1.
//!
//! Every other example in this folder is a *multiparty computation*: several parties jointly
//! evaluate a function over inputs none of them may see. This one is a different primitive
//! altogether — a **two-party proof**. Nothing is computed. One party (the *prover*) already knows
//! a secret, and its only goal is to convince the other (the *verifier*) that it knows it, while
//! revealing nothing whatsoever about the secret itself. It is included here because it is the
//! canonical Σ-protocol: the three-move shape below is the skeleton of Schnorr signatures (via
//! Fiat–Shamir), of countless identification schemes, and of the consistency proofs that make
//! passively-secure MPC protocols actively secure.
//!
//! # The statement
//!
//! Fix the secp256k1 group with generator `g`. The prover holds a scalar `x` and both parties hold
//! the public point `h = g^x`. The prover wants to establish:
//!
//! > *"I know an `x` such that `h = g^x`"*
//!
//! — without revealing `x`. Recovering `x` from `h` unaided is the discrete-logarithm problem,
//! believed hard on this curve; the point of the protocol is that the prover can demonstrate
//! *knowledge* of that `x` anyway.
//!
//! Note that the maths below is written multiplicatively (`g^r`, `u · h^c`), as is conventional for
//! groups, while the code writes the same operations additively, as is conventional for elliptic
//! curves: `g^r` is `C::gen().scalar_mul(&r)` and `u · h^c` is `u.add(&h.scalar_mul(&c))`.
//!
//! # The three moves
//!
//! | # | Direction | Message | Computed as |
//! |---|-----------|---------|-------------|
//! | 1 | prover → verifier | `u`, the *commitment* | prover samples a fresh random `r`, sends `u = g^r` |
//! | 2 | verifier → prover | `c`, the *challenge*  | verifier samples `c` uniformly at random |
//! | 3 | prover → verifier | `z`, the *response*   | prover sends `z = r + c·x` (in the scalar field) |
//!
//! The verifier accepts if and only if
//!
//! ```text
//! g^z  ==  u · h^c
//! ```
//!
//! Three properties make that check meaningful, and they are worth separating because each is
//! doing a different job.
//!
//! **Completeness** — an honest prover always convinces an honest verifier. Substituting is
//! enough: `g^z = g^(r + c·x) = g^r · (g^x)^c = u · h^c`.
//!
//! **Special soundness** — a prover who does *not* know `x` cannot pass, except by guessing `c` in
//! advance. Suppose a prover could answer *two different* challenges `c ≠ c'` on the same
//! commitment `u`, with responses `z` and `z'`. Dividing the two verification equations cancels
//! `u`, leaving `g^(z − z') = h^(c − c')`, and therefore
//!
//! ```text
//! x = (z − z') / (c − c')
//! ```
//!
//! So anyone able to do that could *extract* the discrete logarithm outright. Since we assume that
//! is hard, a cheating prover can answer at most one challenge per commitment — it must gamble on
//! which one arrives, and over the ~2²⁵⁶ scalars of secp256k1 that gamble is hopeless. This is why
//! a single run suffices here, with no repetition.
//!
//! **Honest-verifier zero-knowledge** — the verifier learns nothing beyond the truth of the
//! statement. The argument is that an accepting transcript can be *forged* without any secret:
//! pick `z` and `c` at random and set `u = g^z · h^(−c)`. The resulting `(u, c, z)` satisfies the
//! check and is distributed exactly like a real transcript. A transcript that anyone could have
//! produced on their own cannot have taught the verifier anything — in particular it cannot have
//! leaked `x`. Note the qualifier *honest-verifier*: this argument covers a verifier that samples
//! `c` at random, as the one below does.
//!
//! # The challenge must be unpredictable
//!
//! Soundness rests entirely on the prover not knowing `c` when it commits to `u`. Look again at
//! the forgery in the paragraph above: it *is* the cheating strategy. A prover who learns `c` ahead
//! of time picks `z` at random, sets `u = g^z · h^(−c)`, and passes the check having never known
//! `x`. So the verifier must sample `c` independently, and only after `u` has arrived — the reason
//! the protocol needs three moves rather than two, and the reason each party must draw from its own
//! independent randomness rather than from a shared or replayed stream.
//!
//! # Reading the run
//!
//! `Protocol::Output` is `Option<bool>`, which encodes the asymmetry of the two roles: the verifier
//! outputs `Some(true)`/`Some(false)` — its verdict — while the prover outputs `None`, because a
//! prover learns nothing from a proof it gave. Both parties run the *same* `Protocol`
//! implementation; whether a node acts as prover or verifier is decided solely by whether it was
//! handed the witness `x` (see `SchnorrPok::prover` and `SchnorrPok::verifier`).
//!
//! The simulated traces printed by `main` show the cost of the three moves over
//! `SimpleNetworkConfig::wan()` (100 Mbps, 100 ms RTT):
//!
//! ```text
//! [     0.000s] SEND           0 -> 1 (99 bytes: 1 EC elem.)
//! [     0.100s] RECV           0 <- 1 (33 bytes: 1 field elem.)
//! [     0.100s] SEND           0 -> 1 (33 bytes: 1 field elem.)
//! ```
//!
//! Two observations. The whole proof costs 165 bytes and finishes in 0.150 s of virtual time —
//! three one-way flights, i.e. **1.5 round trips**, which on a wide-area link is what dominates;
//! the bytes are irrelevant at these sizes. And the commitment `u` costs 99 bytes against the
//! 33 of a scalar, because `Secp256k1` is serialised in projective coordinates, as three
//! base-field elements `(X, Y, Z)`.
//!
//! # What this example leaves out
//!
//! - **Fiat–Shamir.** Replacing the verifier's random `c` with `c = H(g, h, u)` makes the proof
//!   non-interactive and single-message — that transformation is exactly how one gets a Schnorr
//!   *signature*. It is not applied here.
//! - **Malicious verifiers.** The zero-knowledge argument above assumes the verifier samples `c`
//!   honestly. Schnorr is proved honest-verifier zero-knowledge; full zero-knowledge against a
//!   verifier that chooses `c` adversarially as a function of `u` needs additional machinery.
//! - **Constant-time execution.** As everywhere in this crate, the arithmetic is not hardened
//!   against timing side channels, and `z = r + c·x` is computed on secret data.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::{
        ec::{secp256k1::Secp256k1, EllipticCurve},
        field::secp256k1_scalar::Secp256k1ScalarField,
    },
    net::simulation::channel::SimpleNetworkConfig,
    prelude::*,
    protocol::{Protocol, RandEnvironment},
};

/// One node's view of a Schnorr proof of knowledge of the discrete logarithm of `h`.
///
/// Both roles share this single type, and `x` is what distinguishes them: holding the witness makes
/// a node the prover, and its absence makes it the verifier. That mirrors the protocol itself,
/// where the two parties differ only in what they know — the statement `h` is public to both.
struct SchnorrPok<const LIMBS: usize, C: EllipticCurve<LIMBS>> {
    /// The witness `x` with `h = g^x`, held by the prover only; `None` marks this node as the
    /// verifier.
    x: Option<C::ScalarField>,
    /// The public statement: the group element whose discrete logarithm is being proved known.
    h: C,
}

impl<const LIMBS: usize, C> SchnorrPok<LIMBS, C>
where
    C: EllipticCurve<LIMBS>,
{
    /// The prover's view: it holds the witness `x`, which must satisfy `h = g^x`.
    fn prover(x: C::ScalarField, h: C) -> Self {
        Self { x: Some(x), h }
    }

    /// The verifier's view: it holds only the public statement `h`.
    fn verifier(h: C) -> Self {
        Self { x: None, h }
    }
}

#[async_trait::async_trait]
impl<const LIMBS: usize, C, E> Protocol<E> for SchnorrPok<LIMBS, C>
where
    C: EllipticCurve<LIMBS> + Send + Sync + Abbreviate,
    C::ScalarField: Send + Sync + Abbreviate,
    E: RandEnvironment,
{
    type Output = Option<bool>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let other = env.network().other()?;
        if let Some(x) = self.x {
            // Here, the node is the prover: holding `x` is what selects this branch.

            // Move 1, the commitment. `r` is a one-time nonce that blinds the witness in the
            // response below; it must be freshly sampled and never reused, since two responses
            // under the same `r` reveal `x` outright (see the module docs on special soundness).
            let r = C::ScalarField::random(env.rng_mut());
            // We are computing `u = g^r`.
            let u = C::gen().scalar_mul(&r);

            // Send u to the verifier.
            let mut pkt = Packet::empty();
            pkt.write_labeled(&u)?;
            env.network_mut().send_to(other, &pkt).await?;

            // Move 2: receive the verifier's challenge c.
            let mut pkt_c = env.network_mut().recv_from(other).await?;
            let c: C::ScalarField = pkt_c.pop()?;

            // Move 3, the response: `z = r + c*x`. The nonce `r` is uniform and independent of `x`,
            // so `z` is uniform too and hides the witness.
            let z = r + &(c * &x);

            // Send z.
            let mut pkt_z = Packet::empty();
            pkt_z.write_labeled(&z)?;
            env.network_mut().send_to(other, &pkt_z).await?;

            // A prover learns nothing from a proof it gave, so it has no output.
            Ok(None)
        } else {
            // Here, the node is the verifier.

            // Move 1: receive the prover's commitment u.
            let mut recv_u_pkt = env.network_mut().recv_from(other).await?;
            let u: C = recv_u_pkt.pop()?;

            // Move 2, the challenge. Sampling it only *after* `u` has arrived is what makes the
            // proof sound: a prover that could predict `c` could pass without knowing `x`.
            let c = C::ScalarField::random(env.rng_mut());
            let mut pkt_c = Packet::empty();
            pkt_c.write_labeled(&c)?;
            env.network_mut().send_to(other, &pkt_c).await?;

            // Move 3: receive the response z from the prover.
            let mut pkt_z = env.network_mut().recv_from(other).await?;
            let z: C::ScalarField = pkt_z.pop()?;

            // Perform the final check: left_side = g^z, right_side = u * h^c. These agree exactly
            // when z was built as `r + c*x` from the `r` behind `u`, i.e. when the prover knew `x`.
            let left_side = C::gen().scalar_mul(&z);
            let right_side = u.add(&self.h.scalar_mul(&c));
            Ok(Some(left_side == right_side))
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SchnorrPok")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prover = PartyId::from(0);
    let verifier = PartyId::from(1);

    // The witness and the statement: a random secret `x` and the public point `h = g^x`. In a real
    // deployment `h` would be a long-lived public key and only the prover would ever see `x`; here
    // `main` sets both up so the simulated run has something to prove.
    let mut rng = ChaCha20Rng::from_rng(&mut rand::rng());
    let x = Secp256k1ScalarField::random(&mut rng);
    let h = Secp256k1::gen().scalar_mul(&x);

    // Both parties run the same protocol; only the prover is handed the witness.
    let proto_builder = |party| {
        if party == prover {
            SchnorrPok::prover(x, h)
        } else {
            SchnorrPok::verifier(h)
        }
    };

    let out = simulate(
        SimpleNetworkConfig::wan(),
        vec![prover, verifier],
        proto_builder,
        |_, net| GeneralEnv::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    println!(
        "==================== Prover trace: ====================\n{}",
        out.traces[&prover]
    );
    println!(
        "==================== Verifier trace: ====================\n{}",
        out.traces[&verifier]
    );

    println!("Prover's output: {:?}", out.outputs[&prover]);
    println!("Verifier's output: {:?}", out.outputs[&verifier]);

    Ok(())
}
