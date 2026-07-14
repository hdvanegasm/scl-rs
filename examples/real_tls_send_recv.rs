//! Real two-party TLS deployment of `SendRecvProtocol`.
//!
//! This is the real-network counterpart of `examples/simple_send_recv.rs`: the **same** protocol
//! logic (see that file for the line-by-line explanation), but instead of the deterministic
//! simulator it runs over a real, mutually authenticated TLS (mTLS) connection between two separate
//! OS processes. This is the headline property of scl-rs — a protocol written generic over
//! `E: Environment` runs unchanged on either backend; only `main` differs (it builds a `TcpNetwork`
//! from a configuration file instead of calling `simulate`).
//!
//! # Running it
//!
//! Unlike the simulator examples, this one launches **two processes**, each passing its own party id
//! and configuration file. The two configuration files are committed alongside this example —
//! `examples/config_p0.json` and `examples/config_p1.json` — so the only thing you generate locally
//! is the TLS material they reference. Run every command from the **crate root**: the paths inside
//! the config files (e.g. `./certs/...`) are resolved relative to the working directory.
//!
//! 1. Generate the self-signed mTLS material for two parties (writes a root CA plus a certificate
//!    and key per party into `./certs`):
//!
//!    ```text
//!    bash gen_self_signed_certs.sh 2
//!    ```
//!
//! 2. In two terminals, start both parties, pointing each at its committed config file (`create`
//!    blocks until every peer has connected, so both processes must be running):
//!
//!    ```text
//!    cargo run --example real_tls_send_recv -- 0 examples/config_p0.json
//!    cargo run --example real_tls_send_recv -- 1 examples/config_p1.json
//!    ```
//!
//! Each party sends its own id and prints the id it received from the other, so party 0 reports it
//! received id `1` and party 1 reports it received id `0`.
//!
//! # The configuration files
//!
//! Both configs describe a local two-party loopback deployment: `base_port` 5000 (party `i` listens
//! on `5000 + i`), both `peer_ips` set to `127.0.0.1`, and a shared `trusted_certs` root CA (the one
//! produced by the script above). They differ only in `server_cert`/`priv_key`, which point to each
//! party's own leaf certificate and key. See the README's "Network configuration" section for the
//! meaning of every field, and "Generating certificates" for what the script emits.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::path::Path;

use scl_rs::{
    net::{Network, NetworkConfig, Packet, TcpNetwork},
    prelude::{Environment, Error, GeneralEnv, Protocol},
    protocol::ProtocolId,
};

/// This protocol and its implementation of the [`Protocol`] trait are fully explained in
/// `examples/simple_send_recv.rs`. We refer to the reader to this file to see all the details about
/// this protocol implementation.
pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SendRecvProtocol {
    type Output = usize;

    async fn run(self, environment: &mut E) -> Result<usize, Error> {
        let mut packet = Packet::empty();

        packet.write(&environment.network().local_party().as_usize())?;
        let other = environment.network().other()?;
        environment.network_mut().send_to(other, &packet).await?;

        let received_packet = environment.network_mut().recv_from(other).await?;
        environment.network_mut().close().await?;
        let their_id: usize = received_packet.read(0)?;
        Ok(their_id)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SendRecvProtocol")
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read this node's party id and configuration-file path from the command line:
    // cargo run --example real_tls_send_recv -- <my_id> <config_path>
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: {} <my_id> <config_path>", args[0]);
        std::process::exit(1);
    }

    let my_id: usize = args[1]
        .parse()
        .expect("<my_id> must be a non-negative integer");
    let config_path = &args[2];

    let config = NetworkConfig::new(Path::new(config_path))?;

    let network = TcpNetwork::create(my_id, config).await?;
    let mut env = GeneralEnv::new(network, ChaCha20Rng::from_rng(&mut rand::rng()));
    let their_id = SendRecvProtocol.execute(&mut env).await?;
    println!("Party {my_id} received id {their_id} from the other party");

    Ok(())
}
