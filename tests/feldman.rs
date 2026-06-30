use itertools::Itertools;
use rand::seq::SliceRandom;
use scl_rs::{
    math::{ec::secp256k1::Secp256k1, field::secp256k1_scalar::Secp256k1ScalarField},
    prelude::Ring,
    ss::feldman::FeldmanSS,
};

fn party_indexes(n: u64) -> Vec<Secp256k1ScalarField> {
    (1..=n).map(Secp256k1ScalarField::from).collect()
}

#[test]
fn round_trip() {
    const T: usize = 3;
    const N: usize = 10;

    let mut rng = rand::rng();
    let secret = Secp256k1ScalarField::random(&mut rng);
    let indexes = party_indexes(N as u64);
    let shares: Vec<FeldmanSS<_, Secp256k1>> =
        FeldmanSS::shares_from_secret(secret, T, &indexes, &mut rng);

    // Let us generate all possible T + 1 subsets out of N.
    for mut idx_set in (1..=N).combinations(T + 1) {
        idx_set.shuffle(&mut rng);
        let shares_set: Vec<_> = idx_set.iter().map(|&i| shares[i - 1].clone()).collect();
        let party_indexes: Vec<_> = idx_set.iter().map(|&i| indexes[i - 1]).collect();
        let reconstr = FeldmanSS::secret_from_shares(&shares_set, &party_indexes).unwrap();
        assert_eq!(reconstr, secret);
    }
}

use proptest::prelude::*;
use rand::{rngs::StdRng, SeedableRng};

mod common;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// A `FeldmanSS` share (a Shamir share plus its commitment vector) survives a `postcard`
    /// round-trip unchanged, across random secrets and polynomial degrees.
    #[test]
    fn postcard_roundtrip(
        secret in common::field_element::<Secp256k1ScalarField>(),
        degree in 0usize..8,
        seed in any::<[u8; 32]>(),
    ) {
        let mut rng = StdRng::from_seed(seed);
        let indexes = party_indexes((degree + 1) as u64);
        let shares: Vec<FeldmanSS<_, Secp256k1>> =
            FeldmanSS::shares_from_secret(secret, degree, &indexes, &mut rng);
        common::roundtrip(shares[0].clone())?;
    }
}
