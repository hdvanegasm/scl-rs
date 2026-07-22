//! End-to-end tests for the passive DN07 protocols (`protocol::passive_shamir`) on the
//! deterministic simulator: `Random`, `Double-Random`, the batched open, and triple generation.
//!
//! The parameters are `n = 5`, `t = 2`, satisfying DN07's `n >= 2t + 1`; each run of `Random` /
//! `Double-Random` therefore yields `n - t = 3` outputs.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::{simulation::channel::SimpleNetworkConfig, PartyId},
    prelude::{simulate, Error, GeneralEnv, Protocol, RandEnvironment},
    protocol::{
        passive_shamir::{
            double_rand_share::PassiveRandDoubleShr,
            mul::PassiveShamirMul,
            rand_share::PassiveRandShr,
            triple::{PassiveTriple, ShamirTriple},
        },
        ProtocolId,
    },
    ss::{shamir::ShamirSS, LinearShare},
};

type F = Mersenne61;
type Share = ShamirSS<1, F>;

const N: usize = 5;
const T: usize = 2;

fn parties() -> Vec<PartyId> {
    (0..N).map(PartyId::from).collect()
}

/// Reconstructs the secret from every party's share.
fn open(shares: Vec<Share>) -> F {
    <Share as LinearShare>::secret_from_shares(&shares, &parties()).unwrap()
}

/// Gathers the shares that each party holds for output `k`, in party order, and reconstructs.
fn open_kth<T, G>(outputs: &std::collections::HashMap<PartyId, Vec<T>>, k: usize, get: G) -> F
where
    G: Fn(&T) -> Share,
{
    open(parties().iter().map(|p| get(&outputs[p][k])).collect())
}

/// Generates `n - t` triples: two runs of `Random` for `[a]` and `[b]`, one of `Double-Random` for
/// the masking randomness, then triple generation.
struct GenTriples {
    king: PartyId,
}

impl<E: RandEnvironment> Protocol<E> for GenTriples {
    // `ShamirTriple` is deliberately one-shot (not `Clone`), like `DoubleShare`, so the shares are
    // handed back destructured.
    type Output = Vec<(Share, Share, Share)>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let a = PassiveRandShr::<1, F>::new(T, parties())?.run(env).await?;
        let b = PassiveRandShr::<1, F>::new(T, parties())?.run(env).await?;
        let doubles = PassiveRandDoubleShr::<1, F>::new(T, parties())?
            .run(env)
            .await?;

        let triples: Vec<ShamirTriple<1, F>> =
            PassiveTriple::new(self.king, parties(), a, b, doubles)?
                .run(env)
                .await?;

        Ok(triples.into_iter().map(ShamirTriple::into_parts).collect())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("GenTriples")
    }
}

/// The defining property of a multiplication triple: the third sharing opens to the product of the
/// first two. This exercises the whole DN07 stack at once — in particular the extraction matrix
/// (which must be a *transposed* Vandermonde) and the batched open in both directions.
#[test]
fn triples_satisfy_c_equals_a_times_b() {
    let all = parties();
    let king = all[0];

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        all.clone(),
        |_| GenTriples { king },
        |_, net| GeneralEnv::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    let n_triples = outcome.outputs[&all[0]].len();
    assert_eq!(n_triples, N - T, "a run should yield n - t triples");

    for k in 0..n_triples {
        let a = open_kth(&outcome.outputs, k, |t| t.0.clone());
        let b = open_kth(&outcome.outputs, k, |t| t.1.clone());
        let c = open_kth(&outcome.outputs, k, |t| t.2.clone());

        assert_eq!(c, a * &b, "triple {k}: c != a * b");
        // Guard against the whole thing degenerating to zeros and passing vacuously.
        assert_ne!(a, F::from(0u64));
        assert_ne!(b, F::from(0u64));
    }
}

/// Runs `Double-Random` and hands back both halves of each double sharing.
struct GenDoubles;

impl<E: RandEnvironment> Protocol<E> for GenDoubles {
    type Output = Vec<(Share, Share)>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let doubles = PassiveRandDoubleShr::<1, F>::new(T, parties())?
            .run(env)
            .await?;
        Ok(doubles.into_iter().map(|d| d.into_parts()).collect())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("GenDoubles")
    }
}

