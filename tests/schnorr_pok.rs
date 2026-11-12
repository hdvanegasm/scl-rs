//! End-to-end tests for `protocol::zk::schnorr::SchnorrPok` on the deterministic simulator, run
//! inside a network with more parties than the two that take part in the proof.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::{
        ec::{secp256k1::Secp256k1, EllipticCurve},
        field::secp256k1_scalar::Secp256k1ScalarField,
    },
    net::{simulation::channel::SimpleNetworkConfig, PartyId},
    prelude::{simulate, Error, GeneralEnvironment, Protocol, RandEnvironment, Ring},
    protocol::{zk::schnorr::SchnorrPok, ProtocolId},
};

/// The number of parties in the test network, more than the two that take part in a proof.
const N_PARTIES: usize = 4;
/// The `LIMBS` parameter of `SchnorrPok` on secp256k1: `Secp256k1` implements `EllipticCurve<4>`.
const LIMBS: usize = 4;

/// A node of a Schnorr proof of knowledge of a discrete logarithm on secp256k1.
type Pok = SchnorrPok<LIMBS, Secp256k1>;

/// The party ids of the test network.
fn parties() -> Vec<PartyId> {
    (0..N_PARTIES).map(PartyId::from).collect()
}

/// Test wrapper standing in for a larger protocol: the parties handed a proof node run it, and the
/// rest take no part. Errors are turned into strings so the simulator, which panics on a protocol
/// error, reports them as outputs instead.
struct MaybeProve {
    /// This party's proof node, or `None` if it takes no part in the proof.
    pok: Option<Pok>,
}

impl<E> Protocol<E> for MaybeProve
where
    E: RandEnvironment,
{
    type Output = Result<Option<bool>, String>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        Ok(match self.pok {
            Some(pok) => pok.execute(env).await.map_err(|e| e.to_string()),
            None => Ok(None),
        })
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("MaybeProve")
    }
}

/// Runs `make_pok` for every party of the test network and returns each party's output.
fn run(
    make_pok: impl Fn(PartyId) -> Option<Pok>,
) -> std::collections::HashMap<PartyId, Result<Option<bool>, String>> {
    simulate(
        SimpleNetworkConfig::default(),
        parties(),
        |pid| MaybeProve { pok: make_pok(pid) },
        |_, net| GeneralEnvironment::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        vec![],
    )
    .outputs
}

/// A random witness and its statement `h = g^x`.
fn instance() -> (Secp256k1ScalarField, Secp256k1) {
    let mut rng = ChaCha20Rng::from_rng(&mut rand::rng());
    let x = Secp256k1ScalarField::random(&mut rng);
    (x, Secp256k1::gen().scalar_mul(&x))
}

/// Party 2 proves to party 0; parties 1 and 3 are bystanders.
#[test]
fn honest_proof_between_arbitrary_parties() {
    let (prover, verifier) = (PartyId::from(2), PartyId::from(0));
    let (x, h) = instance();

    let outputs = run(|pid| {
        if pid == prover {
            Some(Pok::prover(prover, verifier, x, h).unwrap())
        } else if pid == verifier {
            Some(Pok::verifier(prover, verifier, h).unwrap())
        } else {
            None
        }
    });

    assert_eq!(outputs[&verifier], Ok(Some(true)));
    assert_eq!(outputs[&prover], Ok(None));
    for bystander in [PartyId::from(1), PartyId::from(3)] {
        assert_eq!(outputs[&bystander], Ok(None));
    }
}

/// A prover holding the wrong witness is rejected.
#[test]
fn wrong_witness_is_rejected() {
    let (prover, verifier) = (PartyId::from(3), PartyId::from(1));
    let (_, h) = instance();
    let (wrong_x, _) = instance();

    let outputs = run(|pid| {
        if pid == prover {
            Some(Pok::prover(prover, verifier, wrong_x, h).unwrap())
        } else if pid == verifier {
            Some(Pok::verifier(prover, verifier, h).unwrap())
        } else {
            None
        }
    });

    assert_eq!(outputs[&verifier], Ok(Some(false)));
}

/// A party cannot be both prover and verifier.
#[test]
fn same_prover_and_verifier_is_rejected() {
    let (x, h) = instance();
    let party = PartyId::from(0);
    assert!(matches!(Pok::prover(party, party, x, h), Err(Error::Input)));
    assert!(matches!(Pok::verifier(party, party, h), Err(Error::Input)));
}

/// A node whose local party does not match its role fails with `Error::Input` before sending
/// anything, so the parties that do not run the proof are unaffected.
#[test]
fn role_mismatch_is_rejected() {
    let (prover, verifier) = (PartyId::from(0), PartyId::from(1));
    let (x, h) = instance();
    let input_error = Err(Error::Input.to_string());

    let outputs = run(|pid| match usize::from(pid) {
        // Party 1 is the prover of its own proof but was built without the witness.
        1 => Some(Pok::verifier(PartyId::from(1), PartyId::from(0), h).unwrap()),
        // Party 2 runs the proof although it is neither the prover nor the verifier.
        2 => Some(Pok::verifier(prover, verifier, h).unwrap()),
        // Party 3 is the verifier of its own proof but was built with the prover's constructor.
        3 => Some(Pok::prover(PartyId::from(0), PartyId::from(3), x, h).unwrap()),
        _ => None,
    });

    assert_eq!(outputs[&PartyId::from(0)], Ok(None));
    assert_eq!(outputs[&PartyId::from(1)], input_error);
    assert_eq!(outputs[&PartyId::from(2)], input_error);
    assert_eq!(outputs[&PartyId::from(3)], input_error);
}
