use std::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};

use rand::Rng;
use thiserror::Error;

use super::{ring::Ring, vector::Vector};

#[derive(Error, Debug)]
pub enum MatrixError {
    #[error("matrices are not compatible")]
    NotCompatible,

    #[error("invalid dimension: {0:?}, {1:?}")]
    InvalidDimension(usize, usize),
}

pub type Result<T> = std::result::Result<T, MatrixError>;

pub struct Matrix<T: Ring> {
    elements: Vec<T>,
    pub columns: usize,
    pub rows: usize,
}

impl<T> Matrix<T>
where
    T: Ring,
{
    fn new(rows: usize, columns: usize) -> Result<Self> {
        if rows == 0 || columns == 0 {
            return Err(MatrixError::InvalidDimension(rows, columns));
        }
        let elements = Vec::with_capacity(rows * columns);
        Ok(Self {
            elements,
            rows,
            columns,
        })
    }

    pub fn from_vec(rows: usize, columns: usize, elements: Vec<T>) -> Result<Self> {
        if rows * columns != elements.len() {
            return Err(MatrixError::InvalidDimension(rows, columns));
        }
        Ok(Self {
            elements,
            columns,
            rows,
        })
    }

    pub fn new_square(dim: usize) -> Result<Self> {
        Self::new(dim, dim)
    }

    pub fn identity(dim: usize) -> Self {
        let mut elements = Vec::with_capacity(dim * dim);
        for i in 0..dim {
            for j in 0..dim {
                if i == j {
                    elements.push(T::ONE);
                } else {
                    elements.push(T::ZERO);
                }
            }
        }
        Self {
            elements,
            rows: dim,
            columns: dim,
        }
    }

    pub fn zero(dim: usize) -> Self {
        let elements = vec![T::ZERO; dim * dim];
        Self {
            elements,
            rows: dim,
            columns: dim,
        }
    }

    pub fn random<R: Rng>(rows: usize, columns: usize, rng: &mut R) -> Result<Self> {
        let mut matrix = Self::new(rows, columns)?;
        for _ in 0..rows * columns {
            matrix.elements.push(T::random(rng));
        }
        Ok(matrix)
    }

    pub fn bit_size(&self) -> usize {
        self.rows * self.columns * T::BIT_SIZE
    }

    pub fn is_square(&self) -> bool {
        self.rows == self.columns
    }

    pub fn get(&self, i: usize, j: usize) -> Option<&T> {
        self.elements.get(self.rows * i + j)
    }

    pub fn get_mut(&mut self, i: usize, j: usize) -> Option<&mut T> {
        self.elements.get_mut(self.rows * i + j)
    }

    fn is_compatible_with(&self, other: &Self) -> bool {
        self.rows == other.rows && self.columns == other.columns
    }

    pub fn scalar_mut_in_place(&mut self, scalar: &T) {
        for elem in &mut self.elements {
            *elem = elem.mul(scalar);
        }
    }

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
            return Err(MatrixError::NotCompatible);
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
            return Err(MatrixError::NotCompatible);
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
            return Err(MatrixError::NotCompatible);
        }

        let rows = self.rows;
        let columns = rhs.columns;
        let interm = self.columns;

        let mut matrix = Self::new(rows, columns)?;
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
            return Err(MatrixError::NotCompatible);
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
