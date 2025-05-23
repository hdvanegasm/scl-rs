use super::{ring::Ring, vector::Vector};
use crypto_bigint::rand_core::RngCore;
use serde::Serialize;
use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};
use thiserror::Error;

/// Errors that may occurs when creating and operating with matrices.
#[derive(Error, Debug)]
pub enum Error {
    /// The matrices that you are trying to operate are not compatible.
    #[error("matrices are not compatible")]
    NotCompatible,

    /// The matrix that is being created does not have the correct dimension.
    #[error("the matrix has an invalid dimension: {0:?}, {1:?}")]
    InvalidDimension(usize, usize),
}

/// Specialized result for the [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

/// Matrix with elements in a ring.
#[derive(Serialize, Debug, Eq)]
pub struct Matrix<T: Ring> {
    /// Elements of the matrix.
    elements: Vec<T>,
    /// Columns of the matrix.
    pub columns: usize,
    /// Rows of the matrix.
    pub rows: usize,
}

impl<T> Matrix<T>
where
    T: Ring,
{
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

    /// Creates a random matrix with the given dimensions.
    ///
    /// # Errors
    ///
    /// If one or more of the dimensions are zero, the function will return an
    /// error.
    pub fn random<R: RngCore>(rows: usize, columns: usize, rng: &mut R) -> Result<Self> {
        let mut matrix = Self::allocate(rows, columns)?;
        for _ in 0..rows * columns {
            matrix.elements.push(T::random(rng));
        }
        Ok(matrix)
    }

    /// Returns whether the matrix is square or not.
    pub fn is_square(&self) -> bool {
        self.rows == self.columns
    }

    /// Get the field element in i-th row and j-th column.
    pub fn get(&self, i: usize, j: usize) -> Option<&T> {
        self.elements.get(self.rows * i + j)
    }

    /// Get the field element in i-th row and j-th column as a mutable reference.
    pub fn get_mut(&mut self, i: usize, j: usize) -> Option<&mut T> {
        self.elements.get_mut(self.rows * i + j)
    }

    /// Return wether this matrix is compatible with another matrix for
    /// multiplication.
    fn is_compatible_with(&self, other: &Self) -> bool {
        self.rows == other.rows && self.columns == other.columns
    }

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
}

impl<T> PartialEq<Self> for Matrix<T>
where
    T: Ring,
{
    fn eq(&self, other: &Self) -> bool {
        if !self.is_compatible_with(other) {
            return false;
        }
        self.elements == other.elements
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
                        &self.elements[self.rows * i + k].mul(&rhs.elements[rhs.rows * k + j]),
                    );
                }
                matrix.elements.push(sum);
            }
        }
        Ok(matrix)
    }
}

impl<T> Mul<&Vector<T>> for Matrix<T>
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
                sum = sum.add(&self.elements[self.rows * i + j].mul(&rhs[j]))
            }
            elements.push(sum);
        }
        Ok(Vector::from(elements))
    }
}
