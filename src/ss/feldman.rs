use crate::math::ec::EllipticCurve;

pub struct FeldmanSS<const LIMBS: usize, C: EllipticCurve<LIMBS>> {
    value: C::PrimeField,
    commitment: C,
}
