use scl_rs::{
    math::{field::mersenne61::Mersenne61, ring::Ring},
    net::PartyId,
    ss::additive::AdditiveSS,
};

#[test]
fn construct_shares_and_reconstruct() {
    const REPETITIONS: usize = 100;
    const N_PARTIES: usize = 20;

    for _ in 0..REPETITIONS {
        let secret = Mersenne61::random(&mut rand::rng());
        let parties: Vec<PartyId> = (0..N_PARTIES).map(PartyId::from).collect();
        let shares = AdditiveSS::shares_from_secret(secret, &parties, &mut rand::rng());
        let rec_secret = AdditiveSS::secret_from_shares(&shares);
        assert_eq!(secret, rec_secret);
    }
}
