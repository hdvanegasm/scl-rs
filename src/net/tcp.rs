//! Real-network backend: connects this node to every party over mutually authenticated TLS.
//!
//! This is the production counterpart to the deterministic [`simulation`](crate::net::simulation)
//! backend; both implement the shared [`Network`] trait defined in [`crate::net`].

use crate::net::Element;

use super::channel;
use super::{Network, NetworkConfig, NetworkError, Packet, PartyId, Result};
use async_trait::async_trait;
use futures_util::future::try_join_all;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{ClientConfig, ServerConfig};
use tokio_rustls::TlsStream;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::{Stream, StreamExt, StreamMap};
use tokio_util::codec::{FramedRead, LengthDelimitedCodec};

/// Read side of a peer connection: a stream that yields one decoded [`Packet`] per delimited frame.
///
/// For a socket peer this wraps a [`FramedRead`] over the TLS stream's read half, so a partially
/// read frame stays buffered across polls and a dropped receive future is cancel-safe. For the
/// loop-back peer it wraps the receiving end of an in-process `mpsc` channel. Both are boxed behind
/// the same type so the receive paths can treat every peer uniformly.
type PacketStream = Pin<Box<dyn Stream<Item = Result<Packet>> + Send>>;

/// Write side of a peer connection.
///
/// Mirrors [`PacketStream`]: either the write half of a peer's TLS stream or the sending end of the
/// loop-back `mpsc` channel.
enum PeerWriter {
    /// Sending end of the in-process loop-back channel (messages from this node to itself).
    LoopBack(UnboundedSender<Packet>),
    /// Write half of a peer's TLS stream.
    Socket(WriteHalf<TlsStream<TcpStream>>),
}

impl PeerWriter {
    /// Sends a [`Packet`] to the peer, returning the number of payload bytes sent.
    ///
    /// On a socket this writes the postcard-encoded packet with an 8-byte little-endian length
    /// prefix (matching the `LengthDelimitedCodec` on the read side) and flushes it. On the
    /// loop-back channel it hands the packet to the receiver directly.
    async fn send(&mut self, packet: Packet) -> Result<usize> {
        match self {
            PeerWriter::LoopBack(sender) => {
                let size_pkg = packet.size();
                sender.send(packet)?;
                Ok(size_pkg)
            }
            PeerWriter::Socket(stream) => {
                let bytes = postcard::to_allocvec(&packet)?;
                let len_message = bytes.len().to_le_bytes();
                stream.write_all(&len_message).await?;
                stream.write_all(&bytes).await?;
                stream.flush().await?;
                Ok(packet.size())
            }
        }
    }

    /// Closes the peer connection. Shuts down the TLS stream's write half for a socket; the
    /// loop-back channel needs no explicit shutdown, so this is a no-op there.
    async fn close(&mut self) -> Result<()> {
        if let PeerWriter::Socket(socket_writer) = self {
            socket_writer.shutdown().await?;
        }
        Ok(())
    }
}

/// A real-network backend connecting this node to every party over mutually authenticated TLS.
///
/// Each peer connection is split into a `PeerWriter` and a `PacketStream`, both keyed by the peer's
/// [`PartyId`]. The party's connection to itself is an in-process loop-back channel rather than a
/// socket. Receiving from a single peer polls that peer's stream; [`Network::recv_any`] polls all of
/// them at once through the underlying `StreamMap`.
pub struct TcpNetwork {
    /// ID of the party running this node.
    local_party_id: PartyId,
    /// Write side of every peer connection, keyed by peer ID.
    writers: HashMap<PartyId, PeerWriter>,
    /// Read side of every peer connection, keyed by peer ID. Polling the whole map yields the next
    /// packet from whichever peer delivers first; polling a single entry receives from that peer.
    receivers: StreamMap<PartyId, PacketStream>,
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

