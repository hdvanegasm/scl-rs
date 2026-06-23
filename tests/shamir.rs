use itertools::Itertools;
use rand::seq::SliceRandom;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    prelude::Ring,
    ss::{shamir::ShamirSS, ShareError},
};

fn party_indexes(n: u64) -> Vec<Mersenne61> {
    (1..=n).map(Mersenne61::from).collect()
}

#[test]
fn reconstruct_is_subset_invariant() {
    const T: usize = 3;
    const N: usize = 10;

    let mut rng = rand::rng();
    let secret = Mersenne61::random(&mut rng);
    let indexes = party_indexes(N as u64);
    let (shares, _) = ShamirSS::shares_from_secret(secret, T, &indexes, &mut rng);

    // Let us generate all possible T + 1 subsets out of N.
    for mut idx_set in (1..=N).combinations(T + 1) {
        idx_set.shuffle(&mut rng);
        let shares_set: Vec<_> = idx_set.iter().map(|&i| shares[i - 1].clone()).collect();
        let party_indexes: Vec<_> = idx_set.iter().map(|&i| indexes[i - 1]).collect();
        let reconstr = ShamirSS::secret_from_shares(&shares_set, &party_indexes).unwrap();
        assert_eq!(reconstr, secret);
    }
}

#[test]
fn too_few_shares_is_not_enough() {
    const T: usize = 3;
    const N: usize = 10;

    let mut rng = rand::rng();
    let secret = Mersenne61::random(&mut rng);
    let indexes = party_indexes(N as u64);
    let (shares, _) = ShamirSS::shares_from_secret(secret, T, &indexes, &mut rng);

    // Let us generate all possible T + 1 subsets out of N.
    for t_values in 1..=T {
        for mut idx_set in (1..N).combinations(t_values) {
            idx_set.shuffle(&mut rng);
            let shares_set: Vec<_> = idx_set.iter().map(|&i| shares[i - 1].clone()).collect();
            let party_indexes: Vec<_> = idx_set.iter().map(|&i| indexes[i - 1]).collect();
            let reconstr = ShamirSS::secret_from_shares(&shares_set, &party_indexes);
            assert!(matches!(reconstr, Err(ShareError::NotEnoughShares)));
        }
    }
}
