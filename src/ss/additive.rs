use rand::Rng;

use crate::math::ring::Ring;

pub fn compute_additive_shares<T: Ring, R: Rng>(secret: T, n_parties: usize, rng: R) -> Vec<T> {
    todo!()
}

pub fn reconstruct_from_additive_shares<T: Ring>(shares: Vec<T>) -> T {
    todo!()
}