    /// Splits a peer's TLS stream into its write and read halves, wrapping the read half in a
    /// length-delimited, postcard-decoding [`PacketStream`]. The codec uses an 8-byte little-endian
    /// length prefix to match what [`PeerWriter::send`] writes on the socket.
    fn split_socket(stream: TlsStream<TcpStream>) -> (PeerWriter, PacketStream) {
        let (read_half, write_half) = tokio::io::split(stream);
        let codec = LengthDelimitedCodec::builder()
            .little_endian()
            .length_field_length(8)
            .new_codec();
        let reader: PacketStream = Box::pin(FramedRead::new(read_half, codec).map(|frame| {
            let bytes = frame?;
            let inner_bytes: Vec<Element> = postcard::from_bytes(&bytes)?;
            Ok(Packet::new(inner_bytes))
        }));
        (PeerWriter::Socket(write_half), reader)
    }

    /// Builds the loop-back peer: an in-process `mpsc` channel whose sender becomes the
    /// [`PeerWriter`] and whose receiver becomes the [`PacketStream`]. Lets a node send packets to
    /// itself through the same interface it uses for remote peers.
    fn create_loopback() -> (PeerWriter, PacketStream) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let reader: PacketStream = Box::pin(UnboundedReceiverStream::new(receiver).map(Ok));
        (PeerWriter::LoopBack(sender), reader)
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
    /// - With [`NetworkError::PartyNotFound`] when, after every connection is established, some peer
    ///   ID is missing (for example because two accepts reported the same `remote_id`).
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
        let mut writers = HashMap::new();
        let mut readers = StreamMap::new();

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
                    let (writer, reader) = Self::split_socket(stream);
                    readers.insert(PartyId::from(i), reader);
                    writers.insert(PartyId::from(i), writer);
                }
                Ordering::Greater => {
                    log::info!("acting as a server, waiting for a peer to connect");
                    let (stream, remote_id) =
                        channel::accept_connection(&server_listener, &server_conf).await?;
                    log::info!("accepted connection from peer ID {remote_id}");
                    let (writer, reader) = Self::split_socket(stream);
                    readers.insert(PartyId::from(remote_id), reader);
                    writers.insert(PartyId::from(remote_id), writer);
                }
                Ordering::Equal => {
                    log::info!("adding the loop-back channel");
                    let (writer, reader) = Self::create_loopback();
                    readers.insert(PartyId::from(id), reader);
                    writers.insert(PartyId::from(id), writer);
                }
            }
        }

        // Check that all parties are present
        for i in 0..n_parties {
            if !writers.contains_key(&PartyId::from(i)) {
                return Err(NetworkError::PartyNotFound(PartyId::from(i)));
            }
        }

        Ok(Self {
            writers,
            receivers: readers,
            local_party_id: PartyId::from(id),
        })
    }

    /// Sends a packet to every party in the network, including this node's own loop-back channel.
    /// Returns the number of payload bytes written for the last peer.
    pub async fn send(&mut self, packet: &Packet) -> Result<usize> {
        let mut bytes_sent = 0;
        for writer in self.writers.values_mut() {
            bytes_sent = writer.send(packet.clone()).await?;
        }
        Ok(bytes_sent)
    }

    /// Receives one packet from each party in the network, including this node's own loop-back
    /// channel, ordered by ascending party ID.
    pub async fn recv(&mut self) -> Result<Vec<Packet>> {
        let mut packets = Vec::new();
        for i in 0..self.receivers.len() {
            packets.push(self.recv_from(PartyId::from(i)).await?);
        }
        Ok(packets)
    }
}

#[async_trait]
impl Network for TcpNetwork {
    fn party_ids(&self) -> Vec<PartyId> {
        self.writers.keys().copied().collect()
    }

    async fn recv_any(&mut self) -> Result<(Packet, PartyId)> {
        match self.receivers.next().await {
            Some((peer_id, result_packet)) => Ok((result_packet?, peer_id)),
            None => Err(NetworkError::ConnectionClosed(None)),
        }
    }

