//! Passive-adversary MPC over Shamir sharing, following the protocols of Damgård and Nielsen,
//! *Scalable and Unconditionally Secure Multiparty Computation* (CRYPTO 2007) — "DN07".
//!
//! The protocols here assume a **semi-honest** adversary corrupting up to `t` of the `n` parties:
//! everyone follows the protocol, but the corrupted parties pool what they see. Except for
//! [`rand_share`], they require `n >= 2t + 1`, because a degree-`2t` sharing has to remain openable.
//!
//! DN07's contribution is that generating shared randomness costs `O(n)` work per party, not
//! `O(n²)`: every party deals one sharing, and the `n` dealt sharings are compressed into `n - t`
//! that are uniformly random even if `t` of the dealers cheated in choosing their inputs. The
//! compression is a Vandermonde extraction matrix, built once by this module's private
//! `extraction_matrix` helper. The pieces fit together as:
//!
//! - [`rand_share`] — `Random`: `n - t` degree-`t` sharings of secrets no one knows.
//! - [`double_rand_share`] — `Double-Random`: the same, but each secret is shared twice, at degree
//!   `t` and degree `2t`. This is what re-randomizes the product of two sharings.
//! - [`open_king`] — `Open`: every party sends its share to a designated *king*, who reconstructs
//!   and sends the result back. The batched form opens many values in a single round.
//! - [`triple`] — multiplication triples `([a], [b], [a · b])`, assembled from the above.
//! - [`mul`] — Beaver multiplication, which spends those triples to multiply live wire values. All
//!   the products at one depth of a circuit go in a single batch, so a circuit's round count tracks
//!   its multiplicative depth rather than its gate count.

/// DN07 `Double-Random`: batches of degree-`t` / degree-`2t` sharings of the same unknown secrets.
pub mod double_rand_share;
/// Beaver multiplication: spends triples to multiply sharings, a whole batch per round.
pub mod mul;
/// DN07 `Open`: reconstruction through a designated king, one value or a batch at a time.
pub mod open_king;
/// DN07 `Random`: batches of degree-`t` sharings of secrets that no party knows.
pub mod rand_share;
/// DN07 triple generation: multiplication triples `([a], [b], [a · b])`.
pub mod triple;

use crate::{
    math::{field::FiniteField, matrix::Matrix},
    net::PartyId,
    ss::{shamir::ShamirSS, LinearShare},
};

/// Builds the randomness-extraction matrix `M = Van^(n, outputs)ᵀ` used by the DN07 protocols.
///
/// The result has `outputs` rows and one column **per dealer**: entry `(k, i)` is `α_i^k`, where
/// `α_i` is the field point of the `i`-th party in `parties`. Applied to the vector of shares dealt
/// by the `n` parties, it maps the `n` dealt secrets — of which up to `t` are chosen by the
/// adversary — onto `outputs` secrets that are uniformly random from the adversary's view. That
/// holds because every choice of `outputs` columns yields a transposed Vandermonde matrix on
/// distinct nodes, which is invertible.
///
/// The **transpose** is essential and not a detail: `Matrix::vandermonde` builds the `n × outputs`
/// matrix, whose rows are indexed by parties. Feeding that to the extraction directly would be a
/// dimension error, and "fixing" it by truncating a square Vandermonde to `outputs` rows would be
/// silently wrong — a row-truncated Vandermonde over a finite field is not invertible in general.
///
/// `parties` must be non-empty and `outputs` non-zero; callers validate this before calling.
fn extraction_matrix<const LIMBS: usize, F>(parties: &[PartyId], outputs: usize) -> Matrix<F>
where
    F: FiniteField<LIMBS> + From<u64> + Send + Sync,
{
    let points: Vec<F> = parties
        .iter()
        .copied()
        .map(<ShamirSS<LIMBS, F> as LinearShare>::encode_party)
        .collect();
    Matrix::vandermonde(&points, outputs)
        .expect("callers guarantee a non-empty party list and a non-zero output count")
        .transpose()
}
