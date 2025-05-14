use super::field::FiniteField;

mod secp256k1;

pub trait EllipticCurve {
    type ScalarField: FiniteField<4>;
    type PrimeField: FiniteField<4>;
}
