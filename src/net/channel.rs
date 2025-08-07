use crate::net::Packet;
use bincode::config;
use rustls::pki_types::ServerName;
use rustls::{
    ClientConfig, ClientConnection, ConnectionCommon, ServerConfig, ServerConnection, SideData,
    StreamOwned,
};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};
use thiserror::Error;

/// Possible errors that may appear in a channel.
#[derive(Debug, Error)]
pub enum ChannelError {
    /// The party tried to connect to the other party but the timeout was reached.
    #[error("connection timeout")]
    ConnectionTimeout,

    /// Trying to read from a channel with no information.
    #[error("channel buffer is empty")]
    EmptyBuffer,

    #[error("error during serialization")]
    SerializationError(#[from] bincode::error::EncodeError),

    #[error("error during deserialization")]
    DeserializationError(#[from] bincode::error::DecodeError),

    #[error("error in IO")]
    IoError(#[from] io::Error),

    #[error("error in TLS")]
    TlsError(rustls::Error),
}

/// Specialized [`Result`] for channel errors.
pub type Result<T> = std::result::Result<T, ChannelError>;

/// Defines a channel of the network.
pub trait Channel {
    /// Closes a channel.
    fn shutdown(&mut self) -> Result<()>;
    /// Send a packet using the current channel.
    fn send(&mut self, packet: &Packet) -> Result<usize>;
    /// Receives a packet from the current channel.
    fn recv(&mut self) -> Result<Packet>;
}

impl<C, T, S> Channel for StreamOwned<C, T>
where
    C: Sized + DerefMut + Deref<Target = ConnectionCommon<S>>,
    T: Sized + Read + Write,
    S: SideData,
{
    fn shutdown(&mut self) -> Result<()> {
        self.conn.send_close_notify();
        log::info!("channel successfully closed");
        Ok(())
    }

    fn send(&mut self, packet: &Packet) -> Result<usize> {
        // First, we need to send the size of the packet to be able to know the amout
        // of bits that are being sent.
        let packet_size = packet.size();
        const USIZE_LENGTH: usize = (usize::BITS / 8) as usize;
        let mut bytes_size_packet = [0; USIZE_LENGTH];
        bincode::encode_into_slice(
            packet_size,
            &mut bytes_size_packet,
            config::standard()
                .with_big_endian()
                .with_fixed_int_encoding(),
        )
        .map_err(ChannelError::SerializationError)?;
        self.write_all(&bytes_size_packet)
            .map_err(ChannelError::IoError)?;

        // Then, we send the actual packet.
        let mut packet_bytes = Vec::new();
        bincode::serde::encode_into_slice(
            packet,
            &mut packet_bytes,
            config::standard()
                .with_big_endian()
                .with_fixed_int_encoding(),
        )?;
        self.write_all(&packet_bytes)?;
        Ok(packet.size())
    }

    fn recv(&mut self) -> Result<Packet> {
        let mut buffer_packet_size = [0; (usize::BITS / 8) as usize];
        self.read_exact(&mut buffer_packet_size)
            .map_err(ChannelError::IoError)?;
        let (packet_size, _): (usize, usize) = bincode::serde::decode_from_slice(
            &buffer_packet_size,
            config::standard()
                .with_big_endian()
                .with_fixed_int_encoding(),
        )
        .map_err(ChannelError::DeserializationError)?;

        // Then, we receive the buffer the amount bytes until the end is reached.
        let mut payload_buffer = vec![0; packet_size];
        self.read_exact(&mut payload_buffer)?;
        let (packet, _) = bincode::serde::decode_from_slice(
            &payload_buffer,
            config::standard()
                .with_big_endian()
                .with_fixed_int_encoding(),
        )?;

        Ok(Packet::new(packet))
    }
}

/// Accepts a connection in the corresponding listener.
pub(crate) fn accept_connection(
    listener: &TcpListener,
    server_conf: &ServerConfig,
) -> Result<(ServerConnection, TcpStream, usize)> {
    let (mut stream, socket) = listener.accept().map_err(ChannelError::IoError)?;
    stream
        .set_nonblocking(false)
        .map_err(ChannelError::IoError)?;

    let mut tls_conn =
        ServerConnection::new(Arc::new(server_conf.clone())).map_err(ChannelError::TlsError)?;
    let (read_bytes, write_bytes) = tls_conn
        .complete_io(&mut stream)
        .map_err(ChannelError::IoError)?;
    log::debug!("Created TLS connection: read {read_bytes} bytes, write {write_bytes} bytes");

    // Once the client is connected, we receive his ID from the current established channel.
    let mut id_buffer = [0; (usize::BITS / 8) as usize];
    loop {
        if tls_conn.wants_read() {
            tls_conn
                .read_tls(&mut stream)
                .map_err(ChannelError::IoError)?;
            tls_conn
                .process_new_packets()
                .map_err(ChannelError::TlsError)?;

            match tls_conn.reader().read_exact(&mut id_buffer) {
                Ok(()) => break Ok(()),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(err) => break Err(ChannelError::IoError(err)),
            }
        }
    }?;

    let remote_id = usize::from_le_bytes(id_buffer);
    log::info!(
        "accepted connection request acting like a server from {:?} with ID {}",
        socket,
        remote_id,
    );

    Ok((tls_conn, stream, remote_id))
}

/// Connect to the remote address as a client using the corresponding timeout. The party
/// tries to connect to the "server" (the other node) multiple times using a sleep time between calls.
/// If the "server" party does not answer within the timeout, then the function returns
/// an error.
pub(crate) fn connect_as_client(
    local_id: usize,
    remote_addr: SocketAddr,
    timeout: Duration,
    sleep_time: Duration,
    client_conf: &ClientConfig,
) -> Result<(ClientConnection, TcpStream)> {
    let start_time = Instant::now();

    // Repeatedly tries to connect to the server during the timeout.
    log::info!("trying to connect as a client to {:?}", remote_addr);
    loop {
        match TcpStream::connect(remote_addr) {
            Ok(mut stream) => {
                // We want the stream to actually block.
                stream
                    .set_nonblocking(false)
                    .map_err(ChannelError::IoError)?;

                // Create the client connection.
                let mut client_conn = ClientConnection::new(
                    Arc::new(client_conf.clone()),
                    ServerName::from(remote_addr.ip()),
                )
                .map_err(ChannelError::TlsError)?;
                let (read_bytes, write_bytes) = client_conn
                    .complete_io(&mut stream)
                    .map_err(ChannelError::IoError)?;
                log::debug!(
                    "TLS connection with {:?}: write {write_bytes} bytes, read {read_bytes} bytes",
                    remote_addr
                );

                // Send the id of the party that is connecting to the
                // server once the connection is successfull.
                client_conn
                    .writer()
                    .write_all(&local_id.to_le_bytes())
                    .map_err(ChannelError::IoError)?;
                let bytes = loop {
                    if client_conn.wants_write() {
                        match client_conn.write_tls(&mut stream) {
                            Ok(bytes) => break Ok(bytes),
                            Err(err) => break Err(ChannelError::IoError(err)),
                        }
                    }
                }?;
                log::debug!("sending ID to {:?}: {bytes} bytes", remote_addr);

                log::info!(
                    "connected successfully with {:?} using the local port {:?}",
                    remote_addr,
                    stream.local_addr().map_err(ChannelError::IoError)?
                );

                break Ok((client_conn, stream));
            }
            Err(_) => {
                let elapsed = start_time.elapsed();
                if elapsed > timeout {
                    // At this moment the enlapsed time passed the timeout. Hence we return an
                    // error. Tired of waiting for the "server" to be ready.
                    log::error!(
                        "timeout reached, server not listening from ID {local_id} to server {:?}",
                        remote_addr
                    );
                    return Err(ChannelError::ConnectionTimeout);
                }
                // The connection was not successfull. Hence, we try to connect again with the
                // "server" party.
                std::thread::sleep(sleep_time)
            }
        }
    }
}

/// This is a channel used when a party wants to connect with himself.
#[derive(Default)]
pub struct LoopBackChannel {
    /// Queue of incomming channels.
    buffer: VecDeque<Packet>,
}

impl Channel for LoopBackChannel {
    fn shutdown(&mut self) -> Result<()> {
        self.buffer.clear();
        log::info!("channel successfully closed");
        Ok(())
    }

    fn send(&mut self, packet: &Packet) -> Result<usize> {
        log::info!("sent {} bytes to myself", packet.0.len());
        self.buffer.push_back(packet.clone());
        Ok(packet.0.len())
    }

    fn recv(&mut self) -> Result<Packet> {
        log::info!("received packet from myself");
        self.buffer.pop_front().ok_or(ChannelError::EmptyBuffer)
    }
}

/// A dumy channel acting as a placeholder.
pub struct DummyChannel;

impl Channel for DummyChannel {
    fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }

    fn send(&mut self, _: &Packet) -> Result<usize> {
        Ok(0)
    }

    fn recv(&mut self) -> Result<Packet> {
        Ok(Packet::empty())
    }
}
