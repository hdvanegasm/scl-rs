use scl_rs::net::simulation::channel::SimpleNetworkConfig;
use scl_rs::net::simulation::simulator::simulate;
use scl_rs::net::{Network, Packet, PartyId};
use scl_rs::protocol::{Environment, Error, GeneralEnv, Protocol, ProtocolId};
use std::time::Duration;

/// P0 waits with a timeout; P1 never sends. The recv must resolve to a Timeout error.
pub struct SilentPartyProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SilentPartyProtocol {
    type Output = bool;

    async fn run(self, environment: &mut E) -> Result<bool, Error> {
        let me = environment.network().local_party();
        if me.as_usize() == 0 {
            let other = environment.network().other()?;
            let res = environment
                .network_mut()
                .recv_from_with_timeout(other, Duration::from_millis(100))
                .await;
            environment.network_mut().close().await?;
            Ok(matches!(res, Err(scl_rs::net::NetworkError::Timeout(Some(p))) if p == other))
        } else {
            environment.network_mut().close().await?;
            Ok(true)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SilentPartyProtocol")
    }
}

#[test]
fn silent_party_times_out() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| SilentPartyProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );
    assert!(outcome.outputs[&p0], "expected a timeout error for P0");
}

/// P1 sends promptly; the timed recv must succeed well before the deadline.
pub struct PromptSenderProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for PromptSenderProtocol {
    type Output = bool;

    async fn run(self, environment: &mut E) -> Result<bool, Error> {
        let me = environment.network().local_party();
        let other = environment.network().other()?;
        if me.as_usize() == 0 {
            let res = environment
                .network_mut()
                .recv_from_with_timeout(other, Duration::from_secs(60))
                .await;
            environment.network_mut().close().await?;
            Ok(res.is_ok())
        } else {
            let mut packet = Packet::empty();
            packet.write(&42_usize).unwrap();
            environment.network_mut().send_to(other, &packet).await?;
            environment.network_mut().close().await?;
            Ok(true)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PromptSenderProtocol")
    }
}

#[test]
fn prompt_sender_succeeds() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| PromptSenderProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );
    assert!(outcome.outputs[&p0], "expected P0 to receive the packet");
}

/// P0 waits on *any* party with a timeout; every peer stays silent. The receive must resolve to
/// `Timeout(None)`: no single party is identifiable as the culprit.
pub struct AllSilentProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for AllSilentProtocol {
    type Output = bool;

    async fn run(self, environment: &mut E) -> Result<bool, Error> {
        let me = environment.network().local_party();
        if me.as_usize() == 0 {
            let res = environment
                .network_mut()
                .recv_any_with_timeout(Duration::from_millis(100))
                .await;
            environment.network_mut().close().await?;
            Ok(matches!(res, Err(scl_rs::net::NetworkError::Timeout(None))))
        } else {
            environment.network_mut().close().await?;
            Ok(true)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("AllSilentProtocol")
    }
}

#[test]
fn recv_any_all_silent_times_out() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| AllSilentProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );
    assert!(outcome.outputs[&p0], "expected Timeout(None) for P0");
}

/// P1 sends, but the link delay (tens of milliseconds of virtual time under
/// `SimpleNetworkConfig`) exceeds P0's 1 ms timeout. The receive must resolve to `Timeout(None)`
/// instead of returning the packet past the deadline; the late packet stays queued.
pub struct LateSenderProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for LateSenderProtocol {
    type Output = bool;

    async fn run(self, environment: &mut E) -> Result<bool, Error> {
        let me = environment.network().local_party();
        let other = environment.network().other()?;
        if me.as_usize() == 0 {
            let res = environment
                .network_mut()
                .recv_any_with_timeout(Duration::from_millis(1))
                .await;
            let timed_out = matches!(res, Err(scl_rs::net::NetworkError::Timeout(None)));
            environment.network_mut().close().await?;
            Ok(timed_out)
        } else {
            let mut packet = Packet::empty();
            packet.write(&42_usize).unwrap();
            environment.network_mut().send_to(other, &packet).await?;
            environment.network_mut().close().await?;
            Ok(true)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("LateSenderProtocol")
    }
}

#[test]
fn recv_any_late_packet_times_out() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| LateSenderProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );
    assert!(
        outcome.outputs[&p0],
        "expected Timeout(None), got the late packet instead"
    );
}

/// P1 sends promptly and the deadline is generous; the receive must succeed and report P1 as the
/// sender.
pub struct PromptAnyProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for PromptAnyProtocol {
    type Output = bool;

    async fn run(self, environment: &mut E) -> Result<bool, Error> {
        let me = environment.network().local_party();
        let other = environment.network().other()?;
        if me.as_usize() == 0 {
            let res = environment
                .network_mut()
                .recv_any_with_timeout(Duration::from_secs(60))
                .await;
            environment.network_mut().close().await?;
            Ok(matches!(res, Ok((sender, _)) if sender == other))
        } else {
            let mut packet = Packet::empty();
            packet.write(&42_usize).unwrap();
            environment.network_mut().send_to(other, &packet).await?;
            environment.network_mut().close().await?;
            Ok(true)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PromptAnyProtocol")
    }
}

#[test]
fn recv_any_prompt_sender_succeeds() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| PromptAnyProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );
    assert!(outcome.outputs[&p0], "expected P0 to receive the packet");
}
