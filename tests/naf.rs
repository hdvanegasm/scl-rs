use scl_rs::math::{
    field::{naf::NafEncoding, secp256k1_scalar::Secp256k1ScalarField},
    ring::Ring,
};

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
