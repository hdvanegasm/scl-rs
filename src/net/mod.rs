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
use serde_json::Value;
use std::sync::Arc;
use std::{
    cmp::Ordering,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    str::FromStr,
    time::Duration,
};
use std::{
    fs,
    io::{self, Error, ErrorKind},
};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
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

    fn ne(&self, other: &Packet) -> bool {
        self.0 != other.0
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
    pub fn pop<'de, T>(&mut self) -> Option<T>
    where
        T: DeserializeOwned,
    {
        let bytes = self.0.pop()?;
        let object = from_bytes(&bytes).ok()?;
        Some(object)
    }

    /// Read the element at the given index inside the [`Packet`].
    pub fn read<'de, T>(&self, obj_idx: usize) -> Option<T>
    where
        T: DeserializeOwned,
    {
        let bytes = self.0.get(obj_idx)?;
        let object = postcard::from_bytes(bytes).ok()?;
        Some(object)
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

/// Configuration of the network.
pub struct NetworkConfig<'a> {
    /// Port that will be used as a base to define the port of each party. Party `i` will listen at
    /// port `base_port + i`.
    base_port: u16,
    /// Timeout for receiving a message after calling the `recv()` function.
    timeout: Duration,
    /// Sleep time before trying to connect again with other party.
    sleep_time: Duration,
    /// IPs of each peer.
    pub peer_ips: Vec<Ipv4Addr>,
    /// Root of trust certificates when acting as a client.
    root_cert_store: RootCertStore,
    /// Certificates to act like a server.
    server_cert: Vec<CertificateDer<'a>>,
    /// Private key to act like a server.
    priv_key: PrivateKeyDer<'a>,
}

impl NetworkConfig<'_> {
    /// Creates a configuration for the network from a configuration file.
    pub fn new(path_file: &Path) -> io::Result<Self> {
        let json_content = fs::read_to_string(path_file)?;
        let json: Value = serde_json::from_str(&json_content)?;

        // Deserialize the peer ips.
        let peers_ips_json = json["peer_ips"].as_array().ok_or(Error::new(
            ErrorKind::InvalidInput,
            "the array of peers is not correct",
        ))?;
        let mut peer_ips = Vec::new();
        for ip_value in peers_ips_json {
            let ip_str = ip_value.as_str().ok_or(Error::new(
                ErrorKind::InvalidInput,
                "the ip of peer is not correct",
            ))?;
            peer_ips.push(
                Ipv4Addr::from_str(ip_str)
                    .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?,
            );
        }

        // Get private key.
        let priv_key_pem = json["priv_key"].as_str().ok_or(Error::new(
            ErrorKind::InvalidData,
            "the private key has not the correct format",
        ))?;
        let priv_key = PrivateKeyDer::from_pem_file(priv_key_pem)
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?;

        // Get the server certificate.
        let server_cert_file = json["server_cert"].as_str().ok_or(Error::new(
            ErrorKind::InvalidData,
            "the private key has not the correct format",
        ))?;
        let server_cert = CertificateDer::pem_file_iter(server_cert_file)
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?
            .map(|cert| cert.unwrap())
            .collect();

        // Get trusted certificates.
        let trusted_certs_json = json["trusted_certs"].as_array().ok_or(Error::new(
            ErrorKind::InvalidInput,
            "the array of peers is not correct",
        ))?;
        let mut trusted_certs = Vec::new();
        for trusted_cert in trusted_certs_json {
            let trusted_cert_path = trusted_cert.as_str().ok_or(Error::new(
                ErrorKind::InvalidInput,
                "the ip of peer is not correct",
            ))?;
            trusted_certs.extend(
                CertificateDer::pem_file_iter(trusted_cert_path)
                    .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?
                    .map(|cert| cert.unwrap()),
            )
        }
        let mut root_cert_store = RootCertStore::empty();
        let (certs_added, certs_ignored) = root_cert_store.add_parsable_certificates(trusted_certs);
        log::info!("added {certs_added} certificates, ignored {certs_ignored} certificates to the root certificate store");

        Ok(Self {
            base_port: json["base_port"].as_u64().ok_or(Error::new(
                ErrorKind::InvalidInput,
                "the base port is not correct",
            ))? as u16,
            timeout: Duration::from_millis(json["timeout"].as_u64().ok_or(Error::new(
                ErrorKind::InvalidInput,
                "the timeout is not correct",
            ))?),
            sleep_time: Duration::from_millis(json["sleep_time"].as_u64().ok_or(Error::new(
                ErrorKind::InvalidInput,
                "the sleep time is not correct",
            ))?),
            peer_ips,
            root_cert_store,
            priv_key,
            server_cert,
        })
    }
}

/// Represents a network used to execute a protocol.
#[async_trait]
pub trait Network {
    /// This method sends a `packet` to the party with ID `party_id`.
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> Result<usize>;
    /// This method receives a `packet` from the party with ID `party_id`.
    async fn recv_from(&mut self, party_id: PartyId) -> Result<Packet>;
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
    /// Configure the TLS channel according to the provided network configuration.
    ///
    /// # Error
    ///
    /// The function returns an error if the certificate and the private key are not configured
    /// correctly.
    fn configure_tls(config: &NetworkConfig<'static>) -> Result<(ClientConfig, ServerConfig)> {
        // Configure the client TLS
        let client_conf = ClientConfig::builder()
            .with_root_certificates(config.root_cert_store.clone())
            .with_no_client_auth();

        let server_conf = ServerConfig::builder()
            .with_no_client_auth()
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
    fn other(&self) -> Result<PartyId> {
        if self.peer_channels.len() != 2 {
            return Err(NetworkError::ExpectedTwoNodeNet(self.peer_channels.len()));
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
