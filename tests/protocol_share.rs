//! End-to-end tests for the passive-adversary deal/open protocols in `protocol::share`, run on the
//! deterministic simulator over both built-in `LinearShare` schemes (additive and Shamir). The
//! party ids are the usual `0`-based network ids; Shamir's `encode_party` shifts them to the field
//! points `i + 1`, keeping the point `0` reserved for the secret.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::{simulation::channel::SimpleNetworkConfig, PartyId},
    prelude::{simulate, Abbreviate, Error, GeneralEnvironment, Protocol, RandEnvironment},
    protocol::{
        share::{
            deal::PassiveDealShr,
            open::{PassiveOpenShr, PassiveOpenToParty},
        },
        ProtocolId,
    },
    ss::{additive::AdditiveSS, shamir::ShamirSS, LinearShare},
};

/// The number of parties in the test network.
const N_PARTIES: usize = 3;

/// The party ids of the test network.
fn parties() -> Vec<PartyId> {
    (0..N_PARTIES).map(PartyId::from).collect()
}

/// Builds the `PassiveDealShr` instance for `pid`: the dealer's (carrying `secret` and the
/// scheme's reconstruction `threshold`) if `pid` is the dealer, a receiver's otherwise.
fn deal_for<S>(
    pid: PartyId,
    dealer: PartyId,
    secret: S::Value,
    threshold: S::Threshold,
) -> PassiveDealShr<S>
where
    S: LinearShare,
{
    if pid == dealer {
        PassiveDealShr::dealer(dealer, secret, parties(), threshold)
    } else {
        PassiveDealShr::receiver(dealer)
    }
}

/// The Shamir threshold used by these tests: dealt at degree 1, so any 2 of the 3 shares would
/// reconstruct — deliberately below full threshold to exercise the caller-chosen degree.
const SHAMIR_DEGREE: usize = 1;

/// Composed test protocol: receive a share from the dealer, then open the secret to everyone.
struct DealThenOpen<S>
where
    S: LinearShare,
{
    /// This party's side of the deal: the dealer's instance or a receiver's, from `deal_for`.
    deal: PassiveDealShr<S>,
}

impl<S, E> Protocol<E> for DealThenOpen<S>
where
    S: LinearShare + Abbreviate,
    E: RandEnvironment,
    S::Value: Send + Sync + 'static,
{
    type Output = S::Value;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let share: S = self.deal.execute(env).await?;
        PassiveOpenShr::new(share).execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("DealThenOpen")
    }
}

/// Deals a secret from party 0 and opens it to everyone; every party must output the secret.
fn deal_then_open_roundtrip<S>(threshold: S::Threshold)
where
    S: LinearShare<Value = Mersenne61> + Abbreviate + 'static,
{
    let parties = parties();
    let dealer = parties[0];
    let secret = Mersenne61::from(123_456_789u64);

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties.clone(),
        |pid| DealThenOpen::<S> {
            deal: deal_for(pid, dealer, secret, threshold),
        },
        |_, net| GeneralEnvironment::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    for party in &parties {
        assert_eq!(outcome.outputs[party], secret);
    }
}

/// An additive sharing dealt by party 0 opens to the dealt secret at every party.
#[test]
fn additive_deal_then_open_roundtrip() {
    deal_then_open_roundtrip::<AdditiveSS<Mersenne61>>(());
}

/// A Shamir sharing of degree `SHAMIR_DEGREE` dealt by party 0 opens to the dealt secret at every
/// party.
#[test]
fn shamir_deal_then_open_roundtrip() {
    deal_then_open_roundtrip::<ShamirSS<1, Mersenne61>>(SHAMIR_DEGREE);
}

/// Composed test protocol: receive a share of `x` from the dealer, locally compute the affine map
/// `a * [x] + b` (communication-free, the point of `LinearShare`), then open the result.
struct DealAffineOpen<S>
where
    S: LinearShare,
{
    /// This party's side of the deal of `x`: the dealer's instance or a receiver's.
    deal: PassiveDealShr<S>,
    /// The public factor that multiplies the share.
    a: S::Value,
    /// The public constant added after the multiplication.
    b: S::Value,
}

