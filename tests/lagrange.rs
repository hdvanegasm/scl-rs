use crypto_bigint::rand_core::{OsRng, RngCore};
use rand::seq::SliceRandom;
use scl_rs::math::{
    field::mersenne61::Mersenne61,
    poly::{interpolate_polynomial_at, Polynomial},
    ring::Ring,
};

#[test]
fn interpolation() {
    const MAX_DEGREE: u64 = 100;
    const N_SAMPLES: u64 = 100;

    for _ in 0..N_SAMPLES {
        let degree: usize = (OsRng.next_u64() % MAX_DEGREE) as usize;
        let random_poly: Polynomial<1, Mersenne61> = Polynomial::random(degree, &mut OsRng);

        let evaluation_test = Mersenne61::random(&mut OsRng);

        // Generates degree + 1 evaluation points
        let mut eval_points: Vec<usize> = (0..1000).collect();
        eval_points.shuffle(&mut rand::rng());
        let eval_points: Vec<Mersenne61> = eval_points[0..degree + 1]
            .iter()
            .map(|elem| Mersenne61::from(*elem as u64))
            .collect();

        let evaluations: Vec<Mersenne61> = eval_points
            .iter()
            .map(|x| random_poly.evaluate(x))
            .collect();

        let interpolated_evaluation =
            interpolate_polynomial_at(&evaluations, &eval_points, &evaluation_test).unwrap();

        assert_eq!(
            interpolated_evaluation,
            random_poly.evaluate(&evaluation_test)
        )
    }
}
