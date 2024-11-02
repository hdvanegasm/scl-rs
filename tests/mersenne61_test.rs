use rand::{thread_rng, Rng};
use scl_rs::math::field::{mersenne61::Mersenne61, FiniteField};

#[test]
fn multiplicative_and_additive_properties() {
    let mut rng = rand::thread_rng();
    let rnd_value: u64 = rng.gen();
    let a = Mersenne61::from(rnd_value);

    let add_identity = a.add(&Mersenne61::ZERO);
    assert!(add_identity.equal(&a));

    let neg_a = a.negate();
    let add_neg = a.add(&neg_a);
    assert!(add_neg.equal(&Mersenne61::ZERO))
}

#[test]
fn zero() {
    let mut rng = thread_rng();
    let elem = Mersenne61::random(&mut rng);
    let s = elem.add(&Mersenne61::ZERO);
    assert_eq!(elem, s);

    let elem = Mersenne61::random(&mut rng);
    let s = elem.subtract(&Mersenne61::ZERO);
    assert_eq!(elem, s);
}

#[test]
fn one() {
    let mut rng = thread_rng();
    let elem = Mersenne61::random(&mut rng);
    let s = elem.multiply(&Mersenne61::ONE);
    assert_eq!(elem, s);
}

#[test]
fn negate() {
    let mut rng = thread_rng();
    let elem = Mersenne61::random(&mut rng);
    let s = elem.add(&elem.negate());
    assert_eq!(s, Mersenne61::ZERO);
}

#[test]
fn subract() {
    let mut rng = thread_rng();
    let elem = Mersenne61::random(&mut rng);
    let s = elem.subtract(&elem);
    assert_eq!(s, Mersenne61::ZERO);
}

#[test]
fn inverse() {
    let mut rng = thread_rng();
    const SAMPLES: usize = 100;
    for _ in 0..SAMPLES {
        let elem = Mersenne61::random(&mut rng);
        let s = elem.multiply(&elem.inverse().unwrap());
        assert_eq!(s, Mersenne61::ONE);
    }
}

#[test]
fn mult_test1() {
    let a = Mersenne61::from(2);
    let b = Mersenne61::from(6);
    let r = Mersenne61::from(12);

    let s = a.multiply(&b);
    assert_eq!(s, r);
}

#[test]
fn mult_conmutativity() {
    const SAMPLES: usize = 50;
    let mut rng = thread_rng();
    for _ in 0..SAMPLES {
        let a = Mersenne61::random(&mut rng);
        let b = Mersenne61::random(&mut rng);
        let mult1 = a.multiply(&b);
        let mult2 = b.multiply(&a);
        assert_eq!(mult1, mult2);
    }
}