    fn other(&self) -> Result<PartyId> {
        if self.writers.len() != 2 {
            Err(NetworkError::ExpectedTwoNodeNet(self.writers.len()))
        } else {
            Ok(PartyId::from(1 - self.local_party_id.as_usize()))
        }
    }

    /// Sends a packet of information to a given party.
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> Result<usize> {
        let bytes_sent = self
            .writers
            .get_mut(&party_id)
            .ok_or(NetworkError::PartyNotFound(party_id))?
            .send(packet.clone())
            .await?;
        Ok(bytes_sent)
    }

    /// Sends every message concurrently — one independent TLS socket per peer.
    ///
    /// Each peer's write half is independent, so rather than awaiting the sends one after another
    /// (the default), this drives them all concurrently *within the current task* via
    /// [`try_join_all`]: a fan-out round then costs roughly one send's latency instead of their sum.
    /// No task is spawned, so this stays a plain `.await` over `Network` futures. Targets are
    /// validated up front, so a missing peer is reported before any packet is sent.
    async fn send_many(&mut self, messages: &[(PartyId, Packet)]) -> Result<()> {
        for (party_id, _) in messages {
            if !self.writers.contains_key(party_id) {
                return Err(NetworkError::PartyNotFound(*party_id));
            }
        }
        // `iter_mut` hands out a disjoint `&mut` per peer, so every matched send borrows a different
        // writer and they can all be in flight at once.
        let sends = self.writers.iter_mut().filter_map(|(party_id, writer)| {
            messages
                .iter()
                .find(|(target, _)| target == party_id)
                .map(|(_, packet)| writer.send(packet.clone()))
        });
        try_join_all(sends).await?;
        Ok(())
    }

    /// Receives a packet from a given party.
    async fn recv_from(&mut self, party_id: PartyId) -> Result<Packet> {
        let (_, reader) = self
            .receivers
            .iter_mut()
            .find(|(id, _)| *id == party_id)
            .ok_or(NetworkError::PartyNotFound(party_id))?;
        match reader.next().await {
            Some(packet) => packet,
            None => Err(NetworkError::ConnectionClosed(Some(party_id))),
        }
    }

    /// Closes the network by closing each channel.
    async fn close(&mut self) -> Result<()> {
        for writer in self.writers.values_mut() {
            writer.close().await?;
        }
        self.receivers.clear();
        Ok(())
    }

