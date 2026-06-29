//! Contains traits to represent elliptic curves over finite fields.

use super::field::FiniteField;
use crate::abbreviate::Abbreviate;
use crate::math::ec::secp256k1::Secp256k1;
use serde::{Deserialize, Serialize};

/// Implementation of the elliptic curve secp256k1.
pub mod secp256k1;

/// Trait that defines an elliptic curve point using certain number of limbs for the scalar and
/// prime field.
pub trait EllipticCurve<const LIMBS: usize>:
    Serialize + for<'a> Deserialize<'a> + PartialEq + Copy + Clone
{
    /// Field of coefficients of the elliptic curve.
    type ScalarField: FiniteField<LIMBS>;

    /// Field in which the elliptic curve is defined. The points in the elliptic curve will be
    /// pairs in this field.
    type PrimeField: FiniteField<LIMBS>;

    /// The zero point in the elliptic courve group.
    const ZERO: Self;

    /// Returns the generator of the curve.
    fn gen() -> Self;

    /// Computes the group addition between two points in the elliptic curve.
    fn add(&self, rhs: &Self) -> Self;

    /// Computes the subtraction between two elements in the elliptic curve.
    fn sub(&self, rhs: &Self) -> Self;

    /// Computes the multiplication by an scalar between an element in the scalar field and an
    /// point in the elliptic curve.
    fn scalar_mul(&self, rhs: &Self::ScalarField) -> Self;

    /// Returns the additive inverse of the point in the elliptic curve.
    fn negate(&self) -> Self;

    /// Tells if the current point is on the curve.
    fn is_on_curve(&self) -> bool;
}

macro_rules! impl_ec_abbreviation {
    ($($t:ty),+ $(,)?) => {
        $(impl Abbreviate for $t {
            const ABBREVIATION: &'static str = "EC elem.";
        })+
    };
}

impl_ec_abbreviation!(Secp256k1);
