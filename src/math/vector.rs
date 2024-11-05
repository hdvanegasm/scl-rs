use super::ring::Ring;
use std::ops::{Add, Index, IndexMut, Mul, Sub};

pub struct Vector<T: Ring>(Vec<T>);

impl<T> Vector<T>
where
    T: Ring,
{
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn zero(len: usize) -> Self {
        Self(vec![T::ZERO; len])
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
    type Output = Self;

    fn add(self, other: &Vector<T>) -> Self::Output {
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem + other_elem);
        }
        Self(output)
    }
}

impl<T> Sub<&Vector<T>> for Vector<T>
where
    T: Ring,
{
    type Output = Self;

    fn sub(self, other: &Vector<T>) -> Self::Output {
        let mut output = Vec::with_capacity(other.0.len());
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            output.push(*self_elem - other_elem);
        }
        Self(output)
    }
}

impl<T> Mul<&Vector<T>> for Vector<T>
where
    T: Ring,
{
    type Output = T;

    fn mul(self, other: &Vector<T>) -> Self::Output {
        let mut result = T::ZERO;
        for (self_elem, other_elem) in self.0.iter().zip(other.0.iter()) {
            result = result + &(*self_elem * other_elem);
        }
        result
    }
}
