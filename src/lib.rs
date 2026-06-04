#![warn(missing_docs)]

//! **scl-rs** is a set of tools to quickly implement MPC protocols in a
//! distributed way. The main features of *scl-rs* are:
//!
//! - A functional network library that uses TLS (implemented with [`rustls`]) for communication between parties.
//! - A set of mathematical tools that are common to MPC protocols:
//!     - Finite fields.
//!     - Finite rings.
//!     - Polynomial over rings and their operations.
//!     - Polynomial interpolation over fields using Lagrange interpolation.
//!     - Matrices and vectors over rings.
//!     - Finite field implementations like Mersenne 61.
//!     - Elliptic curve implementations.
//! - A set of MPC facilities that are common to a wide variety of protocols:
//!     - Feldman verifiable secret-sharing.
//!     - Shamir secret-sharing.
//!     - Additive secret sharing.
//!
//! Also, **scl-rs** offers a simulator based on discrete event simulation to emulate protocol
//! execution without the need of spawning remote nodes and configuring any network layer. This
//! simulation tool allows you to set your preferred network parameters like RTT, bandwidth,
//! package loss, among others, and simulate the execution of the protocol with results that are
//! near to a real remote execution with the desired parameters. For more information about the
//! simulation, you can refer to the documentation in [`crate::net::simulation`].
//!
//! # Defining protocols
//!
//! Protocols are defined by implementing the trait [`Protocol`]. The following example shows an
//! implementation of a two-party protocol that sends a message with the identifier of the party and expects
//! the identifier from the other party:
//!
//! ```ignore
//! use scl_rs::net::simulation::channel::SimpleNetworkConfig;
//! use scl_rs::net::simulation::network::SimulatedNetwork;
//! use scl_rs::protocol::{Environment, Protocol, ProtocolResult};
//! use scl_rs::net::Packet;
//!
//! // Defines a new struct for the protocol.
//! pub struct SendRecvProtocol;
//!
//! #[async_trait::async_trait]
//! impl Protocol<SimulatedNetwork<SimpleNetworkConfig>> for SendRecvProtocol {
//!     // The run method specifies the behavior of the protocol.
//!     async fn run(
//!         &self,
//!         environment: &mut Environment<SimulatedNetwork<SimpleNetworkConfig>>,
//!     ) -> ProtocolResult<SimulatedNetwork<SimpleNetworkConfig>> {
//!         // Create a new packet to send information. All the information must be sent using
//!         // packets. Packet can store multiple elements with different types as long as the
//!         // elements are serializable.
//!         let mut packet = Packet::empty();
//!         packet
//!             .write(&environment.network.local_party().as_usize())
//!             .unwrap();
//!         
//!         // Obtains the ID of the other party and send the packet through the network. The
//!         // network can be accessed through the environment. Also, the network contains all other
//!         // parties in the case of a multiparty protocol.
//!         let other = environment.network.other().unwrap();
//!         environment.network.send_to(other, &packet).await.unwrap();
//!
//!         // Waits to receive the packet from the other party.
//!         let received_packet = environment.network.recv_from(other).await.unwrap();
//!         environment.network.close().await.unwrap();
//!
//!         // Returns the result of the protocol.
//!         ProtocolResult::with_result_only(received_packet.bytes())
//!     }
//!
//!     fn name(&self) -> String {
//!         String::from("SendRecvProtocol")
//!    }
//! }
//! ```
//!
//! # Distributed execution
//!
//! **scl-rs** uses TLS to implement point-to-point channels between parties. To set up a network
//! you need to have proper certificates for each party. Those certificates will be used to
//! secure each channel.
//!
//! We will show how to run the previous protocol in a distributed network
//! locally. We wrote a script to generate self-signed local host certificates for any number of
//! parties. To create the certificates with this script you can run
//!
//! ```bash
//! bash gen_self_signed_certs.sh <n_parties>
//! ```
//! After running the scripts, the certificates will be stored in the `certs/` folder.
//!
//! Then, to execute the protocol you need to load the network configuration from a JSON file. The
//! following code snippet shows a example for a JSON file configuration for the party with ID 0:
//!
//! ```json
//! {
//!   "base_port": 5000,
//!   "timeout": 5000,
//!   "sleep_time": 500,
//!   "peer_ips": ["127.0.0.1", "127.0.0.1", "127.0.0.1"],
//!   "server_cert": "./certs/server_cert_p0.crt",
//!   "priv_key": "./certs/priv_key_p0.pem",
//!   "trusted_certs": ["./certs/rootCA.crt"]
//! }
//! ```
//!
//! The fields above are explained next:
//!
//! - The `base_port`, is the port that will be used as a base to compute the actual
//!   port in which the party will be listening to. For a party with index `i`, the
//!   listening port is `base_port + i`.
//! - The `timeout` is the number of milliseconds a party will repeatedly try to
//!   connect with another party. If the timeout is reached, the application returns
//!   an error.
//! - The `sleep_time` is the number of milliseconds that a party will wait before
//!   trying to connect again with another party in case the connection is not
//!   successful.
//! - The `peer_ips` is the list of IPs for all the peers engaged in the protocol.
//!   In this case, the array is specified in such a way that the party with index
//!   `i` has IP `peer_ips[i]`.
//! - The `server_cert` is the certificate path for that node for secure communication.
//! - The `priv_key` is the file with the private key associated with the
//!   certificate in `server_cert`. This private key is used for secure communication.
//! - `trusted_certs` is a list of paths with trusted CA certificates. This is useful
//!   in executions where the certificates are self-signed.
//!
//! Once you generated the JSON configuration file, you can implement the node as follows:
//!
//! ```ignore
//! use scl_rs::net::{NetworkConfig, NetworkError};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>>  {
//!     let current_id = 0;
//!     let network_config = NetworkConfig::new("file_paty_net_config.json");
//!     let network = TcpNetwork::create(current_id, network_config)?;
//!
//!     let mut env = Environment::new(network);
//!
//!     let protocol = SendRecvProtocol;
//!     let result = protocol.run(&mut env).await;
//!     println("Result of the protocol for party {current_id}: {:?}", result);
//! }
//! ```
//!
//! # Acknowledgements
//!
//! Thanks to [HashCloak Inc.](https://hashcloak.com/) for allowing Hernán Vanegas to make progress
//! on this tool as part of an internal learning initiative.

/// Mathematical tools used in MPC protocols.
pub mod math;

/// Network facilities and methods that allow a set of parties
/// to connect between them using TLS.
pub mod net;

/// Implementation of some tools commonly used in MPC protocols
/// based on secret-sharing techniques.
pub mod ss;

/// Traits and structs to write and run protocols and manage their results.
pub mod protocol;

/// Re-export of the [`Protocol`] trait used to define MPC protocols.
pub use protocol::Protocol;
