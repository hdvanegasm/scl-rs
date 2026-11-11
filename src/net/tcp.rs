//! Real-network backend: connects this node to every party over mutually authenticated TLS.
//!
//! This is the production counterpart to the deterministic [`simulation`]
//! backend; both implement the shared [`Network`] trait defined in [`crate::net`].

use crate::net::session::SessionId;
use crate::net::ForkNetwork;
use crate::net::NetworkError::ConnectionClosed;

use super::channel;
use super::{Network, NetworkConfig, NetworkError, Packet, PartyId, Result};
use futures_util::future::poll_fn;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::io::{self, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::task::AbortHandle;
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{ClientConfig, ServerConfig};
use tokio_rustls::TlsStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, LengthDelimitedCodec};

const LENGTH_PREFIX_BYTES: usize = 8;

fn encode_frame(session: &SessionId, packet: &Packet) -> Result<Vec<u8>> {
    let body = postcard::to_allocvec(&(session.path(), &packet))?;
    let mut frame = Vec::with_capacity(LENGTH_PREFIX_BYTES + body.len());
    frame.extend_from_slice(&(body.len() as u64).to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

fn decode_frame(root_tag: [u8; 32], body: &[u8]) -> Result<(SessionId, Packet)> {
    let (path, elements) = postcard::from_bytes(body)?;
    let sid = SessionId::new(root_tag, path);
    Ok((sid, Packet::new(elements)))
}

type Socket = TlsStream<TcpStream>;

#[derive(Debug)]
enum WriterCmd {
    Frame(Vec<u8>),
    Close(oneshot::Sender<Result<()>>),
}

#[derive(Debug)]
enum PeerState {
    Open,
    Closed,
    Failed(String),
}

#[derive(Debug, Default)]
struct SessionMailbox {
    queue: BTreeMap<PartyId, VecDeque<Packet>>,
    waiter: Option<Waker>,
}

#[derive(Default, Debug)]
struct Mailboxes {
    sessions: HashMap<SessionId, SessionMailbox>,
    peers: HashMap<PartyId, PeerState>,
    retired: HashSet<SessionId>,
}

impl Mailboxes {
    fn deliver(&mut self, sender: PartyId, session: SessionId, packet: Packet) -> Option<Waker> {
        if self.retired.contains(&session) {
            None
        } else {
            let mailbox = self.sessions.entry(session).or_default();
            mailbox.queue.entry(sender).or_default().push_back(packet);
            mailbox.waiter.take()
        }
    }

    fn end_peer(&mut self, peer: PartyId, end: PeerState) -> Vec<Waker> {
        if let Some(state @ PeerState::Open) = self.peers.get_mut(&peer) {
            *state = end;
        }
        self.sessions
            .values_mut()
            .filter_map(|mailbox| mailbox.waiter.take())
            .collect()
    }

    fn peer_error(&self, peer: PartyId) -> Option<NetworkError> {
        match self.peers.get(&peer)? {
            PeerState::Open => None,
            PeerState::Closed => Some(ConnectionClosed(Some(peer))),
            PeerState::Failed(reason) => Some(NetworkError::PeerFailed {
                peer,
                reason: reason.clone(),
            }),
        }
    }

    fn poll_recv_from(
        &mut self,
        session: &SessionId,
        sender: PartyId,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Packet>> {
        let queued = self
            .sessions
            .get_mut(session)
            .and_then(|mailbox| mailbox.queue.get_mut(&sender))
            .and_then(VecDeque::pop_front);
        if let Some(packet) = queued {
            return Poll::Ready(Ok(packet));
        }

        if let Some(error) = self.peer_error(sender) {
            return Poll::Ready(Err(error));
        }

        self.sessions.entry(session.clone()).or_default().waiter = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_recv_any(
        &mut self,
        session: &SessionId,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(PartyId, Packet)>> {
        if let Some(mailbox) = self.sessions.get_mut(session) {
            for (pid, queue) in &mut mailbox.queue {
                if let Some(packet) = queue.pop_front() {
                    return Poll::Ready(Ok((*pid, packet)));
                }
            }
        }

        if self
            .peers
            .values()
            .all(|state| !matches!(state, PeerState::Open))
        {
            return Poll::Ready(Err(NetworkError::ConnectionClosed(None)));
        }

        self.sessions.entry(session.clone()).or_default().waiter = Some(cx.waker().clone());
        Poll::Pending
    }

    fn retire(&mut self, sid: &SessionId) {
        self.sessions.remove(sid);
        self.retired.insert(sid.clone());
    }
}

async fn read_loop(
    peer: PartyId,
    mut frames: FramedRead<ReadHalf<Socket>, LengthDelimitedCodec>,
    mailboxes: Arc<Mutex<Mailboxes>>,
    root_tag: [u8; 32],
) {
    // This variable defines how is the state of the peer.
    let end = loop {
        match frames.next().await {
            Some(Ok(body)) => match decode_frame(root_tag, &body) {
                Ok((sid, packet)) => {
                    // A packet has been received in the stream. Put it in the mailbox and wake the
                    // task to process it.
                    let waker = mailboxes
                        .lock()
                        .expect("the mutex can not be poisoned")
                        .deliver(peer, sid, packet);
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }
                // The frame was not decoded correctly. We return an error.
                Err(error) => break PeerState::Failed(error.to_string()),
            },
            Some(Err(error)) => break PeerState::Failed(error.to_string()),
            // There's no connection with the peer.
            None => break PeerState::Closed,
        }
    };

    // We report the end state of the party and wake all the receive sleep tasks.
    let wakers = mailboxes
        .lock()
        .expect("the mutex can not be poisoned")
        .end_peer(peer, end);
    wakers.into_iter().for_each(Waker::wake);
}

async fn write_loop(
    peer: PartyId,
    mut socket: WriteHalf<Socket>,
    mut commands_receiver: UnboundedReceiver<WriterCmd>,
    mailboxes: Arc<Mutex<Mailboxes>>,
) {
    while let Some(command) = commands_receiver.recv().await {
        match command {
            WriterCmd::Frame(frame) => {
                let written = async {
                    socket.write_all(&frame).await?;
                    socket.flush().await
                };
                if let Err(error) = written.await {
                    let wakers = mailboxes
                        .lock()
                        .expect("the mutex can not be poisoned")
                        .end_peer(peer, PeerState::Failed(error.to_string()));
                    wakers.into_iter().for_each(Waker::wake);
                    return;
                }
            }
            WriterCmd::Close(sender) => {
                let result = socket.shutdown().await.map_err(NetworkError::IoError);
                let _ = sender.send(result);
                return;
            }
        }
    }

    let _ = socket.shutdown().await;
}

#[derive(Debug, Default)]
struct TcpCore {
    parties: Vec<PartyId>,
    writers: HashMap<PartyId, UnboundedSender<WriterCmd>>,
    mailboxes: Arc<Mutex<Mailboxes>>,
    readers: Vec<AbortHandle>,
}

impl Drop for TcpCore {
    fn drop(&mut self) {
        for reader in &self.readers {
            reader.abort();
        }
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
    local_pid: PartyId,
    session: SessionId,
    core: Arc<TcpCore>,
    next_child: u32,
}

impl Drop for TcpNetwork {
    fn drop(&mut self) {
        if let Ok(mut mailboxes) = self.core.mailboxes.lock() {
            mailboxes.retire(&self.session)
        }
    }
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
    /// `root_tag` identifies this execution: it is the tag of the root session every handle of this
    /// network starts in (see [`SessionId::root`](crate::net::session::SessionId::root)). Every
    /// party of the execution must pass the same tag, and the tag must never be reused for another
    /// execution on the same parties, or session digests would no longer tell the two apart. There
    /// is deliberately no default. Derive it from public context that names the run, with
    /// [`derive_root_tag`](crate::net::session::derive_root_tag): for example the protocol name and
    /// version, the party set, the parameters, and an epoch counter the application never reuses.
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
    pub async fn create(
        id: usize,
        config: NetworkConfig<'static>,
        root_tag: [u8; 32],
    ) -> Result<Self> {
        log::info!("creating network");
        let n_parties = config.peer_ips.len();
        let server_port = config.base_port + id as u16;
        let server_address =
            SocketAddr::new(std::net::IpAddr::V4(config.peer_ips[id]), server_port);
        let server_listener = TcpListener::bind(server_address).await?;
        log::info!("listening on {:?}", server_address);

        let (client_conf, server_conf) = Self::configure_tls(&config)?;
        let parties: Vec<PartyId> = (0..n_parties).map(PartyId::from).collect();

        let mut sockets = HashMap::new();
        for i in parties.iter().map(PartyId::as_usize) {
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
                    sockets.insert(PartyId::from(i), stream);
                }
                Ordering::Greater => {
                    log::info!("acting as a server, waiting for a peer to connect");
                    let (stream, remote_id) =
                        channel::accept_connection(&server_listener, &server_conf).await?;
                    log::info!("accepted connection from peer ID {remote_id}");
                    sockets.insert(PartyId::from(remote_id), stream);
                }
                Ordering::Equal => {}
            }
        }

        // Check that all parties are present
        for i in parties.iter().map(PartyId::as_usize) {
            if id != i && !sockets.contains_key(&PartyId::from(i)) {
                return Err(NetworkError::PartyNotFound(PartyId::from(i)));
            }
        }

        // Create the mailboxes and spawn tasks.
        //
        // This needs to be done after all the parties are
        // connected because a later connection will return an error an leave an open socket in a
        // running task.
        let mailboxes = Arc::new(Mutex::new(Mailboxes {
            sessions: HashMap::new(),
            peers: sockets
                .keys()
                .map(|peer| (*peer, PeerState::Open))
                .collect(),
            retired: HashSet::new(),
        }));

        let mut writers = HashMap::new();
        let mut readers = Vec::new();
        for (peer, socket) in sockets {
            let (read_half, write_half) = io::split(socket);
            let codec = LengthDelimitedCodec::builder()
                .little_endian()
                .length_field_length(LENGTH_PREFIX_BYTES)
                .new_codec();
            let frames = FramedRead::new(read_half, codec);
            let reader = tokio::spawn(read_loop(peer, frames, mailboxes.clone(), root_tag));
            readers.push(reader.abort_handle());

            let (tx, rx) = mpsc::unbounded_channel();
            tokio::spawn(write_loop(peer, write_half, rx, mailboxes.clone()));
            writers.insert(peer, tx);
        }

        Ok(Self {
            local_pid: PartyId::from(id),
            session: SessionId::root(root_tag),
            core: Arc::new(TcpCore {
                parties,
                writers,
                mailboxes,
                readers,
            }),
            next_child: 0,
        })
    }

    /// Sends a packet to every party in the network, including this node's own loop-back channel.
    /// Returns the number of payload bytes written for the last peer.
    pub async fn send(&mut self, packet: &Packet) -> Result<usize> {
        let mut size = 0;
        for party in self.core.parties.clone() {
            size += self.send_to(party, packet).await?
        }
        Ok(size)
    }

    /// Receives one packet from each party in the network, including this node's own loop-back
    /// channel, ordered by ascending party ID.
    pub async fn recv(&mut self) -> Result<Vec<Packet>> {
        let mut packets = Vec::new();
        for party in self.core.parties.clone() {
            let p = self.recv_from(party).await?;
            packets.push(p);
        }
        Ok(packets)
    }
}

impl Network for TcpNetwork {
    fn party_ids(&self) -> Vec<PartyId> {
        self.core.parties.clone()
    }

    async fn recv_any(&mut self) -> Result<(PartyId, Packet)> {
        poll_fn(|cx| {
            self.core
                .mailboxes
                .lock()
                .expect("the mutex should not be poisoned")
                .poll_recv_any(&self.session, cx)
        })
        .await
    }

    async fn recv_any_with_timeout(&mut self, timeout: Duration) -> Result<(PartyId, Packet)> {
        let recv_fut = poll_fn(|cx| {
            self.core
                .mailboxes
                .lock()
                .expect("the mutex should not be poisoned")
                .poll_recv_any(&self.session, cx)
        });
        tokio::time::timeout(timeout, recv_fut)
            .await
            .unwrap_or(Err(NetworkError::Timeout(None)))
    }

    fn other(&self) -> Result<PartyId> {
        if self.party_ids().len() != 2 {
            Err(NetworkError::ExpectedTwoNodeNet(self.party_ids().len()))
        } else {
            Ok(PartyId::from(1 - self.local_pid.as_usize()))
        }
    }

    /// Sends a packet of information to a given party, in this handle's session.
    ///
    /// A send to a peer queues the encoded frame for that peer's writer task and returns without
    /// waiting for the write. A send to this party itself goes straight into this session's own
    /// mailbox. Either way the future never suspends, so dropping it (as a fail-fast join does to a
    /// sibling) cannot leave a frame half written. Returns the payload size of the packet.
    ///
    /// # Error
    ///
    /// - [`NetworkError::PartyNotFound`] if `party_id` is not a party of the network.
    /// - [`NetworkError::PeerFailed`] if the connection with the peer has failed.
    /// - [`NetworkError::ConnectionClosed`] if the peer's writer has stopped, after
    ///   [`close`](Network::close) or a failed write.
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> Result<usize> {
        if party_id.as_usize() >= self.core.parties.len() {
            return Err(NetworkError::PartyNotFound(party_id));
        }

        if party_id == self.local_pid {
            // Simulates a loop-back channel.
            let waker = self
                .core
                .mailboxes
                .lock()
                .expect("the mutex can not be poisoned")
                .deliver(self.local_pid, self.session.clone(), packet.clone());
            if let Some(waker) = waker {
                waker.wake();
            }
            return Ok(packet.size());
        }

        if let Some(error @ NetworkError::PeerFailed { .. }) = self
            .core
            .mailboxes
            .lock()
            .expect("the mutex can not be poisoned")
            .peer_error(party_id)
        {
            return Err(error);
        }

        let frame = encode_frame(&self.session, packet)?;
        self.core.writers[&party_id]
            .send(WriterCmd::Frame(frame))
            .map_err(|_| NetworkError::ConnectionClosed(Some(party_id)))?;
        Ok(packet.size())
    }

    /// Sends each `(party, packet)` in `messages`, in order.
    ///
    /// Every target is checked first, so a missing peer is reported before any packet is sent. The
    /// sends need no joining: [`send_to`](Network::send_to) only queues the frame for the peer's
    /// writer task, and the writer tasks already write to their sockets concurrently.
    async fn send_many(&mut self, messages: &[(PartyId, Packet)]) -> Result<()> {
        for (pid, _) in messages {
            if pid.as_usize() >= self.core.parties.len() {
                return Err(NetworkError::PartyNotFound(*pid));
            }
        }
        for (pid, p) in messages {
            self.send_to(*pid, p).await?;
        }
        Ok(())
    }

    async fn recv_from_with_timeout(
        &mut self,
        party_id: PartyId,
        timeout: Duration,
    ) -> Result<Packet> {
        if party_id.as_usize() >= self.core.parties.len() {
            return Err(NetworkError::PartyNotFound(party_id));
        }
        let recv_fut = poll_fn(|cx| {
            self.core
                .mailboxes
                .lock()
                .expect("the mutex should not be poisoned")
                .poll_recv_from(&self.session, party_id, cx)
        });
        tokio::time::timeout(timeout, recv_fut)
            .await
            .unwrap_or(Err(NetworkError::Timeout(Some(party_id))))
    }

    /// Receives a packet from a given party.
    async fn recv_from(&mut self, party_id: PartyId) -> Result<Packet> {
        if party_id.as_usize() >= self.core.parties.len() {
            return Err(NetworkError::PartyNotFound(party_id));
        }
        poll_fn(|cx| {
            self.core
                .mailboxes
                .lock()
                .expect("the mutex should not be poisoned")
                .poll_recv_from(&self.session, party_id, cx)
        })
        .await
    }

    /// Closes the network by closing each channel.
    /// Closes the network.
    async fn close(&mut self) -> Result<()> {
        if self.session.depth() > 0 {
            // A forked handle only gives up its own session.
            self.core
                .mailboxes
                .lock()
                .expect("the mutex must not be poisoned")
                .retire(&self.session);
            return Ok(());
        }
        let mut acks = Vec::new();
        for writer in self.core.writers.values() {
            let (ack, done) = oneshot::channel();
            // A writer that already exited has nothing left to close.
            if writer.send(WriterCmd::Close(ack)).is_ok() {
                acks.push(done);
            }
        }
        let mut result = Ok(());
        for done in acks {
            if let Ok(Err(error)) = done.await {
                result = result.and(Err(error));
            }
        }
        for reader in &self.core.readers {
            reader.abort();
        }

        let mut wakers = Vec::new();
        {
            let mut mailboxes = self
                .core
                .mailboxes
                .lock()
                .expect("the mutex must not be poisoned");
            for peer in self.core.writers.keys() {
                wakers.extend(mailboxes.end_peer(*peer, PeerState::Closed));
            }
        }
        wakers.into_iter().for_each(Waker::wake);
        result
    }

    fn local_party(&self) -> PartyId {
        self.local_pid
    }
}

impl ForkNetwork for TcpNetwork {
    fn fork(&mut self) -> Self {
        let session = self.session.with_child(self.next_child);
        self.next_child += 1;
        Self {
            local_pid: self.local_pid,
            session,
            core: self.core.clone(),
            next_child: 0,
        }
    }

    fn session(&self) -> &SessionId {
        &self.session
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::{fs::File, io::Write};

    use futures_util::future::try_join_all;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    use rcgen::{CertificateParams, Issuer, KeyPair, SanType};
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::try_join;
    use tokio_rustls::{rustls::pki_types::ServerName, TlsAcceptor, TlsConnector};

    use crate::math::field::mersenne61::Mersenne61;
    use crate::protocol::concurrent::Join;
    use crate::protocol::passive_shamir::{
        double_rand_share::PassiveRandDoubleShr, rand_share::PassiveRandShr,
    };
    use crate::protocol::{
        Environment, Error, ForkEnvironment, GeneralEnvironment, Protocol, ProtocolId,
        RandEnvironment,
    };
    use crate::ss::shamir::{DoubleShare, ShamirSS};
    use crate::ss::LinearShare;

    use super::super::NetworkConfigFile;
    use super::*;

    /// Root tag shared by every party of a test execution (see [`TcpNetwork::create`]).
    fn test_root_tag() -> [u8; 32] {
        crate::net::session::derive_root_tag(b"scl-rs/tcp.rs unit tests")
    }

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

    /// Returns the base of a block of ports that no other test of this binary uses.
    ///
    /// Party `i` listens on `base + i`, so a test needs a run of consecutive ports. Asking the OS
    /// for one free port reserves only that one: its neighbours may belong to another test running
    /// in parallel, or be the source port of some connection. The blocks are handed out from a
    /// counter instead, below Linux's default ephemeral range (32768-60999), which is where the OS
    /// picks source ports from.
    fn free_base_port() -> u16 {
        const BLOCK: u16 = 40;
        static NEXT: AtomicU16 = AtomicU16::new(20_000);
        NEXT.fetch_add(BLOCK, Ordering::Relaxed)
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
            TcpNetwork::create(0, cfg_party_0, test_root_tag()),
            TcpNetwork::create(1, cfg_party_1, test_root_tag()),
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
        let (sender_pid, recv_pkg) = net1.recv_any().await.unwrap();
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
            TcpNetwork::create(0, load_config(&temp_dir, 0), test_root_tag()),
            TcpNetwork::create(1, load_config(&temp_dir, 1), test_root_tag()),
            TcpNetwork::create(2, load_config(&temp_dir, 2), test_root_tag()),
        )
        .unwrap();

        // The collector gathers two messages from whichever peers respond first. It only borrows
        // its network for the duration of the collection — tearing it down here would race the
        // senders' own teardown, so all closing is deferred until after everyone is done.
        let collector = async {
            let mut heard = Vec::new();
            for _ in 0..2 {
                let (sender, packet) = net0.recv_any().await.unwrap();
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
            TcpNetwork::create(0, load_config(&temp_dir, 0), test_root_tag()),
            TcpNetwork::create(1, load_config(&temp_dir, 1), test_root_tag()),
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

    type F = Mersenne61;
    type Share = ShamirSS<1, F>;

    /// `n` networks connected to each other over mTLS loopback, indexed by party.
    async fn connected(n: usize) -> Vec<TcpNetwork> {
        let dir = tempfile::tempdir().unwrap();
        write_party_certs(&dir, n);
        write_config_files(&dir, n, free_base_port());
        try_join_all((0..n).map(|i| TcpNetwork::create(i, load_config(&dir, i), test_root_tag())))
            .await
            .unwrap()
    }

    /// How long a session test may run before [`within`] fails it.
    const TEST_TIMEOUT: Duration = Duration::from_secs(30);

    /// Runs `test`, and fails instead of hanging if it does not finish within `limit`: a routing
    /// bug shows up as a receive that never returns.
    async fn within<T>(limit: Duration, test: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(limit, test)
            .await
            .unwrap_or_else(|_| panic!("the test did not finish within {limit:?}"))
    }

    /// A packet holding `tag`, so that packets can be told apart by equality.
    fn packet(tag: u64) -> Packet {
        let mut packet = Packet::empty();
        packet.write(&tag).unwrap();
        packet
    }

    fn party(id: usize) -> PartyId {
        PartyId::from(id)
    }

    /// T3: on one link, session B's frame is written before session A's, while the receiver
    /// already waits in A. Each session receives its own packet.
    #[tokio::test]
    async fn sessions_on_one_tls_link_receive_only_their_own_packets() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
            let (mut a0, mut b0) = (net0.fork(), net0.fork());
            let (mut a1, mut b1) = (net1.fork(), net1.fork());

            // `biased;` makes `join!` poll in the order written: the receive in A is pending
            // before anything is sent. Without it, `join!` rotates which future it polls first.
            let (received_a, ()) = tokio::join!(biased; a1.recv_from(party(0)), async {
                b0.send_to(party(1), &packet(2)).await.unwrap();
                a0.send_to(party(1), &packet(1)).await.unwrap();
            });
            assert_eq!(received_a.unwrap(), packet(1));
            assert_eq!(b1.recv_from(party(0)).await.unwrap(), packet(2));
        })
        .await;
    }

    /// T5: a frame for session 0.3 arrives before the receiver has forked that session. It waits,
    /// and the receiver gets it once it forks down to 0.3.
    #[tokio::test]
    async fn a_frame_for_a_session_not_yet_forked_waits_for_the_fork() {
        /// Forks `handle` four times and returns the handle on its child 3.
        fn child_3(handle: &mut TcpNetwork) -> TcpNetwork {
            for _ in 0..3 {
                handle.fork();
            }
            handle.fork()
        }

        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());

            let mut branch0 = net0.fork();
            let mut early0 = child_3(&mut branch0);
            assert_eq!(early0.session().to_string(), "0.3");
            early0.send_to(party(1), &packet(7)).await.unwrap();
            // One reader files the frames of a link in order, so once this root-session packet
            // is received, the frame for 0.3 has been filed too.
            net0.send_to(party(1), &packet(0)).await.unwrap();
            assert_eq!(net1.recv_from(party(0)).await.unwrap(), packet(0));

            let mut branch1 = net1.fork();
            let mut early1 = child_3(&mut branch1);
            assert_eq!(early1.recv_from(party(0)).await.unwrap(), packet(7));
        })
        .await;
    }

    /// Three parties run a `Join` of two protocols of different types over mTLS.
    struct RandomAndDoubles;

    const T8_PARTIES: usize = 3;
    const T8_THRESHOLD: usize = 1;

    /// The random sharings, and the two halves of each double sharing.
    type RandomAndDoublesOutput = (Vec<Share>, Vec<(Share, Share)>);

    impl<E: RandEnvironment + ForkEnvironment> Protocol<E> for RandomAndDoubles {
        type Output = RandomAndDoublesOutput;

        async fn run(self, env: &mut E) -> std::result::Result<Self::Output, Error> {
            let parties = env.network().party_ids();
            let (random, doubles) = Join((
                PassiveRandShr::<1, F>::new(T8_THRESHOLD, parties.clone())?,
                PassiveRandDoubleShr::<1, F>::new(T8_THRESHOLD, parties)?,
            ))
            .execute(env)
            .await?;
            let doubles = doubles.into_iter().map(DoubleShare::into_parts).collect();
            Ok((random, doubles))
        }

        fn id(&self) -> ProtocolId {
            ProtocolId::from("RandomAndDoubles")
        }
    }

    #[tokio::test]
    async fn join_of_two_protocols_over_tls() {
        within(TEST_TIMEOUT, async {
            let parties: Vec<PartyId> = (0..T8_PARTIES).map(PartyId::from).collect();
            let runs = connected(T8_PARTIES).await.into_iter().enumerate().map(
                |(i, network)| async move {
                    let mut env =
                        GeneralEnvironment::new(network, ChaCha20Rng::seed_from_u64(i as u64));
                    let output = RandomAndDoubles.execute(&mut env).await?;
                    env.network.close().await?;
                    Ok::<_, Error>(output)
                },
            );
            let outputs = try_join_all(runs).await.unwrap();

            // Forking draws from the parent's RNG, so the shares depend on how the protocols
            // were composed: check what they open to, not the shares themselves.
            let open = |get: &dyn Fn(&RandomAndDoublesOutput) -> Share| {
                let shares: Vec<Share> = outputs.iter().map(get).collect();
                <Share as LinearShare>::secret_from_shares(&shares, &parties).unwrap()
            };
            for k in 0..T8_PARTIES - T8_THRESHOLD {
                assert_ne!(open(&|o| o.0[k].clone()), F::from(0u64));
                assert_eq!(
                    open(&|o| o.1[k].0.clone()),
                    open(&|o| o.1[k].1.clone()),
                    "double sharing {k}: the two halves hide different secrets"
                );
            }
        })
        .await;
    }

    /// A receive that times out loses nothing. The packet that arrives afterwards is
    /// received by the next receive in the same session.
    #[tokio::test]
    async fn a_timed_out_receive_does_not_lose_the_next_packet() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
            let (mut a0, mut a1) = (net0.fork(), net1.fork());

            let timed_out = a1
                .recv_from_with_timeout(party(0), Duration::from_millis(50))
                .await;
            assert!(
                matches!(&timed_out, Err(NetworkError::Timeout(Some(peer))) if *peer == party(0)),
                "expected Timeout(Some(0)), got {timed_out:?}"
            );
            let timed_out = a1.recv_any_with_timeout(Duration::from_millis(50)).await;
            assert!(
                matches!(&timed_out, Err(NetworkError::Timeout(None))),
                "expected Timeout(None), got {timed_out:?}"
            );

            a0.send_to(party(1), &packet(9)).await.unwrap();
            assert_eq!(a1.recv_from(party(0)).await.unwrap(), packet(9));
        })
        .await;
    }

    /// Sends one large packet to the other party, then waits for a reply that never comes.
    struct SendThenWait(Packet);

    impl<E: Environment> Protocol<E> for SendThenWait {
        type Output = ();

        async fn run(self, env: &mut E) -> std::result::Result<(), Error> {
            let other = env.network().other()?;
            env.network_mut().send_to(other, &self.0).await?;
            env.network_mut().recv_from(other).await?;
            Ok(())
        }

        fn id(&self) -> ProtocolId {
            ProtocolId::from("SendThenWait")
        }
    }

    /// Fails at once.
    struct Fail;

    impl<E: Environment> Protocol<E> for Fail {
        type Output = ();

        async fn run(self, _env: &mut E) -> std::result::Result<(), Error> {
            Err(Error::Input)
        }

        fn id(&self) -> ProtocolId {
            ProtocolId::from("Fail")
        }
    }

    /// A `Join` fails fast while its other branch has a 1 MiB frame under way. The frame
    /// still arrives whole, and the parent session works afterwards: the dropped branch left no
    /// partial frame on the stream.
    #[tokio::test]
    async fn a_join_that_fails_fast_leaves_the_stream_usable() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (mut net1, net0) = (nets.pop().unwrap(), nets.pop().unwrap());
            let mut big = Packet::empty();
            big.write(&vec![0xABu8; 1024 * 1024]).unwrap();

            let mut env0 = GeneralEnvironment::new(net0, ChaCha20Rng::seed_from_u64(0));
            // `try_join!` polls in the order written, so `SendThenWait` queues its frame and
            // waits. `Fail` then fails, and the `Join` drops `SendThenWait` where it is.
            let joined = Join((SendThenWait(big.clone()), Fail))
                .execute(&mut env0)
                .await;
            assert!(matches!(joined, Err(Error::Input)), "the Join must fail");
            env0.network.send_to(party(1), &packet(5)).await.unwrap();

            // Party 1 forks as its own `Join` would: the large frame is in child session 0.
            let (mut branch, _other_branch) = (net1.fork(), net1.fork());
            assert_eq!(branch.recv_from(party(0)).await.unwrap(), big);
            assert_eq!(net1.recv_from(party(0)).await.unwrap(), packet(5));
        })
        .await;
    }

    /// When a peer closes, the receives waiting for it in every session end with
    /// `ConnectionClosed`.
    #[tokio::test]
    async fn a_closing_peer_ends_the_receives_of_every_session() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
            let (mut a1, mut b1) = (net1.fork(), net1.fork());

            // Three kinds of receive: a `recv_from` in a forked session, a `recv_any` in another,
            // and a `recv_from` in the root session.
            let (in_a, in_b, in_root, closed) = tokio::join!(
                a1.recv_from(party(0)),
                b1.recv_any(),
                net1.recv_from(party(0)),
                net0.close(),
            );

            closed.unwrap();
            for result in [in_a, in_root] {
                assert!(
                    matches!(&result, Err(NetworkError::ConnectionClosed(Some(peer))) if *peer == party(0)),
                    "expected ConnectionClosed(Some(0)), got {result:?}"
                );
            }
            assert!(
                matches!(&in_b, Err(NetworkError::ConnectionClosed(None))),
                "expected ConnectionClosed(None), got {in_b:?}"
            );
        })
        .await;
    }

    /// Dropping the last handle without `close()` still closes the connections, so the
    /// peer's receive ends with `ConnectionClosed`.
    #[tokio::test]
    async fn dropping_the_network_without_close_closes_the_connections() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (mut net1, net0) = (nets.pop().unwrap(), nets.pop().unwrap());
            drop(net0);

            let result = net1.recv_from(party(0)).await;
            assert!(
                matches!(&result, Err(NetworkError::ConnectionClosed(Some(peer))) if *peer == party(0)),
                "expected ConnectionClosed(Some(0)), got {result:?}"
            );
        })
        .await;
    }

    /// After the root handle's `close()`, nothing more can arrive. A receive, in the root session
    /// or in a forked one, ends with `ConnectionClosed` instead of waiting for ever, and a send
    /// fails.
    #[tokio::test]
    async fn after_close_receives_and_sends_end_with_connection_closed() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (_net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
            let mut forked = net0.fork();
            net0.close().await.unwrap();

            for result in [
                net0.recv_from(party(1)).await,
                forked.recv_from(party(1)).await,
                net0.send_to(party(1), &packet(1)).await.map(|_| packet(0)),
            ] {
                assert!(
                    matches!(&result, Err(NetworkError::ConnectionClosed(Some(peer))) if *peer == party(1)),
                    "expected ConnectionClosed(Some(1)), got {result:?}"
                );
            }
        })
        .await;
    }

    /// `recv_any` takes from the lowest ready sender, as the simulator does. Parties 1 and 2 each
    /// send one packet in a child session, then one in the root session. Once party 0 has received
    /// both root packets, both child packets are filed, whatever order they arrived in.
    #[tokio::test]
    async fn recv_any_takes_the_lowest_ready_sender() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(3).await;
            let (mut net2, mut net1, mut net0) = (
                nets.pop().unwrap(),
                nets.pop().unwrap(),
                nets.pop().unwrap(),
            );
            let (mut a0, mut a1, mut a2) = (net0.fork(), net1.fork(), net2.fork());

            // Party 2 sends first.
            for (child, root, me) in [(&mut a2, &mut net2, 2), (&mut a1, &mut net1, 1)] {
                child.send_to(party(0), &packet(me)).await.unwrap();
                root.send_to(party(0), &packet(0)).await.unwrap();
            }
            net0.recv_from(party(2)).await.unwrap();
            net0.recv_from(party(1)).await.unwrap();

            assert_eq!(a0.recv_any().await.unwrap(), (party(1), packet(1)));
            assert_eq!(a0.recv_any().await.unwrap(), (party(2), packet(2)));
        })
        .await;
    }

    /// Dropping a forked handle retires its session: what was queued for it is freed, and a frame
    /// that arrives for it later is discarded instead of queued. The effect is visible only in the
    /// mailbox map, which this test reads directly.
    #[tokio::test]
    async fn frames_for_a_retired_session_are_discarded() {
        within(TEST_TIMEOUT, async {
            let mut nets = connected(2).await;
            let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
            let mut a0 = net0.fork();
            let a1 = net1.fork();
            let session = a1.session().clone();
            let has_mailbox = |net: &TcpNetwork| {
                let mailboxes = net.core.mailboxes.lock().unwrap();
                mailboxes.sessions.contains_key(&session)
            };

            // One frame arrives before the handle is dropped, one after.
            a0.send_to(party(1), &packet(1)).await.unwrap();
            net0.send_to(party(1), &packet(0)).await.unwrap();
            net1.recv_from(party(0)).await.unwrap();
            assert!(has_mailbox(&net1));
            drop(a1);
            assert!(!has_mailbox(&net1));

            a0.send_to(party(1), &packet(2)).await.unwrap();
            net0.send_to(party(1), &packet(0)).await.unwrap();
            net1.recv_from(party(0)).await.unwrap();
            assert!(!has_mailbox(&net1));
        })
        .await;
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
