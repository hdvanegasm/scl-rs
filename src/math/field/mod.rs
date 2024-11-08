use std::{
    fmt::Debug,
    ops::{Div, Index, IndexMut},
};

use crate::math::ring;
use rand::Rng;
use thiserror::Error;

pub mod lagrange;
pub mod mersenne61;

#[derive(Error, Debug)]
pub enum FieldError {
    #[error("trying to compute the inverse of zero")]
    ZeroInverse,
}

/// Trait that represent a finite field of integers modulo a prime p.
pub trait FiniteField: ring::Ring + for<'a> Div<&'a Self> {
    /// Modulus used in for the field.
    const MODULUS: u64;

    /// Computes the inverse of field element.
    fn inverse(&self) -> Result<Self, FieldError>;
}

/// Represents a polynomial whose coefficients are elements in a finite field.
#[derive(PartialEq, Eq, Debug)]
pub struct Polynomial<T: FiniteField>(Vec<T>);

impl<T: FiniteField> Polynomial<T> {
    /// Evaluates the polynomial on a give value.
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