impl<S, E> Protocol<E> for DealAffineOpen<S>
where
    S: LinearShare + Abbreviate,
    E: RandEnvironment,
    S::Value: Send + Sync + 'static,
{
    type Output = S::Value;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let share: S = self.deal.execute(env).await?;
        let affine_share = share * &self.a + &self.b;
        PassiveOpenShr::new(affine_share).execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("DealAffineOpen")
    }
}

/// Deals `x`, has every party locally compute `a·[x] + b`, opens, and checks the result is
/// `a * x + b` — exercising the local `LinearShare` operators through the interactive protocols.
fn deal_affine_open<S>(threshold: S::Threshold)
where
    S: LinearShare<Value = Mersenne61> + Abbreviate + 'static,
{
    let parties = parties();
    let dealer = parties[0];
    let secret = Mersenne61::from(424_242u64);
    let a = Mersenne61::from(7u64);
    let b = Mersenne61::from(13u64);

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties.clone(),
        |pid| DealAffineOpen::<S> {
            deal: deal_for(pid, dealer, secret, threshold),
            a,
            b,
        },
        |_, net| GeneralEnvironment::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    let expected = secret * &a + &b;
    for party in &parties {
        assert_eq!(outcome.outputs[party], expected);
    }
}

/// Under additive sharing, the local `a * [x] + b` opens to `a * x + b` at every party.
#[test]
fn additive_deal_affine_open() {
    deal_affine_open::<AdditiveSS<Mersenne61>>(());
}

/// Under Shamir sharing of degree `SHAMIR_DEGREE`, the local `a * [x] + b` opens to `a * x + b`
/// at every party.
#[test]
fn shamir_deal_affine_open() {
    deal_affine_open::<ShamirSS<1, Mersenne61>>(SHAMIR_DEGREE);
}

/// Composed test protocol: receive a share from the dealer, then open the secret towards a single
/// designated receiver.
struct DealThenOpenTo<S>
where
    S: LinearShare,
{
    /// This party's side of the deal: the dealer's instance or a receiver's, from `deal_for`.
    deal: PassiveDealShr<S>,
    /// The only party that learns the secret.
    receiver: PartyId,
}

impl<S, E> Protocol<E> for DealThenOpenTo<S>
where
    S: LinearShare + Abbreviate,
    E: RandEnvironment,
    S::Value: Send + Sync + 'static,
{
    type Output = Option<S::Value>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let share: S = self.deal.execute(env).await?;
        PassiveOpenToParty::new(self.receiver, share)
            .execute(env)
            .await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("DealThenOpenTo")
    }
}

/// Deals a secret and opens it towards a single party (not the dealer): only the receiver's output
/// is `Some(secret)`; every other party — the dealer included — outputs `None`.
fn deal_then_open_to_party<S>(threshold: S::Threshold)
where
    S: LinearShare<Value = Mersenne61> + Abbreviate + 'static,
{
    let parties = parties();
    let dealer = parties[0];
    let receiver = parties[1];
    let secret = Mersenne61::from(31_337u64);

    let outcome = simulate(
        SimpleNetworkConfig::default(),
        parties.clone(),
        |pid| DealThenOpenTo::<S> {
            deal: deal_for(pid, dealer, secret, threshold),
            receiver,
        },
        |_, net| GeneralEnvironment::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    );

    for party in &parties {
        let expected = if *party == receiver {
            Some(secret)
        } else {
            None
        };
        assert_eq!(outcome.outputs[party], expected);
    }
}

/// Under additive sharing, opening towards party 1 gives it the secret and every other party,
/// the dealer included, `None`.
#[test]
fn additive_deal_then_open_to_party() {
    deal_then_open_to_party::<AdditiveSS<Mersenne61>>(());
}

/// Under Shamir sharing of degree `SHAMIR_DEGREE`, opening towards party 1 gives it the secret and
/// every other party, the dealer included, `None`.
#[test]
fn shamir_deal_then_open_to_party() {
    deal_then_open_to_party::<ShamirSS<1, Mersenne61>>(SHAMIR_DEGREE);
}
