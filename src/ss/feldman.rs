use crate::math::ec::EllipticCurve;

pub struct FeldmanSS<const LIMBS: usize, C: EllipticCurve<LIMBS>> {
    value: C::ScalarField,
    commitment: C,
}

impl<const LIMBS: usize, C: EllipticCurve<LIMBS>> FeldmanSS<LIMBS, C> {
    pub fn new(value: C::ScalarField, commitment: C) -> Self {
        Self { value, commitment }
    }

    /// Checks if the share is valid with respect to the commitment.
    pub fn is_valid(&self) -> bool {
        C::gen().scalar_mul(&self.value).eq(&self.commitment)
    }
}
