use thiserror::Error;

pub mod mersenne61;

#[derive(Error, Debug)]
pub enum FieldError {
    #[error("trying to compute the inverse of zero")]
    ZeroInverse,
}

/// Trait that represent a finite field of integers modulo a prime p.
pub trait FiniteField: Sized {
    /// Type of the underlying representation for a field element.
    type ValueType;

    /// Modulus used in for the field.
    const MODULUS: u64;

    /// Bit size of the elements in the field.
    const BIT_SIZE: usize;

    /// Additive identity of the field.
    const ZERO: Self;

    /// Multiplicative identity of the field.
    const ONE: Self;

    /// Adds two elements in the field.
    fn add(&self, other: Self) -> Self;

    /// Multiplies to elements in the field.
    fn multiply(&self, other: Self) -> Self;

    /// Computes the inverse of field element.
    fn inverse(&self) -> Result<Self, FieldError>;

    /// Compares equality between two field elements.
    fn equal(&self, other: Self) -> bool;

    /// Computes the additive inverse of a field element.
    fn negate(&self) -> Self;

    /// Computes the subtraction between two field elements.
    fn subtract(&self, other: Self) -> Self;
}
