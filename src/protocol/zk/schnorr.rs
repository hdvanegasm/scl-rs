//! Schnorr's interactive proof of knowledge of a discrete logarithm (C. P. Schnorr, *Efficient
//! Signature Generation by Smart Cards*, Journal of Cryptology 4(3), 1991).
//!
//! The prover knows a scalar `x` with `h = g^x`, where `g` is the curve generator
//! ([`EllipticCurve::gen`](crate::math::ec::EllipticCurve::gen)) and `h` is public. It convinces
//! the verifier of this without revealing `x`, in three moves (written multiplicatively, as in the
//! literature; on the curve `g^r` is the scalar multiplication `r·g` and `u · h^c` is the point
//! addition `u + c·h`):
//!
//! 1. **Commitment.** The prover samples a fresh nonce `r` and sends `u = g^r`.
//! 2. **Challenge.** The verifier samples a uniformly random scalar `c` and sends it.
//! 3. **Response.** The prover sends `z = r + c·x`, and the verifier accepts if and only if
//!    `g^z = u · h^c`.
//!
//! This is the standard Sigma protocol for discrete logarithms; see Boneh and Shoup, *A Graduate
//! Course in Applied Cryptography*, §19.1, for the proofs of the properties below.
//!
//! - **Completeness.** An honest prover always convinces an honest verifier, because
//!   `g^z = g^(r + c·x) = g^r · (g^x)^c = u · h^c`.
//! - **Special soundness.** Two accepting transcripts `(u, c, z)` and `(u, c', z')` with the same
//!   commitment and `c ≠ c'` yield the witness: `x = (z - z') / (c - c')`. This is why the nonce
//!   `r` must never be reused — two proofs under one `r` are exactly such a pair, and hand `x` to
//!   anyone who sees both.
//! - **Honest-verifier zero knowledge.** A verifier that samples `c` honestly learns nothing about
//!   `x`: transcripts can be simulated by picking `c` and `z` first and setting `u = g^z · h^(-c)`.
//!
//! # Security model
//!
//! The zero-knowledge guarantee holds against an **honest verifier** only — one that samples its
//! challenge uniformly and independently of `u`. The protocol is interactive; no Fiat–Shamir
//! transform is applied, so the proof is not transferable to third parties.
//!
//! # Running inside a larger network
//!
//! The prover and the verifier are named by their [`PartyId`](crate::net::PartyId)s, so the proof
//! can run between any two distinct parties of a network with more than two nodes — for example as
//! a sub-protocol of a larger MPC protocol. Only those two parties execute
//! [`SchnorrPok`](crate::protocol::zk::schnorr::SchnorrPok); every other party simply does not run
//! it, and no message is sent to or expected from them.

use crate::{
    abbreviate::Abbreviate,
    math::{ec::EllipticCurve, ring::Ring},
    net::{Network, Packet, PartyId},
    protocol::{Error, Protocol, ProtocolId, RandEnvironment},
};

/// One node's view of a Schnorr proof of knowledge of the discrete logarithm of `h`.
///
/// Both roles share this single type. Each node carries the ids of both parties, so that it knows
/// whom to talk to, and the prover's node additionally carries the witness `x`. That mirrors the
/// protocol itself, where the two parties differ only in what they know — the statement `h` and
/// the roles are public to both.
///
/// Build the prover's node with [`SchnorrPok::prover`] and the verifier's with
/// [`SchnorrPok::verifier`], passing the same `prover`, `verifier` and `h` to both. Which role a
/// node plays is decided by comparing the local party against these ids when the protocol runs, as
/// for [`PassiveDealShr`](crate::protocol::share::deal::PassiveDealShr); the witness has to be
/// present exactly on the prover.
pub struct SchnorrPok<const LIMBS: usize, C: EllipticCurve<LIMBS>> {
    /// The party that proves knowledge of `x`.
    prover: PartyId,
    /// The party that checks the proof and outputs the verdict.
    verifier: PartyId,
    /// The witness `x` with `h = g^x`, held by the prover only; `None` on the verifier.
    x: Option<C::ScalarField>,
    /// The public statement: the group element whose discrete logarithm is being proved known.
    h: C,
}

