use serde::{Deserialize, Serialize};

use crate::math::field::FiniteField;

struct ShamirSS<const LIMBS: usize, F: FiniteField<LIMBS>> {
    share: F,
    degree: usize,
}

impl<const LIMBS: usize, F> ShamirSS<LIMBS, F>
where
    F: FiniteField<LIMBS>,
{
    fn shares_from_secret(secret: F, degree: usize, n_parties: usize) -> Vec<F> {
        todo!()
    }
}
