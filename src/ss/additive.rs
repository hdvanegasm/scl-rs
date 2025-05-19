use crypto_bigint::rand_core::RngCore;

use crate::math::ring::Ring;

pub struct AdditiveSS<T: Ring>(T);

impl<T> AdditiveSS<T>
where
    T: Ring,
{
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn value(&self) -> T {
        self.0
    }

    pub fn shares_from_secret<R: RngCore>(secret: T, n_parties: usize, rng: &mut R) -> Vec<Self> {
        let mut shares = Vec::with_capacity(n_parties);
        let mut rand_acc = T::ZERO;
        for _ in 1..n_parties {
            let rnd_ring_value = T::random(rng);
            rand_acc = rand_acc + &rnd_ring_value;
            shares.push(Self(rnd_ring_value));
        }
        let last_share = Self(secret - &rand_acc);
        shares.push(last_share);
        shares
    }

    pub fn secret_from_shares(shares: &[Self]) -> T {
        shares
            .iter()
            .fold(T::ZERO, |acc, share| acc + &share.value())
    }
}
