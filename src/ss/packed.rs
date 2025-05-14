use crate::math::field::FiniteField;

struct PackedSS<const LIMBS: usize, F: FiniteField<LIMBS>> {
    share: F,
    degree: usize,
}