    fn local_party(&self) -> PartyId {
        self.local_party_id
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::Ipv4Addr;
    use std::{fs::File, io::Write};

    use rcgen::{CertificateParams, Issuer, KeyPair, SanType};
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::try_join;
    use tokio_rustls::{rustls::pki_types::ServerName, TlsAcceptor, TlsConnector};

    use super::super::NetworkConfigFile;
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

    fn write_config_files(dir: &TempDir, n_parties: usize, base_port: u16) {
        for i in 0..n_parties {
            let raw_net_config = NetworkConfigFile {
                base_port,
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

    fn free_base_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test]
    async fn tls_public_api_correctness() {
        const N_PARTIES: usize = 2;
        let temp_dir = tempfile::tempdir().unwrap();
        write_party_certs(&temp_dir, N_PARTIES);
        write_config_files(&temp_dir, N_PARTIES, free_base_port());

        // Load the configuration from the created files.
        let cfg_party_0 =
            NetworkConfig::new(temp_dir.path().join("net_config_p0.json").as_path()).unwrap();
        let cfg_party_1 =
            NetworkConfig::new(temp_dir.path().join("net_config_p1.json").as_path()).unwrap();

        let (mut net0, mut net1) = try_join!(
            TcpNetwork::create(0, cfg_party_0),
            TcpNetwork::create(1, cfg_party_1),
        )
        .unwrap();

        let mut packet_send_recv = Packet::empty();
        packet_send_recv.write(&123u8).unwrap();

        net0.send_to(PartyId::from(1), &packet_send_recv)
            .await
            .unwrap();
        let recv_pkg = net1.recv_from(PartyId::from(0)).await.unwrap();
        assert_eq!(packet_send_recv, recv_pkg);

        let mut packet_send_recv_any = Packet::empty();
        packet_send_recv_any.write(&111u8).unwrap();

        net0.send_to(PartyId::from(1), &packet_send_recv_any)
            .await
            .unwrap();
        let (recv_pkg, sender_pid) = net1.recv_any().await.unwrap();
        assert_eq!(sender_pid, PartyId::from(0));
        assert_eq!(recv_pkg, packet_send_recv_any);

        let mut big_packet = Packet::empty();
        let blob: Vec<u8> = (0..64 * 1024).map(|i| i as u8).collect();
        big_packet.write(&blob).unwrap();
        big_packet.write(&u64::MAX).unwrap();
        big_packet.write(&"a string element".to_string()).unwrap();
        big_packet
            .write(&(0..1000u32).collect::<Vec<u32>>())
            .unwrap();

        net0.send_to(PartyId::from(1), &big_packet).await.unwrap();
        let received = net1.recv_from(PartyId::from(0)).await.unwrap();
        assert_eq!(big_packet, received);

        // `send_many`: party 0 scatters distinct packets to party 1 (a TLS socket) and to itself
        // (the in-process loop-back) in one concurrent call. Each target must receive exactly its own
        // packet, exercising the concurrent override's per-peer matching across both writer kinds.
        let mut pkt_to_peer = Packet::empty();
        pkt_to_peer.write(&201u8).unwrap();
        let mut pkt_to_self = Packet::empty();
        pkt_to_self.write(&202u8).unwrap();

        net0.send_many(&[
            (PartyId::from(1), pkt_to_peer.clone()),
            (PartyId::from(0), pkt_to_self.clone()),
        ])
        .await
        .unwrap();

        let received_at_peer = net1.recv_from(PartyId::from(0)).await.unwrap();
        assert_eq!(received_at_peer, pkt_to_peer);
        let received_at_self = net0.recv_from(PartyId::from(0)).await.unwrap();
        assert_eq!(received_at_self, pkt_to_self);

        net0.close().await.unwrap();
        net1.close().await.unwrap();
    }

    #[tokio::test]
    async fn tls_handshake_correctness() {
        const N_PARTIES: usize = 2;
        let temp_dir = tempfile::tempdir().unwrap();
        write_party_certs(&temp_dir, N_PARTIES);
        write_config_files(&temp_dir, N_PARTIES, free_base_port());

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
        write_config_files(&temp_dir, N_PARTIES, free_base_port());

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

    /// Loads the configuration for party `i` from the files written into `dir`.
    fn load_config(dir: &TempDir, i: usize) -> NetworkConfig<'static> {
        NetworkConfig::new(dir.path().join(format!("net_config_p{i}.json")).as_path()).unwrap()
    }

    #[tokio::test]
    async fn recv_any_collects_from_multiple_peers_over_tls() {
        // Three parties over real mTLS: party 0 is the collector, parties 1 and 2 each send their
        // own id to it. The collector gathers both with `recv_any`, never naming a sender. Unlike
        // the two-party `tls_public_api_correctness` test, this drives `recv_any`'s `StreamMap`
        // over more than two live sockets, which is where fairness across peers actually matters.
        const N_PARTIES: usize = 3;
        let temp_dir = tempfile::tempdir().unwrap();
        write_party_certs(&temp_dir, N_PARTIES);
        write_config_files(&temp_dir, N_PARTIES, free_base_port());

        let (mut net0, net1, net2) = try_join!(
            TcpNetwork::create(0, load_config(&temp_dir, 0)),
            TcpNetwork::create(1, load_config(&temp_dir, 1)),
            TcpNetwork::create(2, load_config(&temp_dir, 2)),
        )
        .unwrap();

        // The collector gathers two messages from whichever peers respond first. It only borrows
        // its network for the duration of the collection — tearing it down here would race the
        // senders' own teardown, so all closing is deferred until after everyone is done.
        let collector = async {
            let mut heard = Vec::new();
            for _ in 0..2 {
                let (packet, sender) = net0.recv_any().await.unwrap();
                // Each sender writes its own id, so the payload must match the reported sender.
                let payload: usize = packet.read(0).unwrap();
                assert_eq!(payload, sender.as_usize());
                heard.push(sender.as_usize());
            }
            heard.sort_unstable();
            heard
        };
        // Each sender reports its own id to the collector and hands its network back for teardown.
        let send_from = |mut net: TcpNetwork, me: usize| async move {
            let mut packet = Packet::empty();
            packet.write(&me).unwrap();
            net.send_to(PartyId::from(0), &packet).await.unwrap();
            net
        };

        let (heard, net1, net2) = tokio::join!(collector, send_from(net1, 1), send_from(net2, 2));

        // The collector heard from exactly the two senders, without ever naming them.
        assert_eq!(heard, vec![1, 2]);

        // Everyone has finished; tear the sockets down. A peer may already be gone by the time a
        // given close runs, so a broken-pipe here is benign and deliberately not asserted on.
        let mut net1 = net1;
        let mut net2 = net2;
        let _ = net0.close().await;
        let _ = net1.close().await;
        let _ = net2.close().await;
    }

    #[tokio::test]
    async fn recv_from_closed_peer_reports_connection_closed() {
        // A peer that shuts down cleanly mid-session must surface as `ConnectionClosed` on the
        // other side's next receive, rather than hanging or reporting a generic I/O error.
        const N_PARTIES: usize = 2;
        let temp_dir = tempfile::tempdir().unwrap();
        write_party_certs(&temp_dir, N_PARTIES);
        write_config_files(&temp_dir, N_PARTIES, free_base_port());

        let (mut net0, mut net1) = try_join!(
            TcpNetwork::create(0, load_config(&temp_dir, 0)),
            TcpNetwork::create(1, load_config(&temp_dir, 1)),
        )
        .unwrap();

        // Party 0 shuts down its write half cleanly (TLS close-notify), so party 1's stream from it
        // reaches end-of-input rather than erroring.
        net0.close().await.unwrap();

        let result = net1.recv_from(PartyId::from(0)).await;
        assert!(
            matches!(&result, Err(NetworkError::ConnectionClosed(Some(peer))) if *peer == PartyId::from(0)),
            "expected ConnectionClosed(Some(0)), got {result:?}"
        );

        net1.close().await.unwrap();
    }

    #[test]
    fn malformed_config_json_is_rejected() {
        // The file exists and is readable, but its contents are not valid JSON, so the loader must
        // report a `ConfigParse` error rather than an I/O error.
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("bad_config.json");
        fs::write(&path, "{ this is not valid json ]").unwrap();

        match NetworkConfig::new(&path) {
            Err(NetworkError::ConfigParse(_)) => {}
            other => panic!("expected ConfigParse, got {:?}", other.err()),
        }
    }

    #[test]
    fn unloadable_pem_material_is_rejected() {
        // A structurally valid config whose private-key path points at a file that exists but is
        // not PEM. The JSON parses fine; loading the PEM material is what fails, so the loader must
        // report `InvalidPemFile` rather than `ConfigParse` or an I/O error.
        let temp_dir = tempfile::tempdir().unwrap();
        let garbage = temp_dir.path().join("not_a_key.pem");
        fs::write(&garbage, b"this is not PEM material").unwrap();

        let raw = NetworkConfigFile {
            base_port: 5000,
            timeout: 5000,
            sleep_time: 300,
            peer_ips: vec!["127.0.0.1".parse().unwrap()],
            server_cert: garbage.clone(),
            priv_key: garbage.clone(),
            trusted_certs: vec![garbage.clone()],
        };
        let config_path = temp_dir.path().join("config.json");
        fs::write(&config_path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

        match NetworkConfig::new(&config_path) {
            Err(NetworkError::InvalidPemFile(_)) => {}
            other => panic!("expected InvalidPemFile, got {:?}", other.err()),
        }
    }
}
