use std::{
    collections::HashSet,
    ops::{Index, IndexMut},
};

use crate::math::field::FiniteField;
use rand::Rng;
use thiserror::Error;

use super::ring::Ring;

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
}

/// Specialized type for the [`Error`] type.
pub type Result<T, R> = std::result::Result<T, Error<R>>;

/// Represents a polynomial whose coefficients are elements in a finite field.
#[derive(PartialEq, Eq, Debug)]
pub struct Polynomial<T: FiniteField>(Vec<T>);

impl<T: FiniteField> Polynomial<T> {
    /// Evaluates the polynomial on a given value using the Horner's rule.
    pub fn evaluate(&self, value: &T) -> T {
        let mut result = self.0.last().unwrap().clone();
        for coeff in self.0[0..self.0.len() - 1].iter().rev() {
            result = coeff.add(&result.mul(value));
        }
        result
    }

    /// Generates a random polynomial of a given degree using a given pseudo-random generator.
    pub fn random<R: Rng>(degree: usize, rng: &mut R) -> Self {
        let mut coefficients = Vec::with_capacity(degree + 1);
        for _ in 0..degree + 1 {
            coefficients.push(T::random(rng));
        }
        Self(coefficients)
    }
}

impl<T: FiniteField> Index<usize> for Polynomial<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl<T: FiniteField> IndexMut<usize> for Polynomial<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl<const N: usize, T: FiniteField> From<[T; N]> for Polynomial<T> {
    fn from(coefficients: [T; N]) -> Self {
        Self(Vec::from_iter(coefficients))
    }
}

/// Computes the lagrange basis evaluated at `x`
pub(crate) fn compute_lagrange_basis<T: FiniteField>(nodes: Vec<T>, x: &T) -> Result<Vec<T>, T> {
    if !all_different(&nodes) {
        return Err(Error::NotAllDifferentInterpolation(nodes));
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
pub fn interpolate_polynomial_at<T: FiniteField>(
    evaluations: Vec<T>,
    alphas: Vec<T>,
    x: &T,
) -> Result<T, T> {
    assert!(alphas.len() == evaluations.len());
    let lagrange_basis = compute_lagrange_basis(alphas, x)?;
    let mut interpolation = T::ZERO;
    for (eval, basis) in evaluations.into_iter().zip(lagrange_basis) {
        interpolation = interpolation.add(&eval.mul(&basis));
    }
    Ok(interpolation)
}