impl<const LIMBS: usize, C> SchnorrPok<LIMBS, C>
where
    C: EllipticCurve<LIMBS>,
{
    /// The prover's view: it holds the witness `x`, which must satisfy `h = g^x`, and proves
    /// knowledge of it to the party `verifier`. The local party must be `prover` when the protocol
    /// runs.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Input`](enum@Error) if `prover == verifier`.
    pub fn prover(
        prover: PartyId,
        verifier: PartyId,
        x: C::ScalarField,
        h: C,
    ) -> Result<Self, Error> {
        Self::new(prover, verifier, Some(x), h)
    }

    /// The verifier's view: it holds only the public statement `h`, and checks a proof of knowledge
    /// of its discrete logarithm given by the party `prover`. The local party must be `verifier`
    /// when the protocol runs.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Input`](enum@Error) if `prover == verifier`.
    pub fn verifier(prover: PartyId, verifier: PartyId, h: C) -> Result<Self, Error> {
        Self::new(prover, verifier, None, h)
    }

    /// Shared constructor: a party cannot prove something to itself, so the two roles must be
    /// held by distinct parties.
    fn new(
        prover: PartyId,
        verifier: PartyId,
        x: Option<C::ScalarField>,
        h: C,
    ) -> Result<Self, Error> {
        if prover == verifier {
            return Err(Error::Input);
        }
        Ok(Self {
            prover,
            verifier,
            x,
            h,
        })
    }
}

impl<const LIMBS: usize, C, E> Protocol<E> for SchnorrPok<LIMBS, C>
where
    C: EllipticCurve<LIMBS> + Send + Sync + Abbreviate,
    C::ScalarField: Send + Sync + Abbreviate,
    E: RandEnvironment,
{
    /// The verifier's decision: `Some(true)` if it accepted the proof and `Some(false)` if it
    /// rejected it. The prover always returns `None`. A rejected proof is an ordinary output, not
    /// an [`Error`](enum@Error); errors are reserved for network and decoding failures, and for
    /// [`Error::Input`](enum@Error) when the node does not match its role: the local party is
    /// neither the prover nor the verifier, or the witness is missing on the prover or present on
    /// the verifier.
    type Output = Option<bool>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();
        if me == self.prover {
            // Here, the node is the prover, and it must hold the witness.
            let x = self.x.ok_or(Error::Input)?;
            let verifier = self.verifier;

            // Move 1, the commitment. `r` is a one-time nonce that blinds the witness in the
            // response below; it must be freshly sampled and never reused, since two responses
            // under the same `r` reveal `x` outright (see the module docs on special soundness).
            let r = C::ScalarField::random(env.rng_mut());
            // We are computing `u = g^r`.
            let u = C::gen().scalar_mul(&r);

            // Send u to the verifier.
            let mut pkt = Packet::empty();
            pkt.write_labeled(&u)?;
            env.network_mut().send_to(verifier, &pkt).await?;

            // Move 2: receive the verifier's challenge c.
            let mut pkt_c = env.network_mut().recv_from(verifier).await?;
            let c: C::ScalarField = pkt_c.pop()?;

            // Move 3, the response: `z = r + c*x`. The nonce `r` is uniform and independent of `x`,
            // so `z` is uniform too and hides the witness.
            let z = r + &(c * &x);

            // Send z.
            let mut pkt_z = Packet::empty();
            pkt_z.write_labeled(&z)?;
            env.network_mut().send_to(verifier, &pkt_z).await?;

            // A prover learns nothing from a proof it gave, so it has no output.
            Ok(None)
        } else if me == self.verifier {
            // Here, the node is the verifier. A witness here means the node was built with the
            // wrong constructor; reject it rather than let the secret sit on the wrong party.
            if self.x.is_some() {
                return Err(Error::Input);
            }
            let prover = self.prover;

            // Move 1: receive the prover's commitment u.
            let mut recv_u_pkt = env.network_mut().recv_from(prover).await?;
            let u: C = recv_u_pkt.pop()?;

            // Move 2, the challenge. Sampling it only *after* `u` has arrived is what makes the
            // proof sound: a prover that could predict `c` could pass without knowing `x`.
            let c = C::ScalarField::random(env.rng_mut());
            let mut pkt_c = Packet::empty();
            pkt_c.write_labeled(&c)?;
            env.network_mut().send_to(prover, &pkt_c).await?;

            // Move 3: receive the response z from the prover.
            let mut pkt_z = env.network_mut().recv_from(prover).await?;
            let z: C::ScalarField = pkt_z.pop()?;

            // Perform the final check: left_side = g^z, right_side = u * h^c. These agree exactly
            // when z was built as `r + c*x` from the `r` behind `u`, i.e. when the prover knew `x`.
            let left_side = C::gen().scalar_mul(&z);
            let right_side = u.add(&self.h.scalar_mul(&c));
            Ok(Some(left_side == right_side))
        } else {
            // Only the prover and the verifier take part in the proof.
            Err(Error::Input)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SchnorrPok")
    }
}
