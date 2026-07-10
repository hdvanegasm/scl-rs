//! This example shows how to send a heterogeneous [`Packet`] between two parties. Each party builds
//! a packet carrying several *different* element types — a scalar field element, an elliptic-curve
//! point, and a vector of additive shares — sends it to the other party, and reads the elements
//! back out. It also demonstrates the labeled writes
//! ([`Packet::write_labeled`]/[`Packet::write_many_labeled`]) that annotate the simulator trace with
//! each element's type.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    math::{ec::secp256k1::Secp256k1, field::secp256k1_scalar::Secp256k1ScalarField},
    net::{simulation::channel::SimpleNetworkConfig, Network, Packet, PartyId},
    prelude::{simulate, EllipticCurve, Environment, Error, Protocol, Ring},
    protocol::{GeneralEnv, ProtocolId},
    ss::additive::AdditiveSS,
};

pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SendRecvProtocol {
    type Output = (
        Secp256k1ScalarField,
        Secp256k1,
        Vec<AdditiveSS<Secp256k1ScalarField>>,
    );

    async fn run(self, environment: &mut E) -> Result<Self::Output, Error> {
        let mut packet = Packet::empty();

        let mut rng = ChaCha20Rng::from_rng(&mut rand::rng());
        let rnd_scalar = Secp256k1ScalarField::random(&mut rng);
        let rnd_ec = Secp256k1::gen().scalar_mul(&rnd_scalar);
        let shares = AdditiveSS::shares_from_secret(
            rnd_scalar,
            &[PartyId::from(0), PartyId::from(1)],
            &mut rng,
        );

        packet.write_labeled(&rnd_scalar)?;
        packet.write_labeled(&rnd_ec)?;
        packet.write_many_labeled(&shares)?;

        let other = environment.network().other()?;
        environment.network_mut().send_to(other, &packet).await?;

        let mut received_packet = environment.network_mut().recv_from(other).await?;

        environment.network_mut().close().await?;

        let mut recv_shares: Vec<AdditiveSS<Secp256k1ScalarField>> = Vec::with_capacity(2);
        for _ in 0..2 {
            recv_shares.push(received_packet.pop()?);
        }
        let ec: Secp256k1 = received_packet.pop()?;
        let scalar: Secp256k1ScalarField = received_packet.pop()?;

        Ok((scalar, ec, recv_shares))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SendRecvProtocol")
    }
}

fn main() {
    let p0 = PartyId::from(0);
    let p1 = PartyId::from(1);

    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| SendRecvProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // Once the protocol finishes, you can access the protocol traces and the outputs for each party.
    println!("=== P0 trace: ===\n{}", outcome.traces[&p0]);
    println!("=== P1 trace: ===\n{}", outcome.traces[&p1]);
}
