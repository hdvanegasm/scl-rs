# scl-rs

**scl-rs** is a Rust toolkit for prototyping and testing secure multiparty computation (MPC)
protocols. It bundles the mathematical building blocks, secret-sharing schemes, and networking that
MPC protocols need — and, distinctively, a **deterministic simulator** that lets you develop and
debug a protocol locally and then run the _same_ code over a real TLS network, unchanged.

It offers:

- **Finite field arithmetic** — a `FiniteField` trait, the Mersenne-61 field
  ($\mathbb{Z}_p$ with $p = 2^{61}-1$), and the secp256k1 base and scalar fields.
- **Elliptic curves** — secp256k1 in affine coordinates.
- **Polynomials** over arbitrary rings, with Lagrange interpolation over finite fields.
- **Linear algebra** — matrices and vectors over arbitrary rings.
- **Secret sharing** — additive, Shamir, and Feldman verifiable secret sharing.
- **Networking** — point-to-point channels over TCP, secured with **mutual TLS** (mTLS, via
  `tokio-rustls`): each party authenticates the other's certificate, not just the server's.
- **A typed protocol framework** — write a protocol once as an `async` state machine; protocols
  compose by calling one another and using each other's _typed_ return values.
- **A deterministic, discrete-event simulator** — run protocols on a virtual clock with configurable
  latency and bandwidth, get reproducible results and per-party event traces, with no sockets or
  threads. The simulator and a real deployment share one `Network` trait, so a protocol written for
  one runs on the other without changes.

