use super::FiniteField;

/// Computes the lagrange basis evaluated at `x`
pub(crate) fn compute_lagrange_basis<T: FiniteField>(nodes: Vec<T>, x: &T) -> Vec<T> {
    let mut lagrange_basis = Vec::with_capacity(nodes.len());
    for j in 0..nodes.len() {
        let mut basis = T::ONE;
        let x_j = &nodes[j];
        for (m, node) in nodes.iter().enumerate() {
            if m != j {
                let x_m = node;
                let numerator = x.sub(x_m);
                let denominator = x_j.sub(x_m);

                // The unwrap is safe because x_j - x_m is not zero.
                let term = numerator.mul(&denominator.inverse().unwrap());
                basis = basis.mul(&term);
            }
        }
        lagrange_basis.push(basis);
    }
    lagrange_basis
}

/// Computes the evaluation of the interpolated polynomial at `x`.
pub fn interpolate_polynomial_at<T: FiniteField>(evaluations: Vec<T>, alphas: Vec<T>, x: &T) -> T {
    assert!(alphas.len() == evaluations.len());
    let lagrange_basis = compute_lagrange_basis(alphas, x);
    let mut interpolation = T::ZERO;
    for (eval, basis) in evaluations.into_iter().zip(lagrange_basis) {
        interpolation = interpolation.add(&eval.mul(&basis));
    }
    interpolation
}
