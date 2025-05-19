use serde::{Deserialize, Serialize};

use super::field::FiniteField;

pub mod secp256k1;

pub trait EllipticCurve<const LIMBS: usize>: Serialize + for<'a> Deserialize<'a> {
    type ScalarField: FiniteField<LIMBS>;
    type PrimeField: FiniteField<LIMBS>;

    fn gen() -> Self;
    fn add(&self, rhs: &Self) -> Self;
    fn sub(&self, rhs: &Self) -> Self;
    fn scalar_mul(&self, rhs: &Self::ScalarField) -> Self;
    fn eq(&self, other: &Self) -> bool;
    fn negate(&self) -> Self;
}
