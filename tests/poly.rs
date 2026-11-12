//! Integration tests for polynomial interpolation (`math::poly::interpolate_polynomial_at`) over
//! `Mersenne61`, including its error cases.

mod common;

use proptest::prelude::*;
use rand::{rngs::StdRng, SeedableRng};
use scl_rs::{
    math::{
        field::mersenne61::Mersenne61,
        poly::{interpolate_polynomial_at, Error, Polynomial},
    },
    prelude::Ring,
};

proptest! {
    /// Checks that interpolating a random polynomial of degree below 16 from its evaluations at
    /// `1..=degree + 1` gives its value at a random `x`, and its constant term at `0`.
    #[test]
    fn interpolation_recovers_polynomial(
        seed in any::<[u8; 32]>(),
        degree in 0usize..16,
        x in common::field_element::<Mersenne61>(),
    ) {
        let mut rng = StdRng::from_seed(seed);
        let poly = Polynomial::random(degree, &mut rng);
        let real_eval = poly.evaluate(&x);

        let alphas: Vec<_> = (1..=(degree as u64 + 1)).map(Mersenne61::from).collect();
        let evals: Vec<_> = alphas.iter().map(|alpha| poly.evaluate(alpha)).collect();
        let interpolation = interpolate_polynomial_at(&evals, &alphas, &x).unwrap();
        prop_assert_eq!(interpolation, real_eval);
        prop_assert_eq!(interpolate_polynomial_at(&evals, &alphas, &Mersenne61::ZERO).unwrap(), poly[0]);
    }
}

/// Checks that interpolating with no nodes returns `Error::EmptyInterpolation` instead of
/// panicking.
#[test]
fn interpolate_with_empty_input_is_rejected() {
    let empty: [Mersenne61; 0] = [];
    let x = Mersenne61::from(5);
    // No nodes to interpolate from: a structured error, not a panic.
    let result = interpolate_polynomial_at(&empty, &empty, &x);
    assert!(matches!(result, Err(Error::EmptyInterpolation)));
}

/// Checks that interpolating two evaluations over one node returns `Error::LengthMismatch` with
/// both counts, instead of panicking.
#[test]
fn interpolate_with_length_mismatch_is_rejected() {
    let evaluations = [Mersenne61::from(1), Mersenne61::from(2)];
    let alphas = [Mersenne61::from(3)];
    let x = Mersenne61::from(5);
    // Two evaluations but one node: a structured error, not a panic.
    let result = interpolate_polynomial_at(&evaluations, &alphas, &x);
    assert!(matches!(
        result,
        Err(Error::LengthMismatch {
            nodes: 1,
            evaluations: 2
        })
    ));
}
