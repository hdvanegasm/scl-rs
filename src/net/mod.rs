/// Implements a channel using for point-to-point communication between two nodes.
pub mod channel;

/// Implementation of a simulated network.
///
/// This simulation uses theoretical formulas to simulate network delays. In this simulation, the
/// user of the library can tweak the parameters of the network, and the protocol execution will
/// report a time close to a real execution.
pub mod simulation;

use crate::net::channel::{Channel, ChannelError};
use async_trait::async_trait;
use channel::LoopBackChannel;
use postcard::from_bytes;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::{
    cmp::Ordering,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    time::Duration,
};
use std::{
    fs,
    io::{self, ErrorKind},
};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::server::{VerifierBuilderError, WebPkiClientVerifier};
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};

/// Represents a party ID in the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Hash, PartialOrd, Eq, Default)]
pub struct PartyId(usize);

impl From<PartyId> for usize {
    fn from(id: PartyId) -> Self {
        id.0
    }
}

impl From<usize> for PartyId {
    fn from(id: usize) -> Self {
        Self(id)
    }
}

impl PartyId {
    /// Returns the party ID as a [`usize`].
    pub fn as_usize(&self) -> usize {
        self.0
    }
}

/// Error type for network errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NetworkError {
    /// Encapsulates a TLS error.
    #[error("TLS error: {0:?}")]
    TlsError(#[from] tokio_rustls::rustls::Error),
    /// This error is returned when there is an IO error.
    #[error("IO error: {0:?}")]
    IoError(#[from] io::Error),
    /// This error encapsulates a channel error.
    #[error("channel error: {0:?}")]
    ChannelError(#[from] ChannelError),
    /// Encapsulates a serialization error.
    #[error("error during the serialization: {0:?}")]
    SerializationError(#[from] postcard::Error),
    /// The party was not found in a collection of [`PartyId`].
    #[error("party not found: {0:?}")]
    PartyNotFound(PartyId),
    /// Error returned when the execution expects a two-party protocol.
    #[error("expected a network with only two nodes, there are {0} nodes in the network")]
    ExpectedTwoNodeNet(usize),
    /// The certificate verifier builder fails.
    #[error("building for the verifier of certificates failed: {0:?}")]
    VerifierBuilderError(#[from] VerifierBuilderError),
    /// The requested operation is not yet supported by this network backend.
    #[error("unsupported functionality: {0:}")]
    Unsupported(&'static str),
    /// The packet is empty.
    #[error("the packet is empty")]
    EmptyPacket,
    /// The packet is accessed with wrong index.
    #[error("accessing wrong packet index: {idx}")]
    WrongPacketIdx {
        /// Wrong index.
        idx: usize,
    },
}

/// Special type for the network error.
pub type Result<T> = std::result::Result<T, NetworkError>;

/// Packet of information sent through a given channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Packet(Vec<Vec<u8>>);

impl PartialEq<Packet> for Arc<Packet> {
    fn eq(&self, other: &Packet) -> bool {
        self.0 == other.0
    }
}

impl Packet {
    /// Creates an empty [`Packet`].
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Creates a new [`Packet`] from a buffer.
    pub fn new(buffer: Vec<Vec<u8>>) -> Self {
        Self(buffer)
    }

    /// Returns the size of the [`Packet`].
    pub fn size(&self) -> usize {
        self.0
            .iter()
            .fold(0, |total_length, obj| total_length + obj.len())
    }

    /// Extract the last element added into the [`Packet`].
    pub fn pop<T>(&mut self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let bytes = self.0.pop().ok_or(NetworkError::EmptyPacket)?;
        let object = from_bytes(&bytes)?;
        Ok(object)
    }

    /// Read the element at the given index inside the [`Packet`].
    pub fn read<T>(&self, obj_idx: usize) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let bytes = self
            .0
            .get(obj_idx)
            .ok_or(NetworkError::WrongPacketIdx { idx: obj_idx })?;
        let object = postcard::from_bytes(bytes)?;
        Ok(object)
    }

    /// Write an element at the end of the packet.
    pub fn write<T>(&mut self, obj: &T) -> Result<()>
    where
        T: Serialize,
    {
        let bytes_obj = postcard::to_allocvec(obj)?;
        self.0.push(bytes_obj);
        Ok(())
    }

    /// Returns the bytes of the packet.
    pub fn bytes(&self) -> Vec<u8> {
        self.0.iter().fold(Vec::new(), |mut acc, obj| {
            acc.extend_from_slice(obj);
            acc
        })
    }
}

/// On-disk JSON representation of a [`NetworkConfig`].
///
/// This is the file shape that `serde` deserializes a configuration file into: it mirrors the JSON
/// one-to-one and stores all certificate material as filesystem *paths*. [`NetworkConfig::new`]
/// deserializes this type and then loads the referenced PEM files to build the runtime
/// [`NetworkConfig`]. Keeping it separate lets `serde` validate the structure and types (and report
/// precise errors), while the certificate file I/O stays in `new`.
///
/// `#[serde(deny_unknown_fields)]` turns an unrecognized key (for example a misspelled field name)
/// into a hard error instead of silently ignoring it.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct NetworkConfigFile {
    /// Base listening port. The party with index `i` listens on `base_port + i`.
    base_port: u16,
    /// Milliseconds a party keeps retrying to connect to a peer before giving up with an error.
    timeout: u64,
    /// Milliseconds a party waits between connection retries.
    sleep_time: u64,
    /// IP of every party, indexed by party id and **including this node**; its length is the number
    /// of parties.
    peer_ips: Vec<Ipv4Addr>,
    /// Path to this node's certificate (PEM). It is presented both as the TLS server certificate and
    /// as the client identity for mutual authentication.
    server_cert: PathBuf,
    /// Path to the private key (PEM) associated with `server_cert`.
    priv_key: PathBuf,
    /// Paths to the trusted CA certificates (PEM) used to verify peers (useful for self-signed
    /// certificates).
    trusted_certs: Vec<PathBuf>,
}

/// Configuration of the network.
pub struct NetworkConfig<'a> {
    /// Port that will be used as a base to define the port of each party. Party `i` will listen at
    /// port `base_port + i`.
    base_port: u16,
    /// Time a party keeps retrying to connect to a peer before giving up with an error.
    timeout: Duration,
    /// Time a party waits between connection retries.
    sleep_time: Duration,
    /// IPs of each peer.
    pub peer_ips: Vec<Ipv4Addr>,
    /// Trusted roots used to verify peer certificates in both roles: when this node dials a peer
    /// (verifying the server) and when it accepts one (the mTLS client-certificate verifier).
    root_cert_store: RootCertStore,
    /// This node's certificate chain, presented both as the TLS server certificate and as the
    /// client identity for mutual authentication.
    server_cert: Vec<CertificateDer<'a>>,
    /// Private key associated with `server_cert`.
    priv_key: PrivateKeyDer<'a>,
}

impl NetworkConfig<'_> {
    /// Creates a configuration for the network from a configuration file.
    pub fn new(path_file: &Path) -> io::Result<Self> {
        let raw_file: NetworkConfigFile = serde_json::from_str(&fs::read_to_string(path_file)?)?;

        let priv_key = PrivateKeyDer::from_pem_file(raw_file.priv_key)
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?;

        let server_cert = CertificateDer::pem_file_iter(raw_file.server_cert)
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?
            .map(|cert| cert.unwrap())
            .collect();

        let mut trusted_certs = Vec::new();
        for trusted_cert in &raw_file.trusted_certs {
            trusted_certs.extend(
                CertificateDer::pem_file_iter(trusted_cert)
                    .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?
                    .map(|cert| cert.unwrap()),
            )
        }

        let mut root_cert_store = RootCertStore::empty();
        let (certs_added, certs_ignored) = root_cert_store.add_parsable_certificates(trusted_certs);
        log::info!("added {certs_added} certificates, ignored {certs_ignored} certificates to the root certificate store");

        Ok(Self {
            base_port: raw_file.base_port,
            timeout: Duration::from_millis(raw_file.timeout),
            sleep_time: Duration::from_millis(raw_file.sleep_time),
            peer_ips: raw_file.peer_ips,
            root_cert_store,
            priv_key,
            server_cert,
        })
    }
}

/// Represents a network used to execute a protocol.
///
/// `Network` requires [`Send`] so that protocols generic over `N: Network` can be implemented with
/// the `#[async_trait]` `Protocol` trait (whose `run` future is `Send`). Both `SimNetwork` and
/// `TcpNetwork` already satisfy this.
#[async_trait]
pub trait Network: Send {
    /// Sends a `packet` to the party with ID `party_id`.
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> Result<usize>;
    /// Receives a `packet` from the party with ID `party_id`.
    async fn recv_from(&mut self, party_id: PartyId) -> Result<Packet>;
    /// Receives a `packet` from any party returning also the party ID of the sender.
    async fn recv_any(&mut self) -> Result<(Packet, PartyId)>;
    /// Closes the connection with the network.
    async fn close(&mut self) -> Result<()>;
    /// Returns the ID of the party executing the current node.
    fn local_party(&self) -> PartyId;
    /// Method used in a **two-party** protocol to get the other party.
    ///
    /// # Errors
    ///
    /// This function returns an error if the protocol that is being executed is not a two party protocol.
    fn other(&self) -> Result<PartyId>;
}

/// Network that contains all the channels connected to the party. Each channel is
/// a connection to other parties.
pub struct TcpNetwork {
    /// ID of the local party.
    local_party_id: PartyId,
    /// Channels for each peer.
    peer_channels: Vec<Box<dyn Channel + Send>>,
}

impl TcpNetwork {
    /// Builds the client and server TLS configurations for **mutual** authentication (mTLS) from the
    /// network configuration: this node presents `server_cert` as both its server certificate and its
    /// client identity, and verifies peers in both roles against the trusted root store
    /// (`WebPkiClientVerifier` on the server side).
    ///
    /// # Error
    ///
    /// The function returns an error if the certificate/private key pair is invalid or the client
    /// certificate verifier cannot be built.
    fn configure_tls(config: &NetworkConfig<'static>) -> Result<(ClientConfig, ServerConfig)> {
        // Configure the client TLS.
        let client_conf = ClientConfig::builder()
            .with_root_certificates(config.root_cert_store.clone())
            .with_client_auth_cert(config.server_cert.clone(), config.priv_key.clone_key())?;

        // Configure the server TLS.
        let verifier =
            WebPkiClientVerifier::builder(Arc::new(config.root_cert_store.clone())).build()?;
        let server_conf = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(config.server_cert.clone(), config.priv_key.clone_key())
            .map_err(NetworkError::TlsError)?;

        Ok((client_conf, server_conf))
    }

    /// Creates a new network using the ID of the current party and the number of parties connected
    /// to the network.
    ///
    /// # Error
    ///
    /// The function returns an error in the following cases:
    /// - When the binding of the channel to a certain IP address is
    ///   not done correctly.
    /// - When the TLS configuration is not done correctly.
    /// - When the node is trying to connect as a server but is unable to accept the provided
    ///   client.
    pub async fn create(id: usize, config: NetworkConfig<'static>) -> Result<Self> {
        log::info!("creating network");
        let n_parties = config.peer_ips.len();
        let server_port = config.base_port + id as u16;
        let server_address =
            SocketAddr::new(std::net::IpAddr::V4(config.peer_ips[id]), server_port);
        let server_listener = TcpListener::bind(server_address).await?;
        log::info!("listening on {:?}", server_address);

        let (client_conf, server_conf) = Self::configure_tls(&config)?;

        // Channels are kept indexed by peer ID. Client connections and the loop-back channel land
        // at a known index, but a server accept resolves the peer only after the handshake, so each
        // slot is filled by the `remote_id` the accept reports rather than by loop order.
        let mut peers: Vec<Option<Box<dyn Channel + Send>>> =
            (0..n_parties).map(|_| None).collect();

        for i in 0..n_parties {
            match i.cmp(&id) {
                Ordering::Less => {
                    log::info!("connecting as a client with peer ID {i}");
                    let remote_port = config.base_port + i as u16;
                    let remote_address =
                        SocketAddr::new(std::net::IpAddr::V4(config.peer_ips[i]), remote_port);
                    let stream = channel::connect_as_client(
                        id,
                        remote_address,
                        config.timeout,
                        config.sleep_time,
                        &client_conf,
                    )
                    .await?;
                    peers[i] = Some(Box::new(stream));
                }
                Ordering::Greater => {
                    log::info!("acting as a server, waiting for a peer to connect");
                    let (stream, remote_id) =
                        channel::accept_connection(&server_listener, &server_conf).await?;
                    log::info!("accepted connection from peer ID {remote_id}");
                    peers[remote_id] = Some(Box::new(stream));
                }
                Ordering::Equal => {
                    log::info!("adding the loop-back channel");
                    peers[id] = Some(Box::new(LoopBackChannel::default()));
                }
            }
        }

        // Every slot must have been filled: peers with a lower ID connected to us, peers with a
        // higher ID we connected to, and our own slot is the loop-back channel.
        let peer_channels = peers
            .into_iter()
            .enumerate()
            .map(|(i, channel)| channel.ok_or(NetworkError::PartyNotFound(PartyId(i))))
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            local_party_id: PartyId(id),
            peer_channels,
        })
    }

    /// Send a packet to every party in the network.
    pub async fn send(&mut self, packet: &Packet) -> Result<usize> {
        let mut bytes_sent = 0;
        for i in 0..self.peer_channels.len() {
            bytes_sent = self
                .peer_channels
                .get_mut(i)
                .expect("channel index not found")
                .send(packet)
                .await
                .map_err(NetworkError::ChannelError)?;
        }
        Ok(bytes_sent)
    }

    /// Receive a packet from each party in the network.
    pub async fn recv(&mut self) -> Result<Vec<Packet>> {
        let mut packets = Vec::new();
        for i in 0..self.peer_channels.len() {
            let packet = self
                .peer_channels
                .get_mut(i)
                .expect("channel index not found")
                .recv()
                .await
                .map_err(NetworkError::ChannelError)?;
            packets.push(packet);
        }

        Ok(packets)
    }
}

