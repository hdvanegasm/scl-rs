use crate::ss::LinearShare;

use super::{ring::Ring, vector::Vector};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};
use thiserror::Error;

/// Errors that may occurs when creating and operating with matrices.
#[non_exhaustive]
#[derive(Error, Debug)]
pub enum Error {
    /// The matrices that you are trying to operate are not compatible.
    #[error("matrices are not compatible")]
    NotCompatible,

    /// The matrix that is being created does not have the correct dimension.
    #[error("the matrix has an invalid dimension: {0:?}, {1:?}")]
    InvalidDimension(usize, usize),

    /// The requested position lies outside the matrix.
    #[error("index out of bounds: ({i}, {j})")]
    IndexOutOfBounds {
        /// The row that was addressed.
        i: usize,
        /// The column that was addressed.
        j: usize,
    },
}

/// Specialized result for the [`enum@Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

/// Matrix with elements in a ring.
#[derive(Serialize, Deserialize, Debug, Eq, Clone, PartialEq)]
pub struct Matrix<T> {
    /// Elements of the matrix.
    elements: Vec<T>,
    /// Columns of the matrix.
    pub columns: usize,
    /// Rows of the matrix.
    pub rows: usize,
}

impl<T> Matrix<T> {
    /// Returns whether the matrix is square or not.
    pub fn is_square(&self) -> bool {
        self.rows == self.columns
    }

    /// Get the field element in i-th row and j-th column.
    pub fn get(&self, i: usize, j: usize) -> Option<&T> {
        if i >= self.rows || j >= self.columns {
            return None;
        }
        self.elements.get(self.columns * i + j)
    }

    /// Writes `value` into the element in the `i`-th row and `j`-th column.
    ///
    /// # Errors
    ///
    /// If the position lies outside the matrix, this function will return an
    /// [`Error::IndexOutOfBounds`] error.
    pub fn set(&mut self, i: usize, j: usize, value: T) -> Result<()> {
        if i >= self.rows || j >= self.columns {
            return Err(Error::IndexOutOfBounds { i, j });
        }
        *self
            .elements
            .get_mut(self.columns * i + j)
            .ok_or(Error::IndexOutOfBounds { i, j })? = value;
        Ok(())
    }

    /// Get the field element in i-th row and j-th column as a mutable reference.
    pub fn get_mut(&mut self, i: usize, j: usize) -> Option<&mut T> {
        if i >= self.rows || j >= self.columns {
            return None;
        }
        self.elements.get_mut(self.columns * i + j)
    }

    /// Return whether this matrix is compatible with another matrix for
    /// multiplication.
    fn is_compatible_with(&self, other: &Self) -> bool {
        self.rows == other.rows && self.columns == other.columns
    }

    /// Returns a new matrix with memory allocated for the given rows and columns.
    ///
    /// # Errors
    ///
    /// If the rows or the columns are zero, the function will return an [`Error::InvalidDimension`]
    /// error.
    fn allocate(rows: usize, columns: usize) -> Result<Self> {
        if rows == 0 || columns == 0 {
            return Err(Error::InvalidDimension(rows, columns));
        }
        let elements = Vec::with_capacity(rows * columns);
        Ok(Self {
            elements,
            rows,
            columns,
        })
    }

    /// Returns the transpose of this matrix: the `(i, j)` entry of the result is the `(j, i)` entry
    /// of this matrix, so an `r × c` matrix becomes a `c × r` one.
    pub fn transpose(&self) -> Self
    where
        T: Clone,
    {
        let mut elements = Vec::with_capacity(self.elements.len());
        for j in 0..self.columns {
            for i in 0..self.rows {
                elements.push(self.elements[self.columns * i + j].clone());
            }
        }
        Self {
            elements,
            rows: self.columns,
            columns: self.rows,
        }
    }

    /// Constructs a new matrix with the given rows, columns and list of elements.
    ///
    /// # Errors
    ///
    /// If the rows or the columns are zero, the function will return an [`Error::InvalidDimension`]
    /// error.
    pub fn from_vec(rows: usize, columns: usize, elements: Vec<T>) -> Result<Self> {
        if rows * columns != elements.len() || rows == 0 || columns == 0 {
            return Err(Error::InvalidDimension(rows, columns));
        }
        Ok(Self {
            elements,
            columns,
            rows,
        })
    }
}

