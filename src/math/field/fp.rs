use std::{
    fmt,
    hash::Hash,
    ops::{Add, Div, Mul, Neg, Sub},
};

use crypto_bigint::{
    modular::{ConstMontyForm, ConstMontyParams},
    Limb, NonZero, Random, Uint,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::math::{
    field::{FieldError, FiniteField},
    ring::Ring,
};

/// Element of the prime field $\mathbb{Z}_p$, where the modulus $p$ is fixed at compile time by
/// the type parameter `M`.
///
/// The modulus lives in the type rather than in the implementation, so a new field is obtained by
/// declaring a new modulus type instead of writing another implementation of [`Ring`] and
/// [`FiniteField`]. [`Mersenne61`](super::mersenne61::Mersenne61),
/// [`Secp256k1PrimeField`](super::secp256k1_prime::Secp256k1PrimeField) and
/// [`Secp256k1ScalarField`](super::secp256k1_scalar::Secp256k1ScalarField) are all aliases for
/// `Fp` under a different modulus. Because the modulus is part of the type, elements of two
/// different fields cannot be mixed by accident: the compiler rejects it.
///
/// # Type parameters
///
/// - `M`: a zero-sized type carrying the modulus $p$ together with the Montgomery constants
///   derived from it (see [`ConstMontyParams`]). The `const_monty_params!` macro of
///   `crypto-bigint` declares one from a big-endian hexadecimal literal.
/// - `LIMBS`: the number of [`Limb`]s used to store an element. It must match the width of the
///   integer type the modulus was declared with. Write it as `{ U256::LIMBS }` rather than as a
///   literal `4`, so that it stays correct on targets where a [`Limb`] is not 64 bits wide.
///
/// # Representation
///
/// Elements are stored in Montgomery form: the value $a$ is kept internally as $aR \bmod p$, with
/// $R = 2^{\texttt{LIMBS} \cdot w}$ and $w$ the width of a [`Limb`]. Multiplication then reduces
/// via a Montgomery reduction instead of a division by $p$, which is what makes the arithmetic
/// fast; the price is a conversion whenever an element is turned into an ordinary integer. The
/// stored form is always reduced modulo $p$ and therefore unique, so equality and [`Hash`]
/// operate directly on the limbs of the Montgomery representation.
///
/// # Requirements on the modulus
///
/// Montgomery arithmetic needs $p$ to be **odd**, which [`ConstMontyParams`] enforces when the
/// modulus is declared. For `Fp` to be a *field* — that is, for [`FiniteField::inverse`] to
/// succeed on every non-zero element — $p$ must further be **prime**. With a composite modulus
/// the type still behaves as the ring $\mathbb{Z}_p$, but `inverse` reports
/// [`FieldError::ZeroInverse`] for every non-invertible element, not only for zero.
///
/// # Examples
///
/// ```
/// use crypto_bigint::{const_monty_params, U64};
/// use scl_rs::math::{field::fp::Fp, ring::Ring};
///
/// const_monty_params!(Mod61, U64, "1FFFFFFFFFFFFFFF", "The Mersenne prime 2^61 - 1.");
///
/// type F = Fp<Mod61, { U64::LIMBS }>;
///
/// let two = F::ONE + &F::ONE;
/// let four = two * &two;
/// assert_eq!(four - &two, two);
/// assert_eq!((four / &two).unwrap(), two);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Fp<M: ConstMontyParams<LIMBS>, const LIMBS: usize>(ConstMontyForm<M, LIMBS>);

impl<M: ConstMontyParams<LIMBS>, const LIMBS: usize> Fp<M, LIMBS> {
    /// Creates a field element from an integer, reducing it modulo $p$ and converting it into
    /// Montgomery form.
    pub fn new(value: Uint<LIMBS>) -> Self {
        Self(ConstMontyForm::new(&value))
    }

    /// Converts the element out of Montgomery form, returning its canonical representative as an
    /// ordinary integer in $[0, p)$.
    ///
    /// This is the inverse of [`Fp::new`] and the accessor to reach for whenever the *value* of an
    /// element is needed rather than its arithmetic, such as when inspecting its bits.
    pub fn retrieve(&self) -> Uint<LIMBS> {
        self.0.retrieve()
    }
}

impl<M, const LIMBS: usize> Ring for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    const ZERO: Self = Self(ConstMontyForm::ZERO);
    const ONE: Self = Self(ConstMontyForm::ONE);
    const LIMBS: usize = LIMBS;
    const BIT_SIZE: usize = LIMBS * Limb::BITS as usize;

    fn negate(&self) -> Self {
        Self(self.0.neg())
    }

    fn random_non_zero<R: Rng>(generator: &mut R) -> Self {
        let mut random_try = Self::random(generator);
        while random_try == Self::ZERO {
            random_try = Self::random(generator);
        }
        random_try
    }

    fn random<R: Rng>(generator: &mut R) -> Self {
        Self(ConstMontyForm::random_from_rng(generator))
    }
}

impl<M, const LIMBS: usize> FiniteField<LIMBS> for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    const MODULUS: NonZero<Uint<LIMBS>> =
        NonZero::<Uint<LIMBS>>::new_unwrap(*M::PARAMS.modulus().as_ref());

    fn inverse(&self) -> Result<Self, FieldError> {
        self.0.invert().ok_or(FieldError::ZeroInverse).map(Self)
    }
}

impl<M, const LIMBS: usize> Neg for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl<M, const LIMBS: usize> fmt::Debug for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    /// Prints the canonical value of the element, not its Montgomery form. The two differ by a
    /// factor of $R$, so showing the stored limbs would misreport the number the element denotes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fp(0x{:X})", self.retrieve())
    }
}

impl<M, const LIMBS: usize> Hash for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for limb in self.0.as_montgomery().as_limbs() {
            state.write(&limb.to_le_bytes());
        }
    }
}

impl<M, const LIMBS: usize> Mul<&Self> for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    type Output = Self;

    fn mul(self, rhs: &Self) -> Self::Output {
        Self(self.0 * rhs.0)
    }
}

impl<M, const LIMBS: usize> Sub<&Self> for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    type Output = Self;

    fn sub(self, rhs: &Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl<M, const LIMBS: usize> Add<&Self> for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    type Output = Self;

    fn add(self, rhs: &Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl<M, const LIMBS: usize> Div<&Self> for Fp<M, LIMBS>
where
    M: ConstMontyParams<LIMBS>,
{
    type Output = Result<Self, FieldError>;

    fn div(self, rhs: &Self) -> Self::Output {
        Ok(self.mul(&rhs.inverse()?))
    }
}
