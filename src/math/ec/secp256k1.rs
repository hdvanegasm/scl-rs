use crate::math::{
    field::{
        secp256k1_prime::Secp256k1PrimeField, secp256k1_scalar::Secp256k1ScalarField, FiniteField,
    },
    ring::Ring,
};
use crypto_bigint::Uint;
use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul};

use super::EllipticCurve;

/// Implementation of secp256k1 using projective coordinates.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq)]
#[serde(try_from = "(Secp256k1PrimeField, Secp256k1PrimeField, Secp256k1PrimeField)")]
pub struct Secp256k1(
    Secp256k1PrimeField,
    Secp256k1PrimeField,
    Secp256k1PrimeField,
);

/// Errors that can occur when validating a `Secp256k1` point.
#[derive(thiserror::Error, Debug)]
pub enum Secp256k1Error {
    /// The coordinates do not satisfy the curve equation `Y^2 Z = X^3 + 7 Z^3`,
    /// so the point does not lie on secp256k1.
    #[error("invalid affine point: {x:?}, {y:?}, {z:?}")]
    InvalidPoint {
        /// The projective `x`-coordinate of the rejected point.
        x: Secp256k1PrimeField,
        /// The projective `y`-coordinate of the rejected point.
        y: Secp256k1PrimeField,
        /// The projective `z`-coordinate of the rejected point.
        z: Secp256k1PrimeField,
    },
    /// The `z`-coordinate is zero (the point at infinity), which is not accepted
    /// by the checked constructor.
    #[error("z-coordinate is zero")]
    ZeroZCoordinate,
}

/// Representation of a secp256k1 point using affine coordinates.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffinePoint(Secp256k1PrimeField, Secp256k1PrimeField);

impl AffinePoint {
    /// Returns the x-coordinate of the affine point.
    pub fn x(&self) -> &Secp256k1PrimeField {
        &self.0
    }

    /// Returns the y-coordinate of the affine point.
    pub fn y(&self) -> &Secp256k1PrimeField {
        &self.1
    }

    /// Checks if the affine point lies in the elliptic curve.
    pub fn is_valid(&self) -> bool {
        let b = Secp256k1PrimeField::from(7);
        let lhs = self.y().mul(self.y());
        let rhs = self.x().mul(self.x()).mul(self.x()).add(&b);
        lhs.eq(&rhs)
    }
}

impl
    TryFrom<(
        Secp256k1PrimeField,
        Secp256k1PrimeField,
        Secp256k1PrimeField,
    )> for Secp256k1
{
    type Error = Secp256k1Error;
    fn try_from(
        value: (
            Secp256k1PrimeField,
            Secp256k1PrimeField,
            Secp256k1PrimeField,
        ),
    ) -> Result<Self, Self::Error> {
        if value.2.eq(&Secp256k1PrimeField::ZERO) {
            return Err(Secp256k1Error::ZeroZCoordinate);
        }
        let b = Secp256k1PrimeField::from(7);

        // Computes y^2 * z
        let lhs = value.1.mul(&value.1).mul(&value.2);

        // Computes x^3 + 7 * z^3
        let rhs = value
            .0
            .mul(&value.0)
            .mul(&value.0)
            .add(&b.mul(&value.2.mul(&value.2.mul(&value.2))));
        if lhs.eq(&rhs) {
            Ok(Self(value.0, value.1, value.2))
        } else {
            Err(Secp256k1Error::InvalidPoint {
                x: value.0,
                y: value.1,
                z: value.2,
            })
        }
    }
}

impl Secp256k1 {
    /// Point at infinity using affine coordinates.
    pub const POINT_AT_INFINITY: Self = Self(
        Secp256k1PrimeField::ZERO,
        Secp256k1PrimeField::ONE,
        Secp256k1PrimeField::ZERO,
    );

    /// Returns the x-coordinate of the projective representation.
    pub fn x(&self) -> &Secp256k1PrimeField {
        &self.0
    }

    /// Returns the y-coordinate of the projective representation.
    pub fn y(&self) -> &Secp256k1PrimeField {
        &self.1
    }

    /// Returns the z-coordinate of the projective representation.
    pub fn z(&self) -> &Secp256k1PrimeField {
        &self.2
    }

    /// Converts the point from projective coordinates to affine coordinates.
    pub fn to_affine(&self) -> AffinePoint {
        if self.z().eq(&Secp256k1PrimeField::ONE) {
            AffinePoint(*self.x(), *self.y())
        } else {
            assert!(!(self.z() == &Secp256k1PrimeField::ZERO));
            let z = self.z().inverse().unwrap();
            AffinePoint(self.x().mul(&z), self.y().mul(&z))
        }
    }

