use serde::{Deserialize, Serialize};

use super::field::FiniteField;

pub mod secp256k1;

/// Trait that defines an elliptic curve point using certain number of limbs for the scalar and
/// prime field.
pub trait EllipticCurve<const LIMBS: usize>: Serialize + for<'a> Deserialize<'a> {
    type ScalarField: FiniteField<LIMBS>;

    /// Field in which the elliptic curve is defined. The points in the elliptic curve will be
    /// pairs in this field.
    type PrimeField: FiniteField<LIMBS>;

    /// Returns the generator of the curve.
    fn gen() -> Self;

    /// Computes the group addition between two points in the elliptic curve.
    fn add(&self, rhs: &Self) -> Self;

    /// Computes the subtraction between two elements in the elliptic curve.
    fn sub(&self, rhs: &Self) -> Self;

    /// Computes the multiplication by an scalar between an element in the scalar field and an
    /// point in the elliptic curve.
    fn scalar_mul(&self, rhs: &Self::ScalarField) -> Self;

    /// Returns whether two points in the elliptic curve are equal.
    fn eq(&self, other: &Self) -> bool;

    /// Returns the additive inverse of the point in the elliptic curve.
    fn negate(&self) -> Self;
}
