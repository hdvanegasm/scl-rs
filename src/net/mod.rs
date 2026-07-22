/// TLS connection helpers for point-to-point communication between two nodes, and the channel error
/// type shared with the simulated network.
pub mod channel;

/// Implementation of a simulated network.
///
/// This simulation uses theoretical formulas to simulate network delays. In this simulation, the
/// user of the library can tweak the parameters of the network, and the protocol execution will
/// report a time close to a real execution.
pub mod simulation;

/// Real-network backend: [`TcpNetwork`], connecting parties over mutually authenticated TLS.
pub mod tcp;

pub use tcp::TcpNetwork;

use crate::abbreviate::Abbreviate;
use crate::net::channel::ChannelError;
use crate::protocol::ProtocolId;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::{fs, io};
use std::{net::Ipv4Addr, path::Path, time::Duration};
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::server::VerifierBuilderError;
use tokio_rustls::rustls::RootCertStore;

/// Represents a party ID in the protocol.
#[derive(
    Debug, Clone, Copy, PartialEq, Hash, PartialOrd, Ord, Eq, Default, Serialize, Deserialize,
)]
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
    /// Connection closed with a remote party.
    ///
    /// If the party is known the inner value will be `Some(id)`, otherwise,
    /// the inner value would be `None`.
    #[error("the connection was closed with the remote peer {0:?}")]
    ConnectionClosed(Option<PartyId>),
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
    /// The network configuration file could not be parsed.
    #[error("error parsing the network configuration file: {0:?}")]
    ConfigParse(#[from] serde_json::Error),
    /// A certificate or private-key PEM file referenced by the configuration could not be loaded.
    #[error("error loading PEM material from the configuration: {0:?}")]
    InvalidPemFile(#[from] tokio_rustls::rustls::pki_types::pem::Error),
    /// The packet is empty.
    #[error("the packet is empty")]
    EmptyPacket,
    /// The packet is accessed with wrong index.
    #[error("accessing wrong packet index: {idx}")]
    WrongPacketIdx {
        /// Wrong index.
        idx: usize,
    },
    /// Encapsulates sending errors to a `tokio` channel.
    #[error("error sending to the tokio channel")]
    SendError(#[from] SendError<Packet>),
    /// The party waited to receive a message and reached the timeout.
    ///
    /// For a receive from a specific party, the inner value is `Some(id)`, identifying the
    /// silent party; for a receive from *any* party it is `None`, as no single peer can be
    /// blamed for the timeout.
    #[error("timed out waiting for a packet from {}", fmt_timeout_party(.0))]
    Timeout(Option<PartyId>),
}

/// Renders the awaited party of a [`NetworkError::Timeout`] for its error message: the specific
/// party when one was awaited, or "any party" for a timed-out receive from any party.
fn fmt_timeout_party(party: &Option<PartyId>) -> String {
    match party {
        Some(party) => format!("party {party:?}"),
        None => String::from("any party"),
    }
}

/// Special type for the network error.
pub type Result<T> = std::result::Result<T, NetworkError>;

/// One serialized object inside a [`Packet`], with an optional display-only type label.
///
/// `bytes` is the postcard encoding of the written value and is the only field sent over the wire.
/// `label` is local trace metadata (the [`Abbreviate::ABBREVIATION`] of the source type, or `None`
/// for values written through [`Packet::write`]); it is `#[serde(skip)]`, so it never crosses the
/// network and defaults back to `None` on the receiving side of a real connection. The simulator
/// passes packets in-process without serializing, so labels survive there for trace rendering.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Element {
    /// Postcard-encoded bytes of the written value.
    bytes: Vec<u8>,
    /// Display-only type label; never serialized (see the struct docs).
    #[serde(skip)]
    label: Option<&'static str>,
}

// Equality is defined on the payload only: the `label` is non-semantic trace metadata and is
// dropped on the wire, so a received element must still compare equal to the one that was sent.
impl PartialEq for Element {
    fn eq(&self, other: &Element) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for Element {}

impl Element {
    /// Builds an element tagged with a display label (from [`Packet::write_labeled`]).
    fn new_labeled(bytes: Vec<u8>, label: &'static str) -> Self {
        Self {
            bytes,
            label: Some(label),
        }
    }

    /// Builds an element with no label (from [`Packet::write`]); rendered as `unknown elem.`.
    fn new_unlabeled(bytes: Vec<u8>) -> Self {
        Self { bytes, label: None }
    }
}

/// Packet of information sent through a given channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Packet(Vec<Element>);

impl PartialEq<Packet> for Arc<Packet> {
    fn eq(&self, other: &Packet) -> bool {
        self.0 == other.0
    }
}

impl Packet {
    /// Creates an empty [`Packet`].
    ///
    /// This is the entry point for building a packet: start empty, then append values with
    /// [`write`](Packet::write) (or [`write_labeled`](Packet::write_labeled)) and read them back
    /// by index with [`read`](Packet::read).
    ///
    /// # Examples
    ///
    /// ```
    /// use scl_rs::net::Packet;
    ///
    /// let mut packet = Packet::empty();
    /// packet.write(&42u32).unwrap();
    /// packet.write(&"hello".to_string()).unwrap();
    ///
    /// assert_eq!(packet.read::<u32>(0).unwrap(), 42);
    /// assert_eq!(packet.read::<String>(1).unwrap(), "hello");
    /// ```
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Creates a [`Packet`] from already-built elements.
    ///
    /// Crate-internal: [`Element`] is private, and the public way to build a packet is
    /// [`empty`](Packet::empty) followed by [`write`](Packet::write) /
    /// [`write_labeled`](Packet::write_labeled). This constructor exists for the TLS backend to
    /// rebuild a packet from the elements decoded off the wire.
    fn new(buffer: Vec<Element>) -> Self {
        Self(buffer)
    }

    /// Returns the size of the [`Packet`].
    pub fn size(&self) -> usize {
        self.0
            .iter()
            .fold(0, |total_length, obj| total_length + obj.bytes.len())
    }

    /// Extract the last element added into the [`Packet`].
    pub fn pop<T>(&mut self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let bytes = self.0.pop().ok_or(NetworkError::EmptyPacket)?.bytes;
        let object = postcard::from_bytes(&bytes)?;
        Ok(object)
    }

    /// Read the element at the given index inside the [`Packet`].
    pub fn read<T>(&self, obj_idx: usize) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let element = self
            .0
            .get(obj_idx)
            .ok_or(NetworkError::WrongPacketIdx { idx: obj_idx })?;
        let object = postcard::from_bytes(&element.bytes)?;
        Ok(object)
    }

    /// Writes a serializable value at the end of the packet, **without** a type label.
    ///
    /// The element is recorded as `unknown elem.` in a trace's element breakdown. To attach the
    /// value's type label, use [`write_labeled`](Packet::write_labeled) on a type that implements
    /// [`Abbreviate`].
    pub fn write<T>(&mut self, obj: &T) -> Result<()>
    where
        T: Serialize,
    {
        let bytes_obj = postcard::to_allocvec(obj)?;
        self.0.push(Element::new_unlabeled(bytes_obj));
        Ok(())
    }

    /// Writes each value in `objs` at the end of the packet, in slice order, **without** a type
    /// label.
    ///
    /// Equivalent to calling [`write`](Packet::write) on each element in turn; the values become
    /// separate packet entries (read them back individually, not as one slice). They are recorded as
    /// `unknown elem.` in a trace's element breakdown — use
    /// [`write_many_labeled`](Packet::write_many_labeled) to tag them.
    pub fn write_many<T>(&mut self, objs: &[T]) -> Result<()>
    where
        T: Serialize,
    {
        let mut elements: Vec<_> = Vec::with_capacity(objs.len());
        for obj in objs {
            let bytes_obj = postcard::to_allocvec(obj)?;
            elements.push(Element::new_unlabeled(bytes_obj));
        }
        self.0.extend_from_slice(&elements);
        Ok(())
    }

    /// Writes each value in `objs` at the end of the packet, in slice order, tagging every element
    /// with its type's [`Abbreviate::ABBREVIATION`] label.
    ///
    /// The labeled counterpart of [`write_many`](Packet::write_many): the values become separate
    /// packet entries (read them back individually), and each contributes to the per-type breakdown
    /// shown in the trace (see [`composition`](Packet::composition)).
    pub fn write_many_labeled<T>(&mut self, objs: &[T]) -> Result<()>
    where
        T: Serialize + Abbreviate,
    {
        let mut elements: Vec<_> = Vec::with_capacity(objs.len());
        for obj in objs {
            let bytes_obj = postcard::to_allocvec(obj)?;
            elements.push(Element::new_labeled(bytes_obj, T::ABBREVIATION));
        }
        self.0.extend_from_slice(&elements);
        Ok(())
    }

    /// Writes a serializable value at the end of the packet, tagging it with its type's
    /// [`Abbreviate::ABBREVIATION`] label.
    ///
    /// The label is recorded for trace display only (see [`composition`](Packet::composition)); it
    /// is identical on the wire to a value written through [`write`](Packet::write), so switching
    /// between the two never changes what is transmitted. Use this for the typed elements a
    /// protocol sends (field elements, curve points, shares, …) so the `SEND` trace line reports
    /// what kind of data crossed the wire.
    pub fn write_labeled<T>(&mut self, obj: &T) -> Result<()>
    where
        T: Serialize + Abbreviate,
    {
        let bytes_obj = postcard::to_allocvec(obj)?;
        self.0
            .push(Element::new_labeled(bytes_obj, T::ABBREVIATION));
        Ok(())
    }

    /// Returns the bytes of the packet.
    pub fn bytes(&self) -> Vec<u8> {
        self.0.iter().fold(Vec::new(), |mut acc, obj| {
            acc.extend_from_slice(&obj.bytes);
            acc
        })
    }

    /// Returns the per-type element breakdown of the packet: each `(label, count)` pair gives a
    /// type label and how many elements carry it.
    ///
    /// Labels come from [`write_labeled`](Packet::write_labeled); elements written through
    /// [`write`](Packet::write) are grouped under `unknown elem.`. The pairs are returned in the
    /// order the labels first appear in the packet, so the breakdown is **deterministic** across
    /// runs (a `HashMap` iteration order would not be) — this matters because the simulator renders
    /// it into reproducible event traces. Used by the trace `Display` to annotate `SEND` events.
    pub fn composition(&self) -> Vec<(&'static str, usize)> {
        let mut order: Vec<&'static str> = Vec::new();
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        for element in &self.0 {
            let label = element.label.unwrap_or("unknown elem.");
            counts
                .entry(label)
                .and_modify(|count| *count += 1)
                .or_insert_with(|| {
                    order.push(label);
                    1
                });
        }
        order
            .into_iter()
            .map(|label| (label, counts[label]))
            .collect()
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
    pub(crate) base_port: u16,
    /// Time a party keeps retrying to connect to a peer before giving up with an error.
    pub(crate) timeout: Duration,
    /// Time a party waits between connection retries.
    pub(crate) sleep_time: Duration,
    /// IPs of each peer.
    pub peer_ips: Vec<Ipv4Addr>,
    /// Trusted roots used to verify peer certificates in both roles: when this node dials a peer
    /// (verifying the server) and when it accepts one (the mTLS client-certificate verifier).
    pub(crate) root_cert_store: RootCertStore,
    /// This node's certificate chain, presented both as the TLS server certificate and as the
    /// client identity for mutual authentication.
    pub(crate) server_cert: Vec<CertificateDer<'a>>,
    /// Private key associated with `server_cert`.
    pub(crate) priv_key: PrivateKeyDer<'a>,
}

impl NetworkConfig<'_> {
    /// Creates a configuration for the network from a configuration file.
    ///
    /// # Errors
    ///
    /// Returns [`NetworkError::IoError`] if the file cannot be read, [`NetworkError::ConfigParse`] if
    /// its JSON is malformed or has unknown fields, and [`NetworkError::InvalidPemFile`] if any
    /// referenced certificate or private-key PEM file cannot be loaded.
    pub fn new(path_file: &Path) -> Result<Self> {
        let raw_file: NetworkConfigFile = serde_json::from_str(&fs::read_to_string(path_file)?)?;

        let priv_key = PrivateKeyDer::from_pem_file(raw_file.priv_key)?;

        let server_cert = CertificateDer::pem_file_iter(raw_file.server_cert)?
            .map(|cert| cert.unwrap())
            .collect();

        let mut trusted_certs = Vec::new();
        for trusted_cert in &raw_file.trusted_certs {
            trusted_certs
                .extend(CertificateDer::pem_file_iter(trusted_cert)?.map(|cert| cert.unwrap()))
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
/// `Network` requires [`Send`] so that protocols generic over an `Environment` (whose associated
/// `Net: Network` threads through every layer) can be implemented with the `Protocol` trait, whose
/// `run` future is `Send`. Both `SimNetwork` and `TcpNetwork` already satisfy this.
///
/// The async methods are declared as `fn … -> impl Future<Output = …> + Send` rather than as
/// `async fn`, because a bare `async fn` in a trait leaves the returned future's auto traits
/// unspecified: an implementor could return a non-`Send` future and callers spawning a protocol on
/// a multi-threaded runtime would not compile. Implementors still write a plain `async fn`.
pub trait Network: Send {
    /// Sends a `packet` to the party with ID `party_id`.
    fn send_to(
        &mut self,
        party_id: PartyId,
        packet: &Packet,
    ) -> impl Future<Output = Result<usize>> + Send;

    /// Sends each `(party, packet)` in `messages`, returning once every packet has been handed to
    /// the network.
    ///
    /// This is the *scatter* primitive: a party fanning a (typically distinct) message out to many
    /// peers in one round. The default implementation sends sequentially; a backend where concurrent
    /// sends matter — a real network with an independent socket per peer — may override it to
    /// dispatch them concurrently. Any such concurrency must stay **within the calling task** (e.g.
    /// `futures::future::join_all`), never `tokio::spawn` or a background thread, so the method stays
    /// drivable by the deterministic simulator. The simulator stamps every send at the sender's
    /// current virtual instant regardless of dispatch order, so sequential and concurrent dispatch
    /// are equivalent there; an override only changes a real deployment.
    ///
    /// Each peer should appear at most once in `messages`; to send several packets to the same peer,
    /// call [`send_to`](Network::send_to) in sequence.
    fn send_many(
        &mut self,
        messages: &[(PartyId, Packet)],
    ) -> impl Future<Output = Result<()>> + Send {
        async move {
            for (party_id, packet) in messages {
                self.send_to(*party_id, packet).await?;
            }
            Ok(())
        }
    }

    /// Receives a `packet` from the party with ID `party_id`.
    fn recv_from(&mut self, party_id: PartyId) -> impl Future<Output = Result<Packet>> + Send;

    /// Receives a `packet` from any party returning also the party ID of the sender.
    fn recv_any(&mut self) -> impl Future<Output = Result<(PartyId, Packet)>> + Send;

    /// Receives a `packet` from a party within a `timeout`.
    ///
    /// # Errors
    ///
    /// If the current party does not receive the message within the provided `timeout`, the
    /// function will return a [`NetworkError::Timeout`] with the ID of the delayed party.
    fn recv_from_with_timeout(
        &mut self,
        party_id: PartyId,
        timeout: Duration,
    ) -> impl Future<Output = Result<Packet>> + Send;

    /// Receives a `packet` from any party within a `timeout`, returning also the party ID of the
    /// sender.
    ///
    /// # Errors
    ///
    /// If no message arrives from any party within the provided `timeout`, the function will
    /// return a [`NetworkError::Timeout`] carrying `None`, as no single party can be identified
    /// as the cause of the timeout.
    fn recv_any_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> impl Future<Output = Result<(PartyId, Packet)>> + Send;

    /// Closes the connection with the network.
    fn close(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Returns the ID of the party executing the current node.
    fn local_party(&self) -> PartyId;

    /// Method used in a **two-party** protocol to get the other party.
    ///
    /// # Errors
    ///
    /// This function returns an error if the protocol that is being executed is not a two party protocol.
    fn other(&self) -> Result<PartyId>;

    /// Returns the party IDs of the parties connected to the network.
    fn party_ids(&self) -> Vec<PartyId>;

    /// Records that a protocol scope is beginning, for backends that keep an execution trace.
    ///
    /// Called by [`Protocol::execute`](crate::protocol::Protocol::execute) right before a protocol
    /// (or sub-protocol) runs, so the trace reflects how protocols nest. The deterministic
    /// simulator records a protocol-begin event; a real-network backend keeps no trace, so the
    /// default is a no-op and behavior is unchanged.
    fn record_protocol_begin(&mut self, _protocol_name: ProtocolId) {}

    /// Records that a protocol scope has ended; the counterpart to
    /// [`record_protocol_begin`](Network::record_protocol_begin).
    fn record_protocol_end(&mut self, _protocol_name: ProtocolId) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Apple;
    impl Abbreviate for Apple {
        const ABBREVIATION: &'static str = "apple";
    }

    #[derive(Serialize)]
    struct Pear;
    impl Abbreviate for Pear {
        const ABBREVIATION: &'static str = "pear";
    }

    #[test]
    fn composition_groups_by_label_in_first_appearance_order() {
        let mut packet = Packet::empty();
        packet.write_labeled(&Apple).unwrap();
        packet.write_labeled(&Pear).unwrap();
        packet.write_labeled(&Apple).unwrap();

        // Grouped by label, ordered by where each label first appears (apple before pear), and
        // deterministic — a `HashMap`-backed implementation would not guarantee this order.
        assert_eq!(packet.composition(), vec![("apple", 2), ("pear", 1)]);
    }

    #[test]
    fn unlabeled_writes_are_reported_as_unknown() {
        let mut packet = Packet::empty();
        packet.write(&7u8).unwrap();
        packet.write_labeled(&Apple).unwrap();
        packet.write(&9u8).unwrap();

        assert_eq!(
            packet.composition(),
            vec![("unknown elem.", 2), ("apple", 1)]
        );
    }

    #[test]
    fn labels_do_not_affect_packet_equality_or_wire_round_trip() {
        // A labeled packet and a byte-identical one written unlabeled must compare equal (labels are
        // non-semantic), and a serialize/deserialize round trip (which drops the label) preserves it.
        let mut labeled = Packet::empty();
        labeled.write_labeled(&Apple).unwrap();
        let mut unlabeled = Packet::empty();
        unlabeled.write(&Apple).unwrap();
        assert_eq!(labeled, unlabeled);

        let bytes = postcard::to_allocvec(&labeled).unwrap();
        let decoded: Packet = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, labeled);
        // The label does not survive the wire, so the decoded packet reports `unknown elem.`.
        assert_eq!(decoded.composition(), vec![("unknown elem.", 1)]);
    }
}
