use crate::net::simulation::channel::ChannelId;
use crate::net::simulation::SimulationError;
use crate::net::Packet;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::io::{self};
use std::sync::Arc;
use std::{net::SocketAddr, time::Duration};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::error::Elapsed;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, ServerConfig};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};

/// Possible errors that may appear in a channel.
#[derive(Debug, Error)]
pub enum ChannelError {
    /// The party tried to connect to the other party but the timeout was reached.
    #[error("connection timeout")]
    Timeout(#[from] Elapsed),
    /// Trying to read from a channel with no information.
    #[error("channel buffer is empty")]
    EmptyBuffer,
    /// Error during byte serialization.
    #[error("error during serialization")]
    SerializationError(#[from] postcard::Error),
    /// Error in an IO process.
    #[error("error in IO")]
    IoError(#[from] io::Error),
    /// A TLS error wrapper.
    #[error("error in TLS")]
    TlsError(#[from] tokio_rustls::rustls::Error),
    /// The channel was not found in the set of available channels for the current node.
    #[error("channel not found: {0:?}")]
    ChannelNotFound(ChannelId),
    /// An internal error in a simulated network execution.
    #[error("error during the execution of the simulation")]
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl From<SimulationError> for ChannelError {
    fn from(err: SimulationError) -> Self {
        ChannelError::Internal(Box::new(err))
    }
}

/// Specialized [`Result`] for channel errors.
pub type Result<T> = std::result::Result<T, ChannelError>;

/// Defines a channel of the network.
#[async_trait]
pub trait Channel {
    /// Closes a channel.
    async fn close(&mut self) -> Result<()>;
    /// Send a packet using the current channel.
    async fn send(&mut self, packet: &Packet) -> Result<usize>;
    /// Receives a packet from the current channel.
    async fn recv(&mut self) -> Result<Packet>;
}

#[async_trait]
impl<T> Channel for T
where
    T: AsyncReadExt + AsyncWriteExt + Unpin + Send,
{
    async fn close(&mut self) -> Result<()> {
        self.shutdown().await?;
        log::info!("channel successfully closed");
        Ok(())
    }

    async fn send(&mut self, packet: &Packet) -> Result<usize> {
        let bytes = postcard::to_allocvec(packet)?;
        let len = (bytes.len() as u64).to_le_bytes();
        self.write_all(&len).await?;
        self.write_all(&bytes).await?;
        Ok(packet.size())
    }

    async fn recv(&mut self) -> Result<Packet> {
        let mut len_buff = [0u8; 8];
        self.read_exact(&mut len_buff).await?;
        let len = u64::from_le_bytes(len_buff) as usize;

        // Then, we receive the buffer the amount bytes until the end is reached.
        let mut payload_buffer = vec![0; len];
        self.read_exact(&mut payload_buffer).await?;
        let inner: Vec<Vec<u8>> = postcard::from_bytes(&payload_buffer)?;

        Ok(Packet::new(inner))
    }
}

/// Accepts a connection in the corresponding listener.
pub(crate) async fn accept_connection(
    listener: &TcpListener,
    server_conf: &ServerConfig,
) -> Result<(TlsStream<TcpStream>, usize)> {
    let acceptor = TlsAcceptor::from(Arc::new(server_conf.clone()));
    let (tcp_stream, socket) = listener.accept().await?;
    let mut tls_stream = acceptor.accept(tcp_stream).await?;

    let mut id_buffer = [0u8; 8];
    tls_stream.read_exact(&mut id_buffer).await?;
    let id_remote = u64::from_le_bytes(id_buffer) as usize;

    log::info!(
        "accepted connection from {:?} with ID {}",
        socket,
        id_remote,
    );

    Ok((TlsStream::from(tls_stream), id_remote))
}

/// Connect to the remote address as a client using the corresponding timeout. The party
/// tries to connect to the "server" (the other node) multiple times using a sleep time between calls.
/// If the "server" party does not answer within the timeout, then the function returns
/// an error.
pub(crate) async fn connect_as_client(
    local_id: usize,
    remote_addr: SocketAddr,
    timeout: Duration,
    sleep_time: Duration,
    client_conf: &ClientConfig,
) -> Result<TlsStream<TcpStream>> {
    // Repeatedly tries to connect to the server during the timeout.
    log::info!("trying to connect as a client to {:?}", remote_addr);
    let connector = TlsConnector::from(Arc::new(client_conf.clone()));
    let server_name = ServerName::from(remote_addr.ip());
    let stream = tokio::time::timeout(timeout, async {
        loop {
            match TcpStream::connect(remote_addr).await {
                Ok(stream) => {
                    break stream;
                }
                Err(_) => {
                    tokio::time::sleep(sleep_time).await;
                }
            }
        }
    })
    .await?;

    // TLS handshake.
    let mut tls_stream = connector.connect(server_name, stream).await?;
    tls_stream
        .write_all(&(local_id as u64).to_le_bytes())
        .await?;
    tls_stream.flush().await?;
    Ok(TlsStream::from(tls_stream))
}

/// This is a channel used when a party wants to connect with himself.
#[derive(Default)]
pub struct LoopBackChannel {
    /// Queue of incomming channels.
    buffer: VecDeque<Packet>,
}

#[async_trait]
impl Channel for LoopBackChannel {
    async fn close(&mut self) -> Result<()> {
        self.buffer.clear();
        log::info!("channel successfully closed");
        Ok(())
    }

    async fn send(&mut self, packet: &Packet) -> Result<usize> {
        log::info!("sent {} bytes to myself", packet.0.len());
        self.buffer.push_back(packet.clone());
        Ok(packet.0.len())
    }

    async fn recv(&mut self) -> Result<Packet> {
        log::info!("received packet from myself");
        self.buffer.pop_front().ok_or(ChannelError::EmptyBuffer)
    }
}
