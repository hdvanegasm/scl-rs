use crate::{abbreviate::Abbreviate, ss::LinearShare};

use super::ring::Ring;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::ops::{Add, Index, IndexMut, Mul, Neg, Sub};

/// Errors that may occur during vector manipulation.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Incompatible dimension during matrix operations.
    #[error("the operands do not have the same dimension: {0:?} and {1:?}")]
    IncompatibleDimension(usize, usize),
}

/// Specialized result type for [`enum@Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Vector over any element type that supports the arithmetic performed on it.
///
/// The operators are bounded on the element operations themselves rather than on [`Ring`], so the
/// entries may be ring elements *or* secret shares. Over shares (`T: LinearShare`), `+`, `-` and
/// unary `-` are the local, communication-free operations of a linear scheme, and
/// [`mul_constant`](Vector::mul_constant) scales every entry by a public constant.
///
/// Element-wise multiplication requires `T: Mul<&T>` — share × share — which is not a local
/// operation and is deliberately absent from [`LinearShare`], so it remains available only over
/// rings. To add a vector of *public* constants to a vector of shares, use
/// [`add_shares`](Vector::add_shares).
///
/// # Examples
///
/// Each party scales the share vector it holds by a public constant, locally, and the result
/// reconstructs to the scaled secret:
///
/// ```
/// use scl_rs::{
///     math::{field::mersenne61::Mersenne61, vector::Vector},
///     net::PartyId,
///     ss::{additive::AdditiveSS, LinearShare},
/// };
///
/// let parties: Vec<PartyId> = (0..3usize).map(PartyId::from).collect();
/// let shares = <AdditiveSS<Mersenne61> as LinearShare>::shares_from_secret(
///     Mersenne61::from(7u64),
///     &parties,
///     (),
///     &mut rand::rng(),
/// )
/// .unwrap();
///
/// // Party `i` holds a one-entry vector; each scales it without any communication.
/// let scaled: Vec<Vector<AdditiveSS<Mersenne61>>> = shares
///     .iter()
///     .map(|share| Vector::from(vec![share.clone()]).mul_constant(&Mersenne61::from(2u64)))
///     .collect();
///
/// let entries: Vec<AdditiveSS<Mersenne61>> = scaled.iter().map(|v| v[0].clone()).collect();
/// let opened =
///     <AdditiveSS<Mersenne61> as LinearShare>::secret_from_shares(&entries, &parties).unwrap();
/// assert_eq!(opened, Mersenne61::from(14u64));
/// ```
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct Vector<T>(Vec<T>);