impl<T> Matrix<T>
where
    T: Ring,
{
    /// Computes the scalar multiplication in place.
    pub fn scalar_mut_in_place(&mut self, scalar: &T) {
        for elem in &mut self.elements {
            *elem = elem.mul(scalar);
        }
    }

    /// Computes the scalar multiplication.
    pub fn scalar_mult(&mut self, scalar: &T) -> Self {
        let mut elements = Vec::with_capacity(self.elements.len());
        for elem in &self.elements {
            elements.push(elem.mul(scalar));
        }
        Self {
            elements,
            rows: self.rows,
            columns: self.columns,
        }
    }

    /// Constructs an identity matrix with the given dimension.
    ///
    /// Errors
    ///
    /// If the dimension is zero, the function will return an [`Error::InvalidDimension`]
    /// error.
    pub fn identity(dim: usize) -> Result<Self> {
        let mut matrix = Self::allocate(dim, dim)?;
        for i in 0..dim {
            for j in 0..dim {
                if i == j {
                    matrix.elements.push(T::ONE);
                } else {
                    matrix.elements.push(T::ZERO);
                }
            }
        }
        Ok(matrix)
    }

    /// Creates a matrix filled with zeros with the given rows and colums.
    ///
    /// # Errors
    ///
    /// If the rows or colums are zero, the function will return an [`Error::InvalidDimension`]
    /// error.
    pub fn zero(rows: usize, columns: usize) -> Result<Self> {
        if rows == 0 || columns == 0 {
            return Err(Error::InvalidDimension(rows, columns));
        }
        let elements = vec![T::ZERO; rows * columns];
        Ok(Self {
            elements,
            rows,
            columns,
        })
    }

    /// Creates a matrix filled with ones with the given rows and columns.
    ///
    /// # Errors
    ///
    /// If the rows or columns are zero, the function will return an [`Error::InvalidDimension`]
    /// error.
    pub fn ones(rows: usize, columns: usize) -> Result<Self> {
        if rows == 0 || columns == 0 {
            return Err(Error::InvalidDimension(rows, columns));
        }
        let elements = vec![T::ONE; rows * columns];
        Ok(Self {
            elements,
            rows,
            columns,
        })
    }

    /// Creates a random matrix with the given dimensions.
    ///
    /// # Errors
    ///
    /// If one or more of the dimensions are zero, the function will return an
    /// error.
    pub fn random<R: Rng>(rows: usize, columns: usize, rng: &mut R) -> Result<Self> {
        let mut matrix = Self::allocate(rows, columns)?;
        for _ in 0..rows * columns {
            matrix.elements.push(T::random(rng));
        }
        Ok(matrix)
    }

    /// Builds the Vandermonde matrix on `values` with `c` columns: the element in row `i` and
    /// column `j` is `values[i]^j`, so each row is the geometric progression of one value and the
    /// first column is all ones.
    ///
    /// The result has one row **per value**, which is the orientation to keep in mind: applying a
    /// Vandermonde to a vector indexed by those same values (randomness extraction, for instance)
    /// wants the [`transpose`](Matrix::transpose) instead.
    ///
    /// # Errors
    ///
    /// If `values` is empty or `c` is zero, the function will return an [`Error::InvalidDimension`]
    /// error.
    pub fn vandermonde(values: &[T], c: usize) -> Result<Self> {
        let mut matrix = Self::ones(values.len(), c)?;
        for (i, value) in values.iter().enumerate() {
            for j in 1..c {
                matrix.set(
                    i,
                    j,
                    *matrix
                        .get(i, j - 1)
                        .ok_or(Error::IndexOutOfBounds { i, j: j - 1 })?
                        * value,
                )?;
            }
        }
        Ok(matrix)
    }

    /// Applies this matrix of public constants to a slice of shares, row by row: the `i`-th
    /// output share is the linear combination `m_i0 · [x_0] + … + m_i(c-1) · [x_(c-1)]` of the
    /// input shares — a local, communication-free operation (see [`LinearShare`]).
    ///
    /// # Errors
    ///
    /// If the number of columns of the matrix does not match the number of shares, this function
    /// will return an [`Error::NotCompatible`] error.
    pub fn mul_shares<S>(&self, shares: &Vector<S>) -> Result<Vector<S>>
    where
        S: LinearShare<Value = T>,
    {
        if self.columns != shares.len() {
            return Err(Error::NotCompatible);
        }
        let mut result = Vec::with_capacity(self.rows);
        for i in 0..self.rows {
            // Seed with the first term: `LinearShare` has no zero share to start a sum from.
            let mut sum = shares[0].clone() * &self.elements[self.columns * i];
            for j in 1..self.columns {
                sum = sum + &(shares[j].clone() * &self.elements[self.columns * i + j]);
            }
            result.push(sum);
        }
        Ok(Vector::from(result))
    }
}

