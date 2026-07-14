use itertools::Itertools;
use rand::seq::SliceRandom;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::PartyId,
    prelude::Ring,
    ss::{shamir::ShamirSS, LinearShare, ShareError},
};

fn party_indexes(n: u64) -> Vec<Mersenne61> {
    (1..=n).map(Mersenne61::from).collect()
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

/// Trait-level dealing honors the caller-chosen threshold: dealt at degree `t` through
/// `LinearShare`, any `t + 1` shares reconstruct — the degree is no longer hardcoded to `n - 1`.
#[test]
fn linear_share_deal_with_threshold_reconstructs() {
    const T: usize = 2;
    const N: usize = 5;

    let mut rng = rand::rng();
    let secret = Mersenne61::random(&mut rng);
    let parties: Vec<PartyId> = (0..N).map(PartyId::from).collect();

    let shares =
        <ShamirSS<1, Mersenne61> as LinearShare>::shares_from_secret(secret, &parties, T, &mut rng)
            .unwrap();
    assert!(shares.iter().all(|share| share.degree() == T));

    let reconstructed = <ShamirSS<1, Mersenne61> as LinearShare>::secret_from_shares(
        &shares[..T + 1],
        &parties[..T + 1],
    )
    .unwrap();
    assert_eq!(reconstructed, secret);
}

/// A degree that even all `n` dealt shares could never reconstruct is rejected at dealing time
/// instead of producing an unreconstructable sharing.
#[test]
fn linear_share_invalid_threshold_is_rejected() {
    const N: usize = 3;

    let mut rng = rand::rng();
    let secret = Mersenne61::random(&mut rng);
    let parties: Vec<PartyId> = (0..N).map(PartyId::from).collect();

    let result =
        <ShamirSS<1, Mersenne61> as LinearShare>::shares_from_secret(secret, &parties, N, &mut rng);
    assert!(matches!(
        result,
        Err(ShareError::InvalidThreshold {
            threshold: N,
            n_parties: N,
        })
    ));
}

use proptest::prelude::*;
use rand::{rngs::StdRng, SeedableRng};

mod common;

proptest! {
    /// Subset invariance across random configurations: for a random secret, threshold, and party
    /// count (including `t = 0` and `t = n - 1`), reconstruct from two independently chosen
    /// `(t + 1)`-subsets and assert both recover the same secret.
    #[test]
    fn reconstruction_is_subset_invariant_across_configs(
        (n, t) in (2usize..=12).prop_flat_map(|n| (Just(n), 0usize..n)),
        secret in common::field_element::<Mersenne61>(),
        seed in any::<[u8; 32]>(),
    ) {
        let mut rng = StdRng::from_seed(seed);
        let indexes = party_indexes(n as u64);
        let (shares, _) = ShamirSS::shares_from_secret(secret, t, &indexes, &mut rng);

        let mut positions: Vec<usize> = (0..n).collect();
        for _ in 0..2 {
            positions.shuffle(&mut rng);
            let chosen = &positions[..t + 1];
            let shares_set: Vec<_> = chosen.iter().map(|&i| shares[i].clone()).collect();
            let idx_set: Vec<_> = chosen.iter().map(|&i| indexes[i]).collect();
            let reconstr = ShamirSS::secret_from_shares(&shares_set, &idx_set).unwrap();
            prop_assert_eq!(reconstr, secret);
        }
    }

    /// A `ShamirSS` share survives a `postcard` serialization round-trip unchanged.
    #[test]
    fn postcard_roundtrip(
        share in common::field_element::<Mersenne61>(),
        degree in 0usize..16,
    ) {
        let s: ShamirSS<1, Mersenne61> = ShamirSS::new(share, degree);
        common::roundtrip(s)?;
    }
}
