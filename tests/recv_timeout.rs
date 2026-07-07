//! Temporary verification for recv_from_with_timeout — delete after review.
use scl_rs::net::simulation::channel::SimpleNetworkConfig;
use scl_rs::net::simulation::simulator::simulate;
use scl_rs::net::{Network, Packet, PartyId};
use scl_rs::protocol::{Environment, Error, GeneralEnv, Protocol};
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
            Ok(res.is_err())
        } else {
            environment.network_mut().close().await?;
            Ok(true)
        }
    }

    fn name(&self) -> &'static str {
        "SilentPartyProtocol"
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

    fn name(&self) -> &'static str {
        "PromptSenderProtocol"
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
