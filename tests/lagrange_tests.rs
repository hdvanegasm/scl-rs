use rand::{seq::SliceRandom, thread_rng, Rng};

use scl_rs::math::{
    field::{lagrange::interpolate_polynomial_at, mersenne61::Mersenne61, FiniteField, Polynomial},
    ring::Ring,
};

#[test]
fn interpolation() {
    const MAX_DEGREE: u64 = 100;
    const N_SAMPLES: u64 = 100;

    let mut rng = thread_rng();

    for _ in 0..N_SAMPLES {
        let degree: usize = (rng.gen::<u64>() % MAX_DEGREE) as usize;
        let random_poly: Polynomial<Mersenne61> = Polynomial::random(degree, &mut rng);

        let evaluation_test = Mersenne61::random(&mut rng);

        // Generates degree + 1 evaluation points
        let mut eval_points: Vec<usize> = (0..1000).collect();
        eval_points.shuffle(&mut rng);
        let eval_points: Vec<Mersenne61> = eval_points[0..degree + 1]
            .iter()
            .map(|elem| Mersenne61::from(*elem as u64))
            .collect();

        let evaluations = eval_points
            .iter()
            .map(|x| random_poly.evaluate(x))
            .collect();

        let interpolated_evaluation =
            interpolate_polynomial_at(evaluations, eval_points, &evaluation_test);

        assert_eq!(
            interpolated_evaluation,
            random_poly.evaluate(&evaluation_test)
        )
    }
}