#[async_trait]
impl Network for TcpNetwork {
    async fn recv_any(&mut self) -> Result<(Packet, PartyId)> {
        Err(NetworkError::Unsupported(
            "the recv_any is not supported yet for TcpNetwork",
        ))
    }

    fn other(&self) -> Result<PartyId> {
        if self.peer_channels.len() != 2 {
            Err(NetworkError::ExpectedTwoNodeNet(self.peer_channels.len()))
        } else {
            Ok(PartyId::from(1 - self.local_party_id.as_usize()))
        }
    }

    /// Sends a packet of information to a given party.
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> Result<usize> {
        let bytes_sent = self.peer_channels[usize::from(party_id)]
            .send(packet)
            .await
            .map_err(NetworkError::ChannelError)?;
        Ok(bytes_sent)
    }

    /// Receives a packet from a given party.
    async fn recv_from(&mut self, party_id: PartyId) -> Result<Packet> {
        let packet = self.peer_channels[usize::from(party_id)]
            .recv()
            .await
            .map_err(NetworkError::ChannelError)?;
        Ok(packet)
    }

    /// Closes the network by closing each channel.
    async fn close(&mut self) -> Result<()> {
        for i in 0..self.peer_channels.len() {
            self.peer_channels
                .get_mut(i)
                .expect("channel index not found")
                .close()
                .await
                .map_err(NetworkError::ChannelError)?;
        }
        Ok(())
    }

