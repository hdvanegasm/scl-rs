use scl_rs::math::{field::secp256k1_prime::Secp256k1PrimeField, ring::Ring, vector::Vector};

#[test]
fn dot_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len);
    let dot_prod = vector.dot(&zero_vec);
    assert_eq!(dot_prod.unwrap(), Secp256k1PrimeField::ZERO);
}

#[test]
fn add_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len);
    let add_vec = &vector + &zero_vec;
    assert_eq!(add_vec.unwrap(), vector);
}

#[test]
#[should_panic]
fn incompatible_dot() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    vector.dot(&zero_vec).unwrap();
}

#[test]
#[should_panic]
fn incompatible_add_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let add_vec = &vector + &zero_vec;
    add_vec.unwrap();
}

#[test]
#[should_panic]
fn incompatible_sub_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let sub = &vector - &zero_vec;
    sub.unwrap();
}

#[test]
#[should_panic]
fn incompatible_mul_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let mul = &vector * &zero_vec;
    mul.unwrap();
}

use scl_rs::math::field::mersenne61::Mersenne61;

#[test]
fn len_and_is_empty() {
    let v: Vector<Mersenne61> = Vector::zero(5);
    assert_eq!(v.len(), 5);
    assert!(!v.is_empty());

    let empty: Vector<Mersenne61> = Vector::from(Vec::new());
    assert!(empty.is_empty());
}

#[test]
fn index_read_and_write() {
    let mut v: Vector<Mersenne61> = Vector::from((1..=3).map(Mersenne61::from).collect::<Vec<_>>());
    assert_eq!(v[0], Mersenne61::from(1));
    assert_eq!(v[2], Mersenne61::from(3));
    v[1] = Mersenne61::from(42);
    assert_eq!(v[1], Mersenne61::from(42));
}

#[test]
fn dot_known_value() {
    let a: Vector<Mersenne61> =
        Vector::from(vec![1, 2, 3].into_iter().map(Mersenne61::from).collect::<Vec<_>>());
    let b: Vector<Mersenne61> =
        Vector::from(vec![4, 5, 6].into_iter().map(Mersenne61::from).collect::<Vec<_>>());
    // 1*4 + 2*5 + 3*6 = 32
    assert_eq!(a.dot(&b).unwrap(), Mersenne61::from(32));
}

#[test]
fn ones_dot_is_length() {
    let n = 10;
    let ones: Vector<Mersenne61> = Vector::ones(n);
    assert_eq!(ones.dot(&ones).unwrap(), Mersenne61::from(n as u64));
}

#[test]
fn elementwise_mul_known() {
    let a: Vector<Mersenne61> =
        Vector::from(vec![2, 3].into_iter().map(Mersenne61::from).collect::<Vec<_>>());
    let b: Vector<Mersenne61> =
        Vector::from(vec![5, 7].into_iter().map(Mersenne61::from).collect::<Vec<_>>());
    let prod = (&a * &b).unwrap();
    let expected: Vector<Mersenne61> =
        Vector::from(vec![10, 21].into_iter().map(Mersenne61::from).collect::<Vec<_>>());
    assert_eq!(prod, expected);
}
