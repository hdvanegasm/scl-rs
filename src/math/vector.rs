use super::ring::Ring;
use crypto_bigint::rand_core::RngCore;
use serde::Serialize;
use std::ops::{Add, Index, IndexMut, Mul, Sub};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("the operands do not have the same dimension: {0:?} and {1:?}")]
    IncompatibleDimension(usize, usize),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Vector whose elements belong to a ring.
#[derive(Serialize, PartialEq, Eq, Debug, Clone)]
pub struct Vector<T: Ring>(Vec<T>);

impl<T> Vector<T>
where
    T: Ring,
{
    /// Returns the length of the vector.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the vector has no elements.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Generates a vector of zeroes.
    pub fn zero(len: usize) -> Self {
        Self(vec![T::ZERO; len])
    }

    /// Generates a vector of ones.
    pub fn ones(len: usize) -> Self {
        Self(vec![T::ONE; len])
    }

    /// Generates a random vector with a given length.
    pub fn random<R: RngCore>(len: usize, rng: &mut R) -> Self {
        let mut elements = Vec::with_capacity(len);
        for _ in 0..len {
            elements.push(T::random(rng));
        }
        Self(elements)
    }

    /// Computes the dot product between two vectors.
    pub fn dot(&self, other: &Vector<T>) -> Result<T> {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut result = T::ZERO;
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            result = result + &(*self_elem * other_elem);
        }
        Ok(result)
    }
}

impl<T> Index<usize> for Vector<T>
where
    T: Ring,
{
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl<T> IndexMut<usize> for Vector<T>
where
    T: Ring,
{
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl<T> From<Vec<T>> for Vector<T>
where
    T: Ring,
{
    fn from(value: Vec<T>) -> Self {
        Self(value)
    }
}

impl<T> Add<&Vector<T>> for Vector<T>
where
    T: Ring,
{
    type Output = Result<Self>;

    fn add(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem + other_elem);
        }
        Ok(Self(output))
    }
}

impl<T> Add<&Vector<T>> for &Vector<T>
where
    T: Ring,
{
    type Output = Result<Vector<T>>;

    fn add(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem + other_elem);
        }
        Ok(Vector(output))
    }
}

impl<T> Sub<&Vector<T>> for Vector<T>
where
    T: Ring,
{
    type Output = Result<Self>;

    fn sub(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem - other_elem);
        }
        Ok(Self(output))
    }
}

impl<T> Sub<&Vector<T>> for &Vector<T>
where
    T: Ring,
{
    type Output = Result<Vector<T>>;

    fn sub(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem - other_elem);
        }
        Ok(Vector(output))
    }
}

impl<T> Mul<&Vector<T>> for Vector<T>
where
    T: Ring,
{
    type Output = Result<Self>;

    fn mul(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem * other_elem);
        }
        Ok(Self(output))
    }
}

impl<T> Mul<&Vector<T>> for &Vector<T>
where
    T: Ring,
{
    type Output = Result<Vector<T>>;

    fn mul(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem * other_elem);
        }
        Ok(Vector(output))
    }
}
