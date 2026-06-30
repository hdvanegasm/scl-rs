use scl_rs::math::ec::secp256k1::Secp256k1;
use scl_rs::math::ec::EllipticCurve;
use scl_rs::math::field::secp256k1_scalar::Secp256k1ScalarField;
use scl_rs::math::ring::Ring;
use scl_rs::{math::field::mersenne61::Mersenne61, net::Packet};

#[test]
fn serialize_deserialize_one_object() {
    let element = Mersenne61::from(3);
    let mut packet = Packet::empty();
    packet.write(&element).unwrap();
    let element_unpacked = packet.pop().unwrap();
    assert_eq!(element, element_unpacked);
}

#[test]
fn serialize_deserialize_multiple_different_objects() {
    let mut rng = rand::rng();
    let scalar = Secp256k1ScalarField::random_non_zero(&mut rng);
    let ec_element = Secp256k1::gen().scalar_mul(&scalar);
    let mut packet = Packet::empty();

    // Insert elements.
    packet.write(&scalar).unwrap();
    packet.write(&ec_element).unwrap();

    // Unpack elements.
    let ec_unpacked = packet.pop().unwrap();
    let scalar_unpacked = packet.pop().unwrap();
    assert_eq!(scalar, scalar_unpacked);
    assert_eq!(ec_element, ec_unpacked);
}

use proptest::prelude::*;

mod common;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// A heterogeneous `Packet` (a scalar field element, an on-curve point, and a Mersenne61
    /// element) survives a `postcard` round-trip unchanged.
    #[test]
    fn postcard_roundtrip(
        scalar in common::field_element::<Secp256k1ScalarField>(),
        m in common::field_element::<Mersenne61>(),
    ) {
        let point = Secp256k1::gen().scalar_mul(&scalar);
        let mut packet = Packet::empty();
        packet.write(&scalar).unwrap();
        packet.write(&point).unwrap();
        packet.write(&m).unwrap();
        common::roundtrip(packet)?;
    }
}
