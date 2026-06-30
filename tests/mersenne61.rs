use rand::Rng;
use scl_rs::math::{
    field::{mersenne61::Mersenne61, FiniteField},
    ring::Ring,
};
use std::ops::{Add, Mul, Sub};

#[test]
fn multiplicative_and_additive_properties() {
    let rnd_value: u64 = rand::rng().next_u64();
    let a = Mersenne61::from(rnd_value);

    let add_identity = a.add(&Mersenne61::ZERO);
    assert!(add_identity.eq(&a));

    let neg_a = a.negate();
    let add_neg = a.add(&neg_a);
    assert!(add_neg.eq(&Mersenne61::ZERO))
}

#[test]
fn zero() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.add(&Mersenne61::ZERO);
    assert_eq!(elem, s);

    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.sub(&Mersenne61::ZERO);
    assert_eq!(elem, s);
}

#[test]
fn one() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.mul(&Mersenne61::ONE);
    assert_eq!(elem, s);
}

#[test]
fn negate() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.add(&elem.negate());
    assert_eq!(s, Mersenne61::ZERO);
}

#[test]
fn subract() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.sub(&elem);
    assert_eq!(s, Mersenne61::ZERO);
}

#[test]
fn inverse() {
    const SAMPLES: usize = 100;
    for _ in 0..SAMPLES {
        let elem = Mersenne61::random_non_zero(&mut rand::rng());
        let s = elem.mul(&elem.inverse().unwrap());
        assert_eq!(s, Mersenne61::ONE);
    }
}

#[test]
fn mult_test1() {
    let a = Mersenne61::from(2);
    let b = Mersenne61::from(6);
    let r = Mersenne61::from(12);

    let s = a.mul(&b);
    assert_eq!(s, r);
}

#[test]
fn mult_conmutativity() {
    const SAMPLES: usize = 50;
    for _ in 0..SAMPLES {
        let a = Mersenne61::random(&mut rand::rng());
        let b = Mersenne61::random(&mut rand::rng());
        let mult1 = a.mul(&b);
        let mult2 = b.mul(&a);
        assert_eq!(mult1, mult2);
    }
}

use proptest::prelude::*;

mod common;

fn element() -> impl Strategy<Value = Mersenne61> {
    common::field_element()
}

proptest! {
    #[test]
    fn mul_distributes_over_add(a in element(), b in element(), c in element()) {
        prop_assert_eq!(a * &(b + &c), (a * &b) + &(a * &c));
    }

    #[test]
    fn mul_inverse_equals_one(a in element()) {
        prop_assume!(a != Mersenne61::ZERO);
        prop_assert_eq!(a * &(a.inverse().unwrap()), Mersenne61::ONE)
    }

    #[test]
    fn add_commutes(a in element(), b in element()) {
        prop_assert_eq!(a + &b, b + &a);
    }

    #[test]
    fn add_associates(a in element(), b in element(), c in element()) {
        prop_assert_eq!((a + &b) + &c, a + &(b + &c));
    }

    #[test]
    fn add_identity(a in element()) {
        prop_assert_eq!(a + &Mersenne61::ZERO, a);
    }

    #[test]
    fn add_inverse_equals_zero(a in element()) {
        prop_assert_eq!(a + &a.negate(), Mersenne61::ZERO);
    }

    #[test]
    fn mul_commutes(a in element(), b in element()) {
        prop_assert_eq!(a * &b, b * &a);
    }

    #[test]
    fn mul_associates(a in element(), b in element(), c in element()) {
        prop_assert_eq!((a * &b) * &c, a * &(b * &c));
    }

    #[test]
    fn mul_identity(a in element()) {
        prop_assert_eq!(a * &Mersenne61::ONE, a);
    }

    #[test]
    fn sub_self_equals_zero(a in element()) {
        prop_assert_eq!(a - &a, Mersenne61::ZERO);
    }

    #[test]
    fn sub_equals_add_negate(a in element(), b in element()) {
        prop_assert_eq!(a - &b, a + &b.negate());
    }
}