scl-rs began as a Rust port of Anders Dalskov's
[Secure Computation Library](https://github.com/anderspkd/secure-computation-library) (SCL) and has
since grown its own architecture — most notably the single-threaded deterministic executor behind
the simulator and the typed `async` protocol composition — so it is now better described as
_SCL-inspired_ than a faithful port.

> **Status:** research / prototyping tool. It stays on **`0.x`** (the API may change between `0.x`
> releases; there is **no planned `1.0`**) and is **not security-audited** — not intended for
> production use. See [`docs/roadmap.md`](docs/roadmap.md) for the development plan.

## Installation

```toml
[dependencies]
scl-rs = "0.4.0"
```

## Writing a protocol

A protocol implements the `Protocol` trait. It declares the typed value it produces (`Output`) and
its behavior in `run`; network operations return a `Result`, so errors propagate with `?`:

```rust
#[async_trait]
pub trait Protocol<N: Network>: Send + Sync {
    /// The typed value this protocol produces.
    type Output;
    /// Behavior of the protocol when run.
    async fn run(self, environment: &mut Environment<N>) -> Result<Self::Output, Error>;
    /// Identifier of the protocol.
    fn name(&self) -> &'static str;
}
```

Protocols communicate by sending `Packet`s — encapsulated bytes that can carry shares, field
elements, polynomials, curve points, or any serializable type — through the `send_to` / `recv_from`
methods of the `Network`. A party can also take the next packet from _whichever_ peer responds first
with `recv_any` — the basis for quorum-based protocols such as reliable broadcast (currently
available on the simulator). Because the protocol is written **generic over `N: Network`**, the very
same code runs on the simulator and over a real TLS network:

```rust
use scl_rs::net::{Network, Packet};
use scl_rs::protocol::{Environment, Error, Protocol};

pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl<N: Network> Protocol<N> for SendRecvProtocol {
    // This protocol returns the other party's id.
    type Output = usize;

    async fn run(self, env: &mut Environment<N>) -> Result<usize, Error> {
        // Put this party's id into a packet and send it to the other party.
        let mut packet = Packet::empty();
        packet.write(&env.network.local_party().as_usize()).unwrap();

        let other = env.network.other()?;
        env.network.send_to(other, &packet).await?;

        // Receive the other party's id and return it as the typed output.
        let received = env.network.recv_from(other).await?;
        env.network.close().await?;

        let their_id: usize = received.read(0).unwrap();
        Ok(their_id)
    }

    fn name(&self) -> &'static str {
        "SendRecvProtocol"
    }
}
```

A protocol can also **call another protocol** inline and use its typed result directly (no
serialization, fully type-checked) — this is how larger protocols are built from smaller ones. For
instance, a larger protocol's `run` could reuse `SendRecvProtocol` and get its typed `usize` output
straight back:

```rust
let their_id: usize = SendRecvProtocol.run(env).await?;
```

### Running on the simulator

Pair each party with an instance of the protocol and hand them to `simulate`, along with a network
configuration and an (optionally empty) list of hooks. The simulator drives every party on a virtual
clock and returns a `SimulationOutcome` with each party's typed output and its event trace:

```rust
use scl_rs::net::simulation::channel::SimpleNetworkConfig;
use scl_rs::net::simulation::runtime::simulate;
use scl_rs::net::PartyId;

let p0 = PartyId::from(0_usize);
let p1 = PartyId::from(1_usize);

let outcome = simulate(
    SimpleNetworkConfig,
    vec![p0, p1],
    |_| SendRecvProtocol,
    vec![],
);

for party in [p0, p1] {
    println!("Party {} output: {:?}", party.as_usize(), outcome.outputs[&party]);
}
```

Each party receives the other party's id, so party 0 outputs the id of party 1 and vice versa:

```text
Party 0 output: 1
Party 1 output: 0
```

`SimpleNetworkConfig` uses instantaneous channels; supply your own `NetworkConfig` to model latency,
bandwidth, and other parameters, and the reported timings will approximate a real deployment under
those conditions.

### Running on a real network

The same `SendRecvProtocol` runs unchanged over real TLS. Every party runs the same binary, passing
its own party id and configuration file (hard-coded to party 0 below; in practice you would read them
from command-line arguments):

```rust
use scl_rs::net::{NetworkConfig, TcpNetwork};
use scl_rs::protocol::{Environment, Protocol};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This node's party id (0, 1, ...).
    let my_id = 0;

    // Load this node's peers, ports, and certificates from a JSON file.
    let config = NetworkConfig::new(Path::new("net_config_p0.json"))?;

    // `create` performs the TLS handshake with every peer, so it is async.
    let network = TcpNetwork::create(my_id, config).await?;
    let mut env = Environment::new(network);

    // The very same protocol that ran on the simulator now runs over real TLS.
    let output = SendRecvProtocol.run(&mut env).await?;
    println!("Party {my_id} output: {output}");
    Ok(())
}
```

`SendRecvProtocol` is a two-party protocol, so launch **two** processes: party 0 with `my_id = 0` and
`net_config_p0.json`, and party 1 with `my_id = 1` and `net_config_p1.json`. `create` blocks until
every peer has connected, so both processes must be started.

#### Network configuration

Each node reads a JSON configuration describing the parties and its own TLS material. Party 0's
`net_config_p0.json` for the two-party run above is:

```json
{
  "base_port": 5000,
  "timeout": 5000,
  "sleep_time": 500,
  "peer_ips": ["127.0.0.1", "127.0.0.1"],
  "server_cert": "./certs/server_cert_p0.crt",
  "priv_key": "./certs/priv_key_p0.pem",
  "trusted_certs": ["./certs/rootCA.crt"]
}
```

Party 1's `net_config_p1.json` is identical except for `server_cert` (`server_cert_p1.crt`) and
`priv_key` (`priv_key_p1.pem`). Both parties run on `127.0.0.1`, distinguished by port
(`base_port + i`).

- `base_port` — the base listening port. The party with index `i` listens on `base_port + i`.
- `timeout` — milliseconds a party keeps retrying to connect to a peer before giving up with an error.
- `sleep_time` — milliseconds a party waits between connection retries.
- `peer_ips` — the IP of every party **including this node**, indexed by party id: party `i` has IP
  `peer_ips[i]`, and the number of entries is the number of parties. Party `i` binds its own listener
  on `peer_ips[i]`.
- `server_cert` — this node's certificate. Connections are **mutually authenticated** (mTLS), so it
  is presented both as the node's TLS server certificate and as its client identity when it dials a
  peer.
- `priv_key` — the private key associated with `server_cert`.
- `trusted_certs` — trusted CA certificates used to verify peers in **both** roles (a server
  verifying a connecting client and a client verifying the server); useful when certificates are
  self-signed.

#### Generating certificates

For a local run you can generate a self-signed root CA plus one certificate and key per party with the
bundled script:

```text
bash gen_self_signed_certs.sh <n_parties>
```

It writes `rootCA.crt` and, for each party `i`, a CA-signed `server_cert_p{i}.crt` and its
`priv_key_p{i}.pem`, into the `certs/` directory referenced by the configuration above. Each leaf
certificate carries both the server- and client-authentication usages, so the same file serves as a
node's TLS server certificate and its client identity under mTLS. The certificates are valid for
`127.0.0.1` only (their subject alternative name is that IP), so a multi-host deployment needs
certificates whose subject alternative name matches each host's address.

## Status and roadmap

scl-rs is under active development and stays on `0.x` indefinitely; the public API may change between
`0.x` releases, and there is no planned `1.0` (the unaudited posture is carried by this disclaimer,
not the version number). The plan toward a stable, well-baked `0.x` API — API stabilization, security
hardening, examples, and remaining features — is tracked in [`docs/roadmap.md`](docs/roadmap.md).

## Acknowledgements

I want to thank [HashCloak Inc](https://hashcloak.com/) for allowing me to dedicate some time to the
development of this project as part of an internal learning initiative. I also want to thank Anders
Dalskov for his support and help, and for the [Secure Computation
Library](https://github.com/anderspkd/secure-computation-library) that inspired this work.