impl<T> Add<&Self> for Matrix<T>
where
    T: Ring,
{
    type Output = Result<Self>;
    fn add(mut self, rhs: &Self) -> Self::Output {
        if !self.is_compatible_with(rhs) {
            return Err(Error::NotCompatible);
        }
        self += rhs;
        Ok(self)
    }
}

impl<T> AddAssign<&Self> for Matrix<T>
where
    T: Ring,
{
    fn add_assign(&mut self, rhs: &Self) {
        for (self_elem, other_elem) in self.elements.iter_mut().zip(&rhs.elements) {
            *self_elem = self_elem.add(other_elem);
        }
    }
}

impl<T> Sub<&Self> for Matrix<T>
where
    T: Ring,
{
    type Output = Result<Self>;

    fn sub(mut self, rhs: &Self) -> Self::Output {
        if !self.is_compatible_with(rhs) {
            return Err(Error::NotCompatible);
        }
        self -= rhs;
        Ok(self)
    }
}

impl<T> SubAssign<&Self> for Matrix<T>
where
    T: Ring,
{
    fn sub_assign(&mut self, rhs: &Self) {
        for (self_elem, other_elem) in self.elements.iter_mut().zip(&rhs.elements) {
            *self_elem = self_elem.sub(other_elem);
        }
    }
}

impl<T> Mul<&Self> for Matrix<T>
where
    T: Ring,
{
    type Output = Result<Self>;

    fn mul(self, rhs: &Self) -> Self::Output {
        if self.columns != rhs.rows {
            return Err(Error::NotCompatible);
        }

        let rows = self.rows;
        let columns = rhs.columns;
        let interm = self.columns;

        let mut matrix = Self::allocate(rows, columns)?;
        for i in 0..rows {
            for j in 0..columns {
                let mut sum = T::ZERO;
                for k in 0..interm {
                    sum = sum.add(
                        &self.elements[self.columns * i + k]
                            .mul(&rhs.elements[rhs.columns * k + j]),
                    );
                }
                matrix.elements.push(sum);
            }
        }
        Ok(matrix)
    }
}

impl<T> Mul<&Vector<T>> for &Matrix<T>
where
    T: Ring,
{
    type Output = Result<Vector<T>>;

    fn mul(self, rhs: &Vector<T>) -> Self::Output {
        if self.columns != rhs.len() {
            return Err(Error::NotCompatible);
        }
        let mut elements = Vec::with_capacity(self.rows);
        for i in 0..self.rows {
            let mut sum = T::ZERO;
            for j in 0..self.columns {
                sum = sum.add(&self.elements[self.columns * i + j].mul(&rhs[j]))
            }
            elements.push(sum);
        }
        Ok(Vector::from(elements))
    }
}

impl<T> Mul<&Vector<T>> for Matrix<T>
where
    T: Ring,
{
    type Output = Result<Vector<T>>;

    fn mul(self, rhs: &Vector<T>) -> Self::Output {
        (&self).mul(rhs)
    }
}

impl<T> Mul<Vector<T>> for Matrix<T>
where
    T: Ring,
{
    type Output = Result<Vector<T>>;

    fn mul(self, rhs: Vector<T>) -> Self::Output {
        self.mul(&rhs)
    }
}

impl<T> Mul<Vector<T>> for &Matrix<T>
where
    T: Ring,
{
    type Output = Result<Vector<T>>;

    fn mul(self, rhs: Vector<T>) -> Self::Output {
        self.mul(&rhs)
    }
}

impl<T> Mul<&T> for Matrix<T>
where
    T: Ring,
{
    type Output = Matrix<T>;

    fn mul(mut self, rhs: &T) -> Self::Output {
        for i in 0..self.rows {
            for j in 0..self.rows {
                *self
                    .get_mut(i, j)
                    .expect("the indexes move on valid values") =
                    *self.get(i, j).expect("the indexes move on valid values") * rhs
            }
        }
        self
    }
}
