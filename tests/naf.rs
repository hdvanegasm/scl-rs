use scl_rs::math::{
    field::{naf::NafEncoding, secp256k1_scalar::Secp256k1ScalarField},
    ring::Ring,
};
use std::ops::{Add, Sub};

#[test]
fn fixed_naf() {
    let naf = Secp256k1ScalarField::ZERO.to_naf();
    assert_eq!(NafEncoding::new(Secp256k1ScalarField::BIT_SIZE + 1), naf);

    let naf = Secp256k1ScalarField::from(13).to_naf();
    let mut true_naf = NafEncoding::new(Secp256k1ScalarField::BIT_SIZE + 1);
    true_naf.create_pos(0);
    true_naf.create_pos(4);
    true_naf.create_neg(2);
    assert_eq!(naf, true_naf);

    let naf = Secp256k1ScalarField::from(213).to_naf();
    let mut true_naf = NafEncoding::new(Secp256k1ScalarField::BIT_SIZE + 1);
    true_naf.create_pos(0);
    true_naf.create_pos(2);
    true_naf.create_pos(4);
    true_naf.create_pos(8);
    true_naf.create_neg(6);
    assert_eq!(naf, true_naf);
}

/// Decodes a NAF encoding back to a field element by summing `dᵢ · 2ⁱ`, where the digit
/// `dᵢ` is `+1` for a positive position, `-1` for a negative one, and `0` otherwise.
fn decode(naf: &NafEncoding) -> Secp256k1ScalarField {
    let two = Secp256k1ScalarField::from(2);
    let mut acc = Secp256k1ScalarField::ZERO;
    for i in 0..naf.len() {
        let weight = two.pow(i as u64);
        if naf.pos(i) {
            acc = acc.add(&weight);
        } else if naf.neg(i) {
            acc = acc.sub(&weight);
        }
    }
    acc
}

/// The defining property: a NAF encoding decodes back to the value it was built from.
#[test]
fn naf_reconstructs_value() {
    const REPETITIONS: usize = 50;
    let mut rng = rand::rng();

    // Edge cases plus random scalars.
    for value in [
        Secp256k1ScalarField::ZERO,
        Secp256k1ScalarField::ONE,
        Secp256k1ScalarField::from(2),
        Secp256k1ScalarField::from(u64::MAX),
    ] {
        assert_eq!(decode(&value.to_naf()), value);
    }

    for _ in 0..REPETITIONS {
        let value = Secp256k1ScalarField::random(&mut rng);
        assert_eq!(decode(&value.to_naf()), value);
    }
}

/// The "non-adjacent" property: no two consecutive digits are both nonzero.
#[test]
fn naf_is_non_adjacent() {
    const REPETITIONS: usize = 50;
    let mut rng = rand::rng();

    for _ in 0..REPETITIONS {
        let naf = Secp256k1ScalarField::random(&mut rng).to_naf();
        for i in 0..naf.len().saturating_sub(1) {
            let nonzero = !naf.zero(i);
            let next_nonzero = !naf.zero(i + 1);
            assert!(
                !(nonzero && next_nonzero),
                "adjacent nonzero digits at positions {i} and {}",
                i + 1
            );
        }
    }
}
