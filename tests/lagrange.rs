//! Integration test for Lagrange interpolation (`math::poly::interpolate_polynomial_at`) over
//! `Mersenne61`.

use rand::seq::SliceRandom;
use rand::Rng;
use scl_rs::math::{
    field::mersenne61::Mersenne61,
    poly::{interpolate_polynomial_at, Polynomial},
    ring::Ring,
};

/// Checks that interpolating `degree + 1` evaluations of a random polynomial gives its value at a
/// random point, over 100 trials with degrees below 100 and distinct nodes drawn from `0..1000`.
#[test]
fn interpolation() {
    const MAX_DEGREE: u64 = 100;
    const N_SAMPLES: u64 = 100;

    for _ in 0..N_SAMPLES {
        let mut rng = rand::rng();
        let degree: usize = (rng.next_u64() % MAX_DEGREE) as usize;
        let random_poly: Polynomial<Mersenne61> = Polynomial::random(degree, &mut rng);

        let evaluation_test = Mersenne61::random(&mut rng);

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