    fn local_party(&self) -> PartyId {
        self.local_party_id
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use rcgen::{CertificateParams, Issuer, KeyPair, SanType};
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::{rustls::pki_types::ServerName, TlsAcceptor, TlsConnector};

    use super::*;

    fn write_party_certs(dir: &TempDir, n_parties: usize) {
        // CA certificate
        let mut ca_params = CertificateParams::new(vec![]).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        // Save the CA cert
        let path_ca_cert = dir.path().join("rootCA.crt");
        let mut file_ca_cert = File::create_new(path_ca_cert).unwrap();
        file_ca_cert.write_all(ca_cert.pem().as_bytes()).unwrap();

        let issuer = Issuer::new(ca_params, ca_key);

        // Leaf cert. for Party i.
        for i in 0..n_parties {
            let mut leaf_party_cert_params = CertificateParams::new(vec![]).unwrap();
            leaf_party_cert_params.subject_alt_names =
                vec![SanType::IpAddress("127.0.0.1".parse().unwrap())];
            let leaf_key = KeyPair::generate().unwrap();
            let leaf_cert = leaf_party_cert_params
                .signed_by(&leaf_key, &issuer)
                .unwrap();
            // Save leaf certificate
            let path_leaf_cert = dir.path().join(format!("server_cert_p{i}.crt"));
            let mut file_leaf_cert = File::create_new(path_leaf_cert).unwrap();
            file_leaf_cert
                .write_all(leaf_cert.pem().as_bytes())
                .unwrap();

            // Save private key for Party i
            let path_priv_key = dir.path().join(format!("priv_key_p{i}.pem"));
            let mut file_priv_key = File::create_new(path_priv_key).unwrap();
            file_priv_key
                .write_all(leaf_key.serialize_pem().as_bytes())
                .unwrap();
        }
    }

    fn write_config_files(dir: &TempDir, n_parties: usize) {
        for i in 0..n_parties {
            let raw_net_config = NetworkConfigFile {
                base_port: 5000,
                timeout: 5000,
                sleep_time: 300,
                peer_ips: (0..n_parties)
                    .map(|_| "127.0.0.1".parse().unwrap())
                    .collect(),
                server_cert: dir.path().join(format!("server_cert_p{i}.crt")),
                priv_key: dir.path().join(format!("priv_key_p{i}.pem")),
                trusted_certs: vec![dir.path().join("rootCA.crt")],
            };

            let config_file_data = serde_json::to_string_pretty(&raw_net_config).unwrap();
            fs::write(
                dir.path().join(format!("net_config_p{i}.json")),
                config_file_data,
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn tls_handshake_correctness() {
        const N_PARTIES: usize = 2;
        let temp_dir = tempfile::tempdir().unwrap();
        write_party_certs(&temp_dir, N_PARTIES);
        write_config_files(&temp_dir, N_PARTIES);

        // Load the configuration from the created files.
        let cfg_party_0 =
            NetworkConfig::new(temp_dir.path().join("net_config_p0.json").as_path()).unwrap();
        let cfg_party_1 =
            NetworkConfig::new(temp_dir.path().join("net_config_p1.json").as_path()).unwrap();

        // Using party 0 as a client and party 1 as a server
        let (client_conf, _) = TcpNetwork::configure_tls(&cfg_party_0).unwrap();
        let (_, server_conf) = TcpNetwork::configure_tls(&cfg_party_1).unwrap();

        let (a, b) = tokio::io::duplex(64 * 1024);

        let tls_connector = TlsConnector::from(Arc::new(client_conf));
        let tls_acceptor = TlsAcceptor::from(Arc::new(server_conf));
        let server_name = ServerName::IpAddress(Ipv4Addr::new(127, 0, 0, 1).into());

        let (server_res, client_res) = tokio::join!(
            tls_acceptor.accept(a),
            tls_connector.connect(server_name, b)
        );

        let mut server = server_res.unwrap();
        let mut client = client_res.unwrap();

        client.write_all(b"ping").await.unwrap();
        client.flush().await.unwrap();

        let mut buff = [0u8; 4];
        server.read_exact(&mut buff).await.unwrap();
        assert_eq!(&buff, b"ping");
    }

    #[tokio::test]
    async fn server_rejects_client_without_certificate() {
        const N_PARTIES: usize = 2;
        let temp_dir = tempfile::tempdir().unwrap();
        write_party_certs(&temp_dir, N_PARTIES);
        write_config_files(&temp_dir, N_PARTIES);

        // Load the configuration from the created files.
        let cfg_party_1 =
            NetworkConfig::new(temp_dir.path().join("net_config_p1.json").as_path()).unwrap();
        let (_, server_conf) = TcpNetwork::configure_tls(&cfg_party_1).unwrap();

        // A client that presents NO client certificate.
        let client_conf = ClientConfig::builder()
            .with_root_certificates(cfg_party_1.root_cert_store.clone())
            .with_no_client_auth();

        let (a, b) = tokio::io::duplex(64 * 1024);
        let acceptor = TlsAcceptor::from(Arc::new(server_conf));
        let connector = TlsConnector::from(Arc::new(client_conf));
        let name = ServerName::IpAddress(Ipv4Addr::new(127, 0, 0, 1).into());

        let (server_res, _client_res) =
            tokio::join!(acceptor.accept(a), connector.connect(name, b));

        // The server must reject the unauthenticated client.
        assert!(
            server_res.is_err(),
            "server accepted a client with no certificate"
        );
    }
}
