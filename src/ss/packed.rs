use crate::math::field::FiniteField;

pub struct PackedSS<const LIMBS: usize, F: FiniteField<LIMBS>> {
    share: F,
    degree: usize,
}

impl<const LIMBS: usize, F: FiniteField<LIMBS>> PackedSS<LIMBS, F> {
    pub fn new(share: F, degree: usize) -> Self {
        Self { share, degree }
    }
}
