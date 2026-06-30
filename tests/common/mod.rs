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
    math::ring::Ring,
    prelude::{EllipticCurve, FiniteField},
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

pub fn roundtrip<T>(x: T) -> Result<(), TestCaseError>
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let bytes = postcard::to_allocvec(&x).unwrap();
    prop_assert_eq!(postcard::from_bytes::<T>(&bytes).unwrap(), x);
    Ok(())
}
