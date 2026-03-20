use crate::net::{Network, TcpNetwork};
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

pub const DEFAULT_PROTOCOL_NAME: &str = "UNNAMED ";

#[derive(Debug, Error)]
pub enum Error {
    #[error("The protocol did not return any result.")]
    EmptyResult,
    #[error(
        "Error sending the result of the protocol executing protocol {protocol_name}: {source:?},"
    )]
    SendProtocolOutputError {
        #[source]
        source: SendError<Vec<u8>>,
        protocol_name: String,
    },
}

#[async_trait]
pub trait Protocol<N: Network>: Send + Sync {
    async fn run(&self, environment: &mut Environment<N>) -> ProtocolResult<N>;
    fn name(&self) -> String;
}

/// The result of a protocol.
///
/// The result of a protocol can be a result in bytes and/or a next protocol to execute.
pub struct ProtocolResult<N: Network> {
    /// The result of the protocol in bytes.
    pub result_bytes: Option<Vec<u8>>,
    /// Next protocol to run.
    pub next_protocol: Option<Box<dyn Protocol<N>>>,
}

impl<N: Network> ProtocolResult<N> {
    pub fn with_next(next_protocol: Box<dyn Protocol<N>>) -> Self {
        Self {
            result_bytes: None,
            next_protocol: Some(next_protocol),
        }
    }

    pub fn with_next_and_result(next_protocol: Box<dyn Protocol<N>>, result: Vec<u8>) -> Self {
        Self {
            result_bytes: Some(result),
            next_protocol: Some(next_protocol),
        }
    }

    pub fn with_result_only(result: Vec<u8>) -> Self {
        Self {
            result_bytes: Some(result),
            next_protocol: None,
        }
    }

    pub fn empty() -> Self {
        Self {
            result_bytes: None,
            next_protocol: None,
        }
    }
}

pub struct Clock {
    start: std::time::Instant,
}

impl Clock {
    pub fn read(&self) -> std::time::Duration {
        self.start.elapsed()
    }

    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

pub struct Environment<N: Network> {
    pub network: N,
    pub clock: Clock,
}

impl<N: Network> Environment<N> {
    pub fn new(network: N) -> Self {
        Self {
            network,
            clock: Clock::new(),
        }
    }
}

pub async fn evaluate_protocol<N: Network>(
    protocol: Box<dyn Protocol<N>>,
    env: &mut Environment<N>,
    protocol_output_sender: Sender<Vec<u8>>,
) -> Result<(), Error> {
    let mut next_protocol = Some(protocol);
    while let Some(protocol) = next_protocol {
        let result = protocol.run(env).await;
        next_protocol = result.next_protocol;
        if let Some(result) = result.result_bytes {
            protocol_output_sender.send(result).await.map_err(|err| {
                Error::SendProtocolOutputError {
                    protocol_name: protocol.name(),
                    source: err,
                }
            })?;
        }
    }
    Ok(())
}

pub async fn run_protocol_with_result<N: Network>(
    protocol: Box<dyn Protocol<N>>,
    env: &mut Environment<N>,
) -> Option<Vec<u8>> {
    let mut current_protocol = Some(protocol);
    let mut current_result = None;
    while let Some(protocol) = current_protocol {
        let result_exec = protocol.run(env).await;
        current_protocol = result_exec.next_protocol;
        current_result = result_exec.result_bytes;
    }
    current_result
}

pub async fn run_protocol_without_result<N: Network>(
    protocol: Box<dyn Protocol<N>>,
    env: &mut Environment<N>,
) {
    let mut current_protocol = Some(protocol);
    while let Some(protocol) = current_protocol {
        let result = protocol.run(env).await;
        current_protocol = result.next_protocol;
    }
}