    /// Checks if the point is the point at infinity.
    pub fn is_point_at_infinity(&self) -> bool {
        self.z().eq(&Secp256k1PrimeField::ZERO)
    }

    /// Doubles this point.
    pub fn dbl(&self) -> Self {
        let b3 = Secp256k1PrimeField::from(3 * 7);

        let mut t0 = *self.y() * self.y();
        let mut z3 = t0 + &t0;
        z3 = z3 + &z3;

        z3 = z3 + &z3;
        let mut t1 = *self.y() * self.z();
        let mut t2 = *self.z() * self.z();

        t2 = b3 * &t2;
        let mut x3 = t2 * &z3;
        let mut y3 = t0 + &t2;

        z3 = t1 * &z3;
        t1 = t2 + &t2;
        t2 = t1 + &t2;

        t0 = t0 - &t2;
        y3 = t0 * &y3;
        y3 = x3 + &y3;

        t1 = *self.x() * self.y();
        x3 = t0 * &t1;
        x3 = x3 + &x3;

        Self(x3, y3, z3)
    }
}

impl EllipticCurve<4> for Secp256k1 {
    type ScalarField = Secp256k1ScalarField;
    type PrimeField = Secp256k1PrimeField;
    const ZERO: Self = Self::POINT_AT_INFINITY;

    fn gen() -> Self {
        Self(
            Secp256k1PrimeField::new(Uint::from_be_hex(
                "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )),
            Secp256k1PrimeField::new(Uint::from_be_hex(
                "483ada7726a3c4655da4fbfc0e1108a8fd17b448a68554199c47d08ffb10d4b8",
            )),
            Secp256k1PrimeField::ONE,
        )
    }

    fn add(&self, rhs: &Self) -> Self {
        let b3 = Secp256k1PrimeField::from(3 * 7);

        let x1 = self.x();
        let y1 = self.y();
        let z1 = self.z();

        let x2 = rhs.x();
        let y2 = rhs.y();
        let z2 = rhs.z();

        let mut t0 = *x1 * x2;
        let mut t1 = *y1 * y2;
        let mut t2 = *z1 * z2;

        let mut t3 = *x1 + y1;
        let mut t4 = *x2 + y2;
        t3 = t3 * &t4;

        t4 = t0 + &t1;
        t3 = t3 - &t4;
        t4 = *y1 + z1;

        let mut x3 = *y2 + z2;
        t4 = t4 * &x3;
        x3 = t1 + &t2;

        t4 = t4 - &x3;
        x3 = *x1 + z1;
        let mut y3 = *x2 + z2;

        x3 = x3 * &y3;
        y3 = t0 + &t2;
        y3 = x3 - &y3;

        x3 = t0 + &t0;
        t0 = x3 + &t0;
        t2 = b3 * &t2;

        let mut z3 = t1 + &t2;
        t1 = t1 - &t2;
        y3 = b3 * &y3;

        x3 = t4 * &y3;
        t2 = t3 * &t1;
        x3 = t2 - &x3;

        y3 = y3 * &t0;
        t1 = t1 * &z3;
        y3 = t1 + &y3;

        t0 = t0 * &t3;
        z3 = z3 * &t4;
        z3 = z3 + &t0;

        Self(x3, y3, z3)
    }

    fn negate(&self) -> Self {
        if self.y().eq(&Self::PrimeField::ZERO) {
            Self::POINT_AT_INFINITY
        } else {
            Self(*self.x(), self.y().negate(), *self.z())
        }
    }

    fn scalar_mul(&self, scalar: &Self::ScalarField) -> Self {
        if !self.is_point_at_infinity() {
            let mut result = Self::POINT_AT_INFINITY;
            let naf = scalar.to_naf();
            for i in (0..naf.len()).rev() {
                result = result.dbl();
                if naf.pos(i) {
                    result = result.add(self);
                } else if naf.neg(i) {
                    result = result.sub(self);
                }
            }
            result
        } else {
            Self::POINT_AT_INFINITY
        }
    }

    fn sub(&self, rhs: &Self) -> Self {
        self.add(&rhs.negate())
    }
}

impl PartialEq for Secp256k1 {
    fn eq(&self, other: &Self) -> bool {
        self.x().mul(other.z()).eq(&other.x().mul(self.z()))
            && self.y().mul(other.z()).eq(&other.y().mul(self.z()))
    }
}
