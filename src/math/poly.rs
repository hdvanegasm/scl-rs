use super::ring::Ring;
use crate::math::field::FiniteField;
use crypto_bigint::rand_core::RngCore;
use serde::Serialize;
use std::{
    collections::HashSet,
    ops::{Index, IndexMut},
};
use thiserror::Error;

/// Errors for all the polynomial operations.
#[derive(Debug, Error)]
pub enum Error<T>
where
    T: Ring,
{
    /// This error is triggered when there is an interpolation and the elements in the x-axis are
    /// not all different.
    #[error("error in the interpolation, not all the elements in the list are different: {0:?}")]
    NotAllDifferentInterpolation(Vec<T>),

    #[error("the polynomial has no coefficients")]
    EmptyCoefficients,
}

/// Specialized type for the [`Error`] type.
pub type Result<T, R> = std::result::Result<T, Error<R>>;

/// Represents a polynomial whose coefficients are elements in a finite field.
#[derive(PartialEq, Eq, Debug, Serialize)]
pub struct Polynomial<const LIMBS: usize, T: FiniteField<LIMBS>>(Vec<T>);

impl<const LIMBS: usize, T: FiniteField<LIMBS>> Polynomial<LIMBS, T> {
    /// Evaluates the polynomial on a given value using the Horner's rule.
    pub fn evaluate(&self, value: &T) -> T {
        let mut result = *self.0.last().unwrap();
        for coeff in self.0[0..self.0.len() - 1].iter().rev() {
            result = coeff.add(&result.mul(value));
        }
        result
    }

    pub fn coefficients(&self) -> &[T] {
        &self.0
    }

    pub fn degree(&self) -> usize {
        self.0.len() - 1
    }

    /// Generates a random polynomial of a given degree using a given pseudo-random generator.
    pub fn random<R: RngCore>(degree: usize, rng: &mut R) -> Self {
        let mut coefficients = Vec::with_capacity(degree + 1);
        for _ in 0..degree + 1 {
            coefficients.push(T::random(rng));
        }
        Self(coefficients)
    }

    pub fn set_constant_coeff(&mut self, value: T) {
        self[0] = value;
    }

    pub fn new(coef: Vec<T>) -> Result<Self, T> {
        if coef.len() == 0 {
            Err(Error::EmptyCoefficients)
        } else {
            Ok(Self(coef))
        }
    }
}

impl<const LIMBS: usize, T: FiniteField<LIMBS>> Index<usize> for Polynomial<LIMBS, T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl<const LIMBS: usize, T: FiniteField<LIMBS>> IndexMut<usize> for Polynomial<LIMBS, T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl<const LIMBS: usize, const N: usize, T: FiniteField<LIMBS>> From<[T; N]>
    for Polynomial<LIMBS, T>
{
    fn from(coefficients: [T; N]) -> Self {
        Self(Vec::from_iter(coefficients))
    }
}

/// Computes the lagrange basis evaluated at `x`
pub fn compute_lagrange_basis<const LIMBS: usize, T: FiniteField<LIMBS>>(
    nodes: &[T],
    x: &T,
) -> Result<Vec<T>, T> {
    if !all_different(nodes) {
        return Err(Error::NotAllDifferentInterpolation(nodes.to_vec()));
    }
    let mut lagrange_basis = Vec::with_capacity(nodes.len());
    for j in 0..nodes.len() {
        let mut basis = T::ONE;
        let x_j = &nodes[j];
        for (m, node) in nodes.iter().enumerate() {
            if m != j {
                let x_m = node;
                let numerator = x.sub(x_m);
                let denominator = x_j.sub(x_m);

                // The unwrap is safe because x_j - x_m is not zero.
                let term = numerator.mul(&denominator.inverse().unwrap());
                basis = basis.mul(&term);
            }
        }
        lagrange_basis.push(basis);
    }
    Ok(lagrange_basis)
}

/// Checks if all the elements of the list are different.
fn all_different<T: Ring>(list: &[T]) -> bool {
    if list.is_empty() {
        return true;
    }
    let mut set = HashSet::with_capacity(list.len());
    for element in list {
        if !set.insert(element) {
            return false;
        }
    }
    true
}

/// Computes the evaluation of the interpolated polynomial at `x`.
pub fn interpolate_polynomial_at<const LIMBS: usize, T: FiniteField<LIMBS>>(
    evaluations: &[T],
    alphas: &[T],
    x: &T,
) -> Result<T, T> {
    assert!(alphas.len() > 0);
    assert!(alphas.len() == evaluations.len());
    let lagrange_basis = compute_lagrange_basis(alphas, x)?;
    let mut interpolation = T::ZERO;
    for (eval, basis) in evaluations.iter().zip(lagrange_basis) {
        interpolation = interpolation.add(&eval.mul(&basis));
    }
    Ok(interpolation)
}
