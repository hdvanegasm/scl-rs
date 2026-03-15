use crate::net::TcpNetwork;
use thiserror::Error;

pub const DEFAULT_PROTOCOL_NAME: &str = "UNNAMED ";

#[derive(Debug, Error)]
pub enum Error {
    #[error("The protocol did not return any result.")]
    EmptyResult,
}

pub trait Protocol: Send {
    fn run(&self, environment: &Environment) -> ProtocolResult;
    fn name(&self) -> String;
}

/// The result of a protocol.
///
/// The result of a protocol can be a result in bytes and/or a next protocol to execute.
pub struct ProtocolResult {
    /// Name of the protocol.
    pub name: String,
    /// The result of the protocol in bytes.
    pub result: Option<Vec<u8>>,
    /// Next protocol to run.
    pub next_protocol: Option<Box<dyn Protocol>>,
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

pub struct Environment {
    pub network: TcpNetwork,
    pub clock: Clock,
}

impl Environment {
    pub fn new(network: TcpNetwork) -> Self {
        Self {
            network,
            clock: Clock::new(),
        }
    }
}

pub fn run_protocol_with_result(protocol: Box<dyn Protocol>, env: Environment) -> Option<Vec<u8>> {
    let mut current_protocol = Some(protocol);
    let mut current_result = None;
    while let Some(protocol) = current_protocol {
        let result_exec = protocol.run(&env);
        current_protocol = result_exec.next_protocol;
        current_result = result_exec.result;
    }
    current_result
}

pub fn run_protocol_without_result(protocol: Box<dyn Protocol>, env: Environment) {
    let mut current_protocol = Some(protocol);
    while let Some(protocol) = current_protocol {
        let result = protocol.run(&env);
        current_protocol = result.next_protocol;
    }
}