impl<T> Vector<T> {
    /// Returns the length of the vector.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the vector has no elements.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T> IntoIterator for Vector<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Vector<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T> Vector<T>
where
    T: Ring,
{
    /// Generates a vector of zeroes.
    pub fn zero(len: usize) -> Self {
        Self(vec![T::ZERO; len])
    }

    /// Generates a vector of ones.
    pub fn ones(len: usize) -> Self {
        Self(vec![T::ONE; len])
    }

    /// Generates a random vector with a given length.
    pub fn random<R: Rng>(len: usize, rng: &mut R) -> Self {
        let mut elements = Vec::with_capacity(len);
        for _ in 0..len {
            elements.push(T::random(rng));
        }
        Self(elements)
    }

    /// Adds this vector of public constants to a vector of shares element-wise: the `i`-th output
    /// share is `[x_i] + c_i`, where `c_i` is this vector's `i`-th element — a local,
    /// communication-free operation (see [`LinearShare`]).
    ///
    /// # Errors
    ///
    /// If the number of constants does not match the number of shares, this function will return an
    /// [`Error::IncompatibleDimension`] error.
    pub fn add_shares<S>(&self, shares: Vector<S>) -> Result<Vector<S>>
    where
        S: LinearShare<Value = T>,
    {
        if self.len() != shares.len() {
            return Err(Error::IncompatibleDimension(self.len(), shares.len()));
        }
        let mut output = Vec::with_capacity(shares.len());
        for (share, constant) in shares.into_iter().zip(&self.0) {
            output.push(share + constant);
        }
        Ok(Vector::from(output))
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

impl<S> Vector<S>
where
    S: LinearShare,
{
    /// Multiplies every share in this vector by the public constant `constant`: the `i`-th output
    /// share is `[c · x_i]` — a local, communication-free operation (see [`LinearShare`]).
    ///
    /// This is scalar multiplication for a vector of shares. It is an inherent method rather than a
    /// `Mul` operator because the scalar lives in [`S::Value`](LinearShare::Value) while the entries
    /// are shares: an `impl Mul<&S::Value> for Vector<S>` overlaps the element-wise
    /// [`Mul<&Vector<S>>`] impl, since nothing stops `S::Value` from being `Vector<S>`, and the
    /// coherence checker rejects it.
    pub fn mul_constant(self, constant: &S::Value) -> Self {
        Self(self.0.into_iter().map(|share| share * constant).collect())
    }
}

impl<T> Abbreviate for Vector<T> {
    const ABBREVIATION: &'static str = "vec";
}

impl<T> Index<usize> for Vector<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl<T> IndexMut<usize> for Vector<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl<T> From<Vec<T>> for Vector<T> {
    fn from(value: Vec<T>) -> Self {
        Self(value)
    }
}

impl<T> Neg for &Vector<T>
where
    T: Neg<Output = T> + Clone,
{
    type Output = Vector<T>;

    fn neg(self) -> Self::Output {
        let mut output = Vec::new();
        for elem in self {
            output.push(elem.clone().neg());
        }
        Vector::from(output)
    }
}

impl<T> Neg for Vector<T>
where
    T: Neg<Output = T> + Clone,
{
    type Output = Vector<T>;

    fn neg(self) -> Self::Output {
        (&self).neg()
    }
}

impl<T> Add<&Vector<T>> for Vector<T>
where
    T: for<'a> Add<&'a T, Output = T>,
{
    type Output = Result<Self>;

    fn add(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.into_iter().zip(other.0.iter()) {
            output.push(self_elem + other_elem);
        }
        Ok(Self(output))
    }
}

impl<T> Add<&Vector<T>> for &Vector<T>
where
    T: for<'a> Add<&'a T, Output = T> + Clone,
{
    type Output = Result<Vector<T>>;

    fn add(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(self_elem.clone() + other_elem);
        }
        Ok(Vector(output))
    }
}

impl<T> Sub<&Vector<T>> for Vector<T>
where
    T: for<'a> Sub<&'a T, Output = T>,
{
    type Output = Result<Self>;

    fn sub(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.into_iter().zip(other.0.iter()) {
            output.push(self_elem - other_elem);
        }
        Ok(Self(output))
    }
}

impl<T> Sub<&Vector<T>> for &Vector<T>
where
    T: for<'a> Sub<&'a T, Output = T> + Clone,
{
    type Output = Result<Vector<T>>;

    fn sub(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(self_elem.clone() - other_elem);
        }
        Ok(Vector(output))
    }
}

impl<T> Mul<&Vector<T>> for Vector<T>
where
    T: for<'a> Mul<&'a T, Output = T>,
{
    type Output = Result<Self>;

    fn mul(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.into_iter().zip(other.0.iter()) {
            output.push(self_elem * other_elem);
        }
        Ok(Self(output))
    }
}

impl<T> Mul<&Vector<T>> for &Vector<T>
where
    T: for<'a> Mul<&'a T, Output = T> + Clone,
{
    type Output = Result<Vector<T>>;

    fn mul(self, other: &Vector<T>) -> Self::Output {
        if self.len() != other.len() {
            return Err(Error::IncompatibleDimension(self.len(), other.len()));
        }
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(self_elem.clone() * other_elem);
        }
        Ok(Vector(output))
    }
}
impl<T> Mul<&T> for Vector<T>
where
    T: for<'a> Mul<&'a T, Output = T>,
{
    type Output = Self;

    /// Scales every entry by `rhs`.
    fn mul(self, rhs: &T) -> Self::Output {
        Self(self.0.into_iter().map(|elem| elem * rhs).collect())
    }
}

impl<T> Mul<&T> for &Vector<T>
where
    T: for<'a> Mul<&'a T, Output = T> + Clone,
{
    type Output = Vector<T>;

    /// Scales every entry by `rhs`.
    fn mul(self, rhs: &T) -> Self::Output {
        Vector(self.0.iter().map(|elem| elem.clone() * rhs).collect())
    }
}
