use crypto_bigint::rand_core::OsRng;
use scl_rs::{
    math::{field::mersenne61::Mersenne61, ring::Ring},
    ss::additive::AdditiveSS,
};

#[test]
fn construct_shares_and_reconstruct() {
    const REPETITIONS: usize = 100;
    const N_PARTIES: usize = 20;

    for _ in 0..REPETITIONS {
        let secret = Mersenne61::random(&mut OsRng);
        let shares = AdditiveSS::shares_from_secret(secret, N_PARTIES, &mut OsRng);
        let rec_secret = AdditiveSS::secret_from_shares(&shares);
        assert_eq!(secret, rec_secret);
    }
}
