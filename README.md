# scl-rs

[![Documentation](https://docs.rs/scl-rs/badge.svg)](https://docs.rs/scl-rs/)
[![Crates.io](https://img.shields.io/crates/v/scl-rs.svg)](https://crates.io/crates/scl-rs)

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
- **Secret sharing** — additive, Shamir, and Feldman verifiable secret sharing, unified by a
  `LinearShare` trait that exposes their local, communication-free linear operations (adding two
  shares, and adding or multiplying by public constants) — the arithmetic layer MPC protocols build
  on.
- **Networking** — point-to-point channels over TCP, secured with **mutual TLS** (mTLS, via
  `tokio-rustls`): each party authenticates the other's certificate, not just the server's.
- **A typed protocol framework** — write a protocol once as an `async` state machine; protocols
  compose by calling one another and using each other's _typed_ return values. Ships with generic
  protocols for dealing and opening shares that work over any `LinearShare` scheme.
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
scl-rs = "0.12.1"
```

### Releases vs. `main`

The supported way to use scl-rs is the latest **released crate on crates.io**; the
installation instructions above refer to it.

The **`main` branch** is the development tip. It is kept green on CI (`fmt`, `clippy`,
`test`, `doc`) on every commit, so it builds and its tests pass — but it is **unreleased**,
and its public API may **change between commits** without a version bump or notice.

To use unreleased work, depend on a specific commit and pin the `rev`:

```toml
[dependencies]
scl-rs = { git = "https://github.com/hdvanegasm/scl-rs", rev = "<commit-sha>" }
```

Pinning a `rev` (rather than the bare branch) means a later `main` commit can't change the
API under you until you choose to update. Passing CI is the only guarantee `main` carries;
the usual research / prototyping, not-audited status still applies.

## Examples

In the `examples/` folder, you will find a set of examples that show how to use the library.
To run an example in the file `<example_name>.rs`, you must run the command

```text
cargo run --example <example_name>
```

## Bandwidth profiling

Communication — not computation — is what an MPC protocol pays for, and in a protocol built by
composing sub-protocols it is rarely obvious *which* phase spends it. The simulator answers that
question without any instrumentation on your part: it already records every `SendData` event and
every `Protocol::execute` entry/exit, so after a run `SimulationOutcome::bandwidth_tree_for(party)`
reconstructs the call tree of (sub-)protocol invocations and attributes each byte sent to the
innermost protocol that was running when it went out. `ProtocolBandwidthTree::write_folded` then
emits [Brendan Gregg's folded-stacks format](https://www.brendangregg.com/flamegraphs.html), which
[inferno](https://github.com/jonhoo/inferno) renders as a flamegraph — width is bytes, not time:

![Bandwidth flamegraph of the secure-covariance example](docs/cov_bandwidth.svg)

That is `examples/secure_covariance/main.rs`: five parties computing the covariance of two parties'
private datasets with the DN07 protocols, 3,633 bytes on the wire in total. The graph is read as a
cost breakdown of the protocol's own structure. Sharing the two input vectors (`ShareX`, `ShareY`)
is 13% of the traffic. `Preprocessing` — generating the six multiplication triples, itself a tower of
`PassiveRandShr` / `PassiveRandDoubleShr` dealing plus a `PassiveShamirTriple` batched open — is the
single largest phase at 57%. The online `PassiveShamirMul` that *spends* those triples costs 25%,
essentially all of it the one batched open of the masked values, and revealing the result is the
last 5%.

The actionable insight is the one you would want from a profiler: the majority of the bandwidth sits
in a phase that does not depend on the inputs at all, so it can be moved off the critical path and
run before the data even exists. Generate the folded file for the example yourself with

```text
cargo run --example secure_covariance
inferno-flamegraph --countname bytes < covariance_bandwidth.folded > covariance_bandwidth.svg
```

Concatenating several parties' trees into one file is valid — renderers sum duplicate paths, so the
picture becomes network-wide totals per call path. See also `examples/bandwidth_flamegraph.rs` (the
minimal introduction) and `examples/secure_stats_flamegraph.rs` (nested phases).

## Writing a protocol

A protocol implements the `Protocol` trait. It declares the typed value it produces (`Output`) and
its behavior in `run`; network operations return a `Result`, so errors propagate with `?`:

```rust
#[async_trait]
pub trait Protocol<E: Environment>: Send + Sync {
    /// The typed value this protocol produces.
    type Output;
    /// Behavior of the protocol when run.
    async fn run(self, environment: &mut E) -> Result<Self::Output, Error>;
    /// Identifier of the protocol.
    fn id(&self) -> ProtocolId;
}
```

Protocols communicate by sending `Packet`s — encapsulated bytes that can carry shares, field
elements, polynomials, curve points, or any serializable type — through the `send_to` / `recv_from`
methods of the `Network`. A party can also take the next packet from _whichever_ peer responds first
with `recv_any` — the basis for quorum-based protocols such as reliable broadcast, where a party acts
on the first quorum of responses and must not block on the peers that stay silent. `recv_any` is
available on both the simulator and a real TLS network. Because the protocol is written **generic
over `E: Environment`** (and therefore over any `Network` the environment wraps), the very same code
runs on either without changes:

```rust
use scl_rs::net::{Network, Packet};
use scl_rs::protocol::{Environment, Error, Protocol, ProtocolId};

pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SendRecvProtocol {
    // This protocol returns the other party's id.
    type Output = usize;

    async fn run(self, env: &mut E) -> Result<usize, Error> {
        // Put this party's id into a packet and send it to the other party.
        let mut packet = Packet::empty();
        packet.write(&env.network().local_party().as_usize())?;

        let other = env.network().other()?;
        env.network_mut().send_to(other, &packet).await?;

        // Receive the other party's id and return it as the typed output.
        let received = env.network_mut().recv_from(other).await?;
        env.network_mut().close().await?;

        let their_id: usize = received.read(0)?;
        Ok(their_id)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SendRecvProtocol")
    }
}
```

The `ProtocolId` a protocol reports for itself is what labels its scope in a simulation trace: the
simulator brackets every `execute` call with a `PROTOCOL_BEGIN` / `PROTOCOL_END` pair carrying that
id, which is how nested sub-protocol calls become visible as an indented tree.

A protocol can also **call another protocol** inline and use its typed result directly (no
serialization, fully type-checked) — this is how larger protocols are built from smaller ones. For
instance, a larger protocol's `run` could reuse `SendRecvProtocol` and get its typed `usize` output
straight back:

```rust
let their_id: usize = SendRecvProtocol.execute(env).await?;
```

### Running on the simulator

Pair each party with an instance of the protocol and hand them to `simulate`, along with a network
configuration and an (optionally empty) list of hooks. The simulator drives every party on a virtual
clock and returns a `SimulationOutcome` with each party's typed output and its event trace:

```rust
use scl_rs::net::simulation::channel::SimpleNetworkConfig;
use scl_rs::net::simulation::simulator::simulate;
use scl_rs::net::PartyId;
use scl_rs::protocol::GeneralEnv;
use rand::{rngs::StdRng, SeedableRng};

let p0 = PartyId::from(0_usize);
let p1 = PartyId::from(1_usize);

let outcome = simulate(
    SimpleNetworkConfig::default(),
    vec![p0, p1],
    // Per-party protocol factory.
    |_| SendRecvProtocol,
    // Per-party environment factory: wrap the simulated network with a session RNG.
    |_, net| GeneralEnv::new(net, StdRng::from_rng(&mut rand::rng())),
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

`SimpleNetworkConfig` applies one channel configuration to every inter-party link, and makes a
party's link to itself instantaneous. `::default()` uses the default TCP parameters (1 Mbps, 100 ms
RTT); `SimpleNetworkConfig::lan()` and `SimpleNetworkConfig::wan()` are loss-less presets for a
1 Gbps/1 ms LAN and a 100 Mbps/100 ms WAN. Supply your own `NetworkConfig` to vary latency,
bandwidth, and other parameters per link, and the reported timings will approximate a real
deployment under those conditions.

#### Event traces and element labels

Each party's `SimulationTrace` (in `outcome.traces[&party]`) prints as an indented, per-party,
virtual-time-ordered log via `Display`. `SEND` and `RECV` lines report not just the byte size but a
breakdown of _what kind_ of elements the packet carried:

```text
SEND    2 -> 0 (1024 bytes: 1 EC elem., 2 Shamir shr., 4 field elem.)
RECV    2 <- 0 (1024 bytes: 1 EC elem., 2 Shamir shr., 4 field elem.)
```

On a `RECV` line the labels are the _sender's_, carried in-process by the simulator (which never
serializes packets); they describe what was sent, not what the receiver chooses to deserialize each
element into.

The breakdown comes from how a protocol writes into a `Packet`. `Packet::write` records an element
as `unknown elem.`; `Packet::write_labeled` tags it with the type's label, declared through the
`Abbreviate` trait:

```rust
use scl_rs::abbreviate::Abbreviate;

struct PublicKey;

impl Abbreviate for PublicKey {
    const ABBREVIATION: &'static str = "pub. key";
}
```

The built-in field, curve, polynomial, vector, and secret-sharing types already implement
`Abbreviate`. Labels are display-only metadata: they are never serialized onto the wire (so they
cost no bandwidth and do not affect packet equality), and the breakdown is therefore available on
the simulator, where packets are passed in-process.

#### Hooks

The last argument to `simulate` is a list of hooks. A hook implements `TriggeredHook`, names the
event type it reacts to (or `None` for every event), and runs each time a matching event is appended
to a party's trace — the extension point for measuring a run or for steering it.

`MetricHook` is the built-in example: it reacts to `SendData` and totals the payload **bytes** each
party puts on the wire, in aggregate and per sender.

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use scl_rs::net::simulation::hook::MetricHook;

let metrics = Arc::new(MetricHook::new(
    Arc::new(Mutex::new(0_usize)),
    Arc::new(Mutex::new(HashMap::new())),
));

simulate(
    SimpleNetworkConfig::default(),
    vec![p0, p1],
    |_| SendRecvProtocol,
    |_, net| GeneralEnv::new(net, StdRng::from_rng(&mut rand::rng())),
    vec![metrics.clone()],
);

println!("total bytes sent: {}", metrics.total_data());
println!("bytes sent by party 0: {:?}", metrics.total_data_by(&p0));
```

Because `simulate` takes each hook as an `Arc<dyn TriggeredHook>` and a hook only ever sees `&self`,
its counters live behind a `Mutex`; register a clone of the `Arc` and read the totals back through it
once the run has finished.

### Running on a real network

The same `SendRecvProtocol` runs unchanged over real TLS. Every party runs the same binary, passing
its own party id and configuration file (hard-coded to party 0 below; in practice you would read them
from command-line arguments):

```rust
use scl_rs::net::{NetworkConfig, TcpNetwork};
use scl_rs::protocol::{GeneralEnv, Protocol};
use rand::{rngs::StdRng, SeedableRng};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This node's party id (0, 1, ...).
    let my_id = 0;

    // Load this node's peers, ports, and certificates from a JSON file.
    let config = NetworkConfig::new(Path::new("net_config_p0.json"))?;

    // `create` performs the TLS handshake with every peer, so it is async.
    let network = TcpNetwork::create(my_id, config).await?;
    let mut env = GeneralEnv::new(network, StdRng::from_rng(&mut rand::rng()));

    // The very same protocol that ran on the simulator now runs over real TLS.
    let output = SendRecvProtocol.execute(&mut env).await?;
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

## Benchmarks

We compare the simulated execution time against a real mutually-authenticated-TLS run over a
loopback link shaped with the `tc netem` Linux utility to match the simulated network parameters.
The simulator is meant to be a _useful_ predictor, not a perfect one: the goal is that "I ran this in
the simulator and it took X" lets you expect a real run to behave similarly. That fidelity holds _for
protocols that suspend only through the abstractions the simulator models_ (the `Network` trait), and
only in the regimes measured below — which do not come out equal.

The numbers below come from the harness in [`benches/comparison`](benches/comparison/), which runs
each of the three regimes 50 times over shaped loopback against a **round-dominated** protocol
(`PingPong`, 30 sequential 1 KB round trips) and a **bandwidth-dominated** one (`BulkTransfer`, a
single 0.5–1 MB message). Figures are for the connection initiator, as the median over 50
repetitions unless stated. See [`benches/comparison/README.md`](benches/comparison/README.md) for the
per-scenario figures, the full results table, and how to reproduce them:

```bash
./benches/comparison/run_all.sh        # ~26 min; shapes loopback with tc, restores it on exit
python3 benches/comparison/plot.py     # one figure per scenario, plus a summary table
```

**The split that runs through every regime is round- versus bandwidth-dominated.** A protocol whose
running time is a sum of round trips is predicted within a few percent in _every_ regime measured
here — including under packet loss (the round-dominated `PingPong` came in at −0.7 % at 1 % loss).
Round trips are priced by the RTT term, which every regime gets right; the throughput terms, where
the model's uncertainty lives, barely enter. The error lives on the bandwidth-dominated side.

**Bandwidth-limited and loss-less: a small, systematic under-prediction.** Over a 100 ms RTT,
1 Mbit/s, loss-less link the simulator under-predicts by 3.4 % for `PingPong` and 10.6 % for
`BulkTransfer` (real 3.63 s and 4.93 s; simulated 3.51 s and 4.41 s). The simulator prices an
idealized steady-state throughput; the shaped link's real goodput falls a little short of it — from
protocol framing and the `tc` rate limiter's own accounting that the model does not reproduce — and
the shortfall grows with bytes moved, so the bulk transfer's gap exceeds the latency-bound
ping-pong's. The real distributions are extremely tight (coefficient of variation 0.0–0.1 % over 50
runs), so this is a genuine bias, not measurement noise. (It is *not* TCP slow start: at 1 Mbit/s the
bandwidth-delay product is 12.5 KB, below Linux's initial congestion window, so the connection
reaches full rate within the first round trip.)

**Window-limited and loss-less: the model's form holds, and `window_size` must be calibrated.** When
the bandwidth-delay product exceeds the TCP window, the window alone sets the rate (`window · 8 /
RTT`) and the configured bandwidth is ignored. Over a 100 ms RTT, 100 Mbit/s link (BDP 1.25 MB, far
above the 64 KiB default window) the nominal prediction over-predicts by 2.3 % (`PingPong`) and 7.0 %
(`BulkTransfer`). Recovering the window Linux actually delivered from the real bulk transfers — the
procedure the `WindowSize` documentation prescribes — yields 70,422 bytes, only 7.5 % above the
default. Re-simulating with it drives the bulk-transfer error to zero, but that is circular (the
window was fit to that transfer's own median), and this run has no independent window-bound workload
to cross-check it: the round-dominated protocol is latency-bound and barely moves — +2.3 % to
+2.1 % — whatever the window. So the run demonstrates the calibration recipe rather than pinning the
number down; a real cross-check needs differently-shaped runs, as the `WindowSize` documentation
describes. The default and delivered windows are close on this host, so the miss is mild; where they
diverge more, leaving the default in place misprices every message.

**Lossy (`package_loss` above zero): a standard formula, applied outside its validity domain.** The
`√(3/2p)` square-root formula is the widely-used one from the literature, and it is implemented
faithfully; what does not carry over is the question we ask of it. It is stated there as an
_asymptotic_ result, valid as `p → 0`, for the _almost-sure mean_ throughput of a _long-lived_ TCP
_Reno_ flow — whereas scl-rs reads a single number off it to price one short message. Two
consequences show up in the data. First, because the formula enters only through throughput, its
error is **confined to bandwidth-dominated protocols**: at 1 % loss the round-dominated `PingPong` is
predicted within 0.7 %, while `BulkTransfer` on the same link is over-predicted by ~400 % against the
median (~211 % against the mean). Second — more decisively than any error bar — the real runs are not
reproducible: 50 identical 1 % loss `BulkTransfer` trials spanned 0.73 s to 7.11 s, precisely the
deviation-from-the-mean that the formula makes no claim about. Treat simulated timings for a
bandwidth-dominated protocol on a lossy channel as order-of-magnitude only; the `PackageLoss`
documentation gives the conditions and the citation.

Where the model does not fit, the results aren't silently wrong: a `tc`-shaped validation run
surfaces the gap. The untested axis that matters most is concurrency: every measurement so far keeps
a single message in flight, so the per-link independent-bandwidth assumption has never been stressed
by simultaneous transfers.

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
