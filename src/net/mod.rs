pub mod channel;
pub mod fake_net;

use crate::net::channel::Channel;
use channel::{DummyChannel, LoopBackChannel};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    ClientConfig, RootCertStore, ServerConfig, StreamOwned,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    cmp::Ordering,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    path::Path,
    str::FromStr,
    time::Duration,
};
use std::{
    fs,
    io::{self, Error, ErrorKind},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("TLS error: {0:?}")]
    TlsError(rustls::Error),

    #[error("IO error: {0:?}")]
    IoError(io::Error),

    #[error("channel error: {0:?}")]
    ChannelError(channel::ChannelError),
}

pub type Result<T> = std::result::Result<T, NetworkError>;

// This packet can be changed such that the elements in it can be of multiple types.
// The idea would be as follows. The packet is a Vec<Vec<u8>>, so the packet has the structure
//
// [
//  [obj1 bytes]
//  [obj2 bytes]
//  [obj3 bytes]
// ]
//
// Hence each object can be deserialized according with its method. The method `read()` should be
// changed so that one can read an element from the packet at a time, as in a queue or a deque. So
// once you execute `read()` you obtain the first element of the vector (for example).

/// Packet of information sent through a given channel.
#[derive(Debug, Serialize, Deserialize)]
pub struct Packet(Vec<u8>);

impl Packet {
    pub fn empty() -> Self {
        Self(Vec::new())
    }
    /// Creates a new packet.
    pub fn new(buffer: Vec<u8>) -> Self {
        Self(buffer)
    }

    /// Returns an slice to the packet.
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Returns the size of the packet.
    pub fn size(&self) -> usize {
        self.0.len()
    }

    pub fn pop<'de, T>(&mut self) -> T
    where
        T: Deserialize<'de>,
    {
        todo!()
    }

    pub fn read<T>(&self, _obj_idx: usize) -> T {
        todo!()
    }

    pub fn write<T>(&mut self, _obj: T)
    where
        T: Serialize,
    {
        todo!()
    }
}

impl From<&[u8]> for Packet {
    fn from(value: &[u8]) -> Self {
        Self(Vec::from(value))
    }
}

/// Configuration of the network
pub struct NetworkConfig<'a> {
    /// Port that will be use as a base to define the port of each party. Party `i` will listen at
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
                "timeout is not correct",
            ))?),
            sleep_time: Duration::from_millis(json["sleep_time"].as_u64().ok_or(Error::new(
                ErrorKind::InvalidInput,
                "the timeout is not correct",
            ))?),
            peer_ips,
            root_cert_store,
            priv_key,
            server_cert,
        })
    }
}

/// Network that contains all the channels connected to the party. Each channel is
/// a connection to other parties.
pub struct Network {
    /// Channnels for each peer.
    peer_channels: Vec<Box<dyn Channel>>,
}

impl Network {
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
    pub fn create(id: usize, config: NetworkConfig<'static>) -> Result<Self> {
        log::info!("creating network");
        let n_parties = config.peer_ips.len();
        let server_port = config.base_port + id as u16;
        let server_address =
            SocketAddr::new(std::net::IpAddr::V4(config.peer_ips[id]), server_port);
        let server_listener = TcpListener::bind(server_address).map_err(NetworkError::IoError)?;
        log::info!("listening on {:?}", server_address);

        let (client_conf, server_conf) = Self::configure_tls(&config)?;

        let mut peers: Vec<Box<dyn Channel>> = Vec::new();
        for i in 0..n_parties {
            if i != id {
                peers.push(Box::new(DummyChannel));
            } else {
                peers.push(Box::new(LoopBackChannel::default()));
            }
        }

        for i in 0..n_parties {
            match i.cmp(&id) {
                Ordering::Less => {
                    log::info!("connecting as a client with peer ID {i}");
                    let remote_port = config.base_port + i as u16;
                    let remote_address =
                        SocketAddr::new(std::net::IpAddr::V4(config.peer_ips[i]), remote_port);
                    let (client_conn, tcp_stream) = channel::connect_as_client(
                        id,
                        remote_address,
                        config.timeout,
                        config.sleep_time,
                        &client_conf,
                    )
                    .map_err(NetworkError::ChannelError)?;
                    let stream = StreamOwned::new(client_conn, tcp_stream);
                    peers[i] = Box::new(stream);
                }
                Ordering::Greater => {
                    log::info!("acting as a server for peer ID {i}");
                    let (server_conn, tcp_stream, remote_id) =
                        channel::accept_connection(&server_listener, &server_conf)
                            .map_err(NetworkError::ChannelError)?;
                    let stream = StreamOwned::new(server_conn, tcp_stream);
                    peers[remote_id] = Box::new(stream);
                }
                Ordering::Equal => {
                    log::info!("adding the loop-back channel");
                    peers[i] = Box::new(LoopBackChannel::default());
                }
            }
        }
        Ok(Self {
            peer_channels: peers,
        })
    }

    /// Send a packet to every party in the network.
    pub fn send(&mut self, packet: &Packet) -> Result<usize> {
        let mut bytes_sent = 0;
        for i in 0..self.peer_channels.len() {
            bytes_sent = self
                .peer_channels
                .get_mut(i)
                .expect("channel index not found")
                .send(packet)
                .map_err(NetworkError::ChannelError)?;
        }
        Ok(bytes_sent)
    }

    /// Receive a packet from each party in the network.
    pub fn recv(&mut self) -> Result<Vec<Packet>> {
        let mut packets = Vec::new();
        for i in 0..self.peer_channels.len() {
            let packet = self
                .peer_channels
                .get_mut(i)
                .expect("channel index not found")
                .recv()
                .map_err(NetworkError::ChannelError)?;
            packets.push(packet);
        }

        Ok(packets)
    }

    /// Closes the network by closing each channel.
    pub fn close(&mut self) -> Result<()> {
        for i in 0..self.peer_channels.len() {
            self.peer_channels
                .get_mut(i)
                .expect("channel index not found")
                .shutdown()
                .map_err(NetworkError::ChannelError)?;
        }
        Ok(())
    }

    /// Sends a packet of information to a given party.
    pub fn send_to(&mut self, packet: &Packet, party_id: usize) -> Result<usize> {
        let bytes_sent = self.peer_channels[party_id]
            .send(packet)
            .map_err(NetworkError::ChannelError)?;
        Ok(bytes_sent)
    }

    /// Receives a packet from a given party.
    pub fn recv_from(&mut self, party_id: usize) -> Result<Packet> {
        let packet = self.peer_channels[party_id]
            .recv()
            .map_err(NetworkError::ChannelError)?;
        Ok(packet)
    }
}
