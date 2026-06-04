use crate::net::Network;
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Sender;

/// Default name for protocols.
pub const DEFAULT_PROTOCOL_NAME: &str = "UNNAMED ";

/// Error that may ocurr during a protocol execution.
#[derive(Debug, Error)]
pub enum Error {
    /// The protocol did not return a result when result was expected.
    #[error("the protocol did not return any result")]
    EmptyResult,
    /// Error sending the protocol output to the [`Sender`].
    #[error(
        "error sending the result of the protocol executing protocol {protocol_name}: {source:?}"
    )]
    SendProtocolOutputError {
        /// Source of the error.
        #[source]
        source: SendError<Vec<u8>>,
        /// Name of the protocol where the error occurs.
        protocol_name: String,
    },
}

/// Represents a protocol.
#[async_trait]
pub trait Protocol<N: Network>: Send + Sync {
    /// Behavior of the protocol when run.
    async fn run(&self, environment: &mut Environment<N>) -> ProtocolResult<N>;
    /// Identifier of the protocol.
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
    /// Creates a new protocol result with including the next protocol to execute.
    pub fn with_next(next_protocol: Box<dyn Protocol<N>>) -> Self {
        Self {
            result_bytes: None,
            next_protocol: Some(next_protocol),
        }
    }

    /// Creates a protocol result with a result in bytes and a next protocol to execute.
    pub fn with_next_and_result(next_protocol: Box<dyn Protocol<N>>, result: Vec<u8>) -> Self {
        Self {
            result_bytes: Some(result),
            next_protocol: Some(next_protocol),
        }
    }

    /// Creates a protoocl with a result but not with a next protocol.
    pub fn with_result_only(result: Vec<u8>) -> Self {
        Self {
            result_bytes: Some(result),
            next_protocol: None,
        }
    }

    /// Creates an empty protocol result.
    pub fn empty() -> Self {
        Self {
            result_bytes: None,
            next_protocol: None,
        }
    }
}

/// Clock that counts the elapsed time from a start point.
pub struct Clock {
    /// Instant in time in which the protocol starts to count.
    start: std::time::Instant,
}

impl Clock {
    /// Elapsed time since the protocol started to run.
    pub fn read(&self) -> std::time::Duration {
        self.start.elapsed()
    }

    /// Creates a new protocol in the current instant.
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

/// Environment in which a protocol is executed.
///
/// The environment includes a clock counting the duration since the simulation started, and the
/// network that is used in the protocol simulation.
pub struct Environment<N: Network> {
    /// Network in which the protocol is being executed.
    pub network: N,
    clock: Clock,
}

impl<N: Network> Environment<N> {
    /// Creates a new environment.
    pub fn new(network: N) -> Self {
        Self {
            network,
            clock: Clock::new(),
        }
    }

    /// Returns a reference to the wall clock for the protocol execution.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }
}

/// Evaluates the protocol sending the results of each protocol execution to a [`Sender`].
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

/// Executes the protocol obtaining a result at the end of the execution.
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

/// Executes the protocol without getting any result from it.
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