/// The point of a double sharing: the degree-`t` and degree-`2t` halves hide the *same* secret. If
/// the extraction matrix were applied inconsistently across parties, or the two halves were dealt at
/// the wrong degrees, the two reconstructions would diverge.
#[test]
fn double_random_halves_hide_the_same_secret() {
    let all = parties();

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        all.clone(),
        |_| GenDoubles,
        |_, net| GeneralEnv::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    let n_doubles = outcome.outputs[&all[0]].len();
    assert_eq!(n_doubles, N - T);

    for k in 0..n_doubles {
        let share_t = open_kth(&outcome.outputs, k, |d| d.0.clone());
        let share_2t = open_kth(&outcome.outputs, k, |d| d.1.clone());

        assert_eq!(share_t, share_2t, "double sharing {k}: halves disagree");
        assert_eq!(outcome.outputs[&all[0]][k].0.degree(), T);
        assert_eq!(outcome.outputs[&all[0]][k].1.degree(), 2 * T);
    }
}

/// `Double-Random` needs `n >= 2t + 1`: with `2t` shares or fewer, the degree-`2t` half could never
/// be opened.
#[test]
fn double_random_rejects_too_few_parties() {
    let four: Vec<PartyId> = (0..4).map(PartyId::from).collect();
    // 2t = 4, so four parties are one short.
    assert!(PassiveRandDoubleShr::<1, F>::new(T, four.clone()).is_err());
    // Five parties satisfy n >= 2t + 1.
    assert!(PassiveRandDoubleShr::<1, F>::new(T, parties()).is_ok());
}

/// `Random` needs a degree below the party count, or nothing could ever be reconstructed.
#[test]
fn random_rejects_degree_at_or_above_party_count() {
    assert!(PassiveRandShr::<1, F>::new(N, parties()).is_err());
    assert!(PassiveRandShr::<1, F>::new(N - 1, parties()).is_ok());
}

/// Multiplies two vectors of random sharings with Beaver, returning `[x]`, `[y]` and `[x · y]` so
/// the caller can check the product against the factors.
struct MulRandomPairs {
    king: PartyId,
}

impl<E: RandEnvironment> Protocol<E> for MulRandomPairs {
    type Output = Vec<(Share, Share, Share)>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        // The factors: sharings of values nobody knows, so no party could shortcut the product.
        let x = PassiveRandShr::<1, F>::new(T, parties())?.run(env).await?;
        let y = PassiveRandShr::<1, F>::new(T, parties())?.run(env).await?;

        // The triples to spend, from a second, independent preprocessing pass.
        let a = PassiveRandShr::<1, F>::new(T, parties())?.run(env).await?;
        let b = PassiveRandShr::<1, F>::new(T, parties())?.run(env).await?;
        let doubles = PassiveRandDoubleShr::<1, F>::new(T, parties())?
            .run(env)
            .await?;
        let triples = PassiveTriple::new(self.king, parties(), a, b, doubles)?
            .run(env)
            .await?;

        let products = PassiveShamirMul::new(self.king, parties(), x.clone(), y.clone(), triples)?
            .run(env)
            .await?;

        Ok(x.into_iter()
            .zip(y)
            .zip(products)
            .map(|((x, y), product)| (x, y, product))
            .collect())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("MulRandomPairs")
    }
}

/// Beaver multiplication returns a sharing of the product, still at degree `t` so it can feed the
/// next layer of a circuit.
#[test]
fn beaver_multiplication_shares_the_product() {
    let all = parties();

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        all.clone(),
        |_| MulRandomPairs { king: all[0] },
        |_, net| GeneralEnv::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    let n_products = outcome.outputs[&all[0]].len();
    assert_eq!(n_products, N - T);

    for k in 0..n_products {
        let x = open_kth(&outcome.outputs, k, |p| p.0.clone());
        let y = open_kth(&outcome.outputs, k, |p| p.1.clone());
        let product = open_kth(&outcome.outputs, k, |p| p.2.clone());

        assert_eq!(product, x * &y, "product {k}: [x · y] != x · y");
        assert_ne!(x, F::from(0u64));
        assert_ne!(y, F::from(0u64));
        // The product must come back at degree t, or it could not feed another multiplication.
        assert_eq!(outcome.outputs[&all[0]][k].2.degree(), T);
    }
}

/// Beaver opens only degree-`t` values, so — unlike triple *generation* — it does not need
/// `n >= 2t + 1`. What it does need is a degree below the party count.
#[test]
fn beaver_rejects_degree_at_or_above_party_count() {
    let share = Share::new(F::from(1u64), N);
    let triple = ShamirTriple::new(share.clone(), share.clone(), share.clone());
    assert!(PassiveShamirMul::<1, F>::new(
        parties()[0],
        parties(),
        vec![share.clone()],
        vec![share],
        vec![triple],
    )
    .is_err());
}
