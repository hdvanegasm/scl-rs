//! Shared helpers for the integration test suite.
//!
//! Cargo treats `tests/common/mod.rs` as a plain module (not its own test binary), so each test
//! file can pull it in with `mod common;`. `dead_code` is allowed because not every test binary
//! uses every helper.
#![allow(dead_code)]

use std::fmt::Debug;

use proptest::prelude::*;
use rand::{rngs::StdRng, SeedableRng};
use scl_rs::{
    math::{ring::Ring, vector::Vector},
    net::PartyId,
    prelude::{EllipticCurve, FiniteField},
    ss::LinearShare,
};
use serde::{de::DeserializeOwned, Serialize};

/// A [`proptest`] strategy that samples a uniform ring/field element.
///
/// It generates a 32-byte seed, seeds a deterministic CSPRNG from it, and draws an element through
/// the type's own [`Ring::random`] (which performs the proper modular reduction). Generating from a
/// seed — rather than from a `u64` — covers the *whole* field, including the wide 256-bit elements
/// that a `u64`-based generator would never reach; proptest shrinks the seed, so failures stay
/// reproducible.
pub fn field_element<F: Ring>() -> impl Strategy<Value = F> {
    any::<[u8; 32]>().prop_map(|seed| F::random(&mut StdRng::from_seed(seed)))
}

/// A [`proptest`] strategy that samples the curve point `a · C::gen()` for a uniform scalar `a`.
///
/// The scalar is drawn as in [`field_element`], from a 32-byte seed that proptest can shrink.
pub fn curve_element<const LIMBS: usize, C, F>() -> impl Strategy<Value = C>
where
    C: EllipticCurve<LIMBS, ScalarField = F> + Debug,
    F: FiniteField<LIMBS>,
{
    any::<[u8; 32]>().prop_map(|seed| {
        let a = F::random(&mut StdRng::from_seed(seed));
        C::gen().scalar_mul(&a)
    })
}

/// Serializes `x` with `postcard`, deserializes it back and fails the test case unless the result
/// equals `x`.
///
/// Panics if serialization or deserialization itself fails.
pub fn roundtrip<T>(x: T) -> Result<(), TestCaseError>
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let bytes = postcard::to_allocvec(&x).unwrap();
    prop_assert_eq!(postcard::from_bytes::<T>(&bytes).unwrap(), x);
    Ok(())
}

/// Deals every entry of `secrets` and regroups the result by holder: the returned `i`-th vector is
/// the one party `parties[i]` holds, with one share per secret.
///
/// This is the layout a protocol actually works in — a party holds one share of each of several
/// secrets — so operating on the returned vectors exercises the local, communication-free
/// operations of [`LinearShare`].
pub fn deal_by_party<S: LinearShare>(
    secrets: &[S::Value],
    parties: &[PartyId],
    threshold: S::Threshold,
) -> Vec<Vector<S>> {
    let mut rng = rand::rng();
    let per_secret: Vec<Vec<S>> = secrets
        .iter()
        .map(|secret| S::shares_from_secret(*secret, parties, threshold, &mut rng).unwrap())
        .collect();
    (0..parties.len())
        .map(|i| {
            Vector::from(
                per_secret
                    .iter()
                    .map(|shares| shares[i].clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Reconstructs entry `k` from a per-party set of share vectors produced by [`deal_by_party`].
pub fn open_entry<S: LinearShare>(
    per_party: &[Vector<S>],
    k: usize,
    parties: &[PartyId],
) -> S::Value {
    let shares: Vec<S> = per_party.iter().map(|v| v[k].clone()).collect();
    S::secret_from_shares(&shares, parties).unwrap()
}
