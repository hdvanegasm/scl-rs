# scl-rs

**scl-rs** is a Rust port of the [Secure Computation Library](https://github.com/anderspkd/secure-computation-library). It provides utilities for prototyping and testing secure multiparty computation (MPC) protocols. **scl-rs** offers the following features:

- Traits for finite field arithmetic.
- An implementation of the Mersenne61 field ($\mathbb{Z}_p$ with $p = 2^{61} - 1$).
- An implementation of secp256k1 using affine coordinates, along with its scalar and prime fields.
- Basic matrix and vector arithmetic over arbitrary rings.
- Basic polynomial arithmetic over arbitrary rings.
- Point-to-point communication over TCP, secured with TLS.
- Lagrange interpolation over finite fields.
- Secret sharing schemes: additive, Feldman verifiable, and Shamir.

**scl-rs** also includes a discrete-event simulation framework for MPC protocol execution. Rather than deploying actual distributed nodes across a physical network, users can configure network parameters (e.g., latency, bandwidth) and simulate protocol execution under those conditions. This is particularly useful for researchers and developers who want to test and benchmark protocols across a range of network settings without the overhead of a real deployment.

## How to run

To write your own protocol, you need to implement the trait `Protocol` defined as:

```rust
/// Represents a protocol.
#[async_trait]
pub trait Protocol<N: Network>: Send + Sync {
    /// The typed value this protocol produces.
    type Output;
    /// Behavior of the protocol when run.
    async fn run(&self, environment: &mut Environment<N>) -> Result<Self::Output, Error>;
    /// Identifier of the protocol.
    fn name(&self) -> &'static str;
}

```

A protocol declares the typed `Output` it produces, and protocols compose by calling each other:
a protocol's `run` body can call another protocol's `run` and use its typed result directly
(no serialization, fully type-checked). Network operations return a `Result`, so errors propagate
with `?`.

The communication channels send `Packet` instances, which is an encapsulation of bytes. As an example, the packets may contain information of shares, field elements, polynomials, elliptic curve points, or any other serializable type in the library. The interaction between parties is done using the functions `send_to` and
`recv_from` defined in the `Network` implementation.

An example of a simple protocol that exchanges information between two parties can be implemented as follows:

```rust
use scl_rs::net::simulation::network::SimNetwork;
use scl_rs::net::{Network, Packet};
use scl_rs::protocol::{Environment, Error, Protocol};

pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl Protocol<SimNetwork> for SendRecvProtocol {
    // The typed value this protocol produces.
    type Output = usize;

    async fn run(
        &self,
        environment: &mut Environment<SimNetwork>,
    ) -> Result<usize, Error> {
        // Creates a packet to store the information that will be sent through
        // the network.
        let mut packet = Packet::empty();

        // Stores the information in the packet.
        packet
            .write(&environment.network.local_party().as_usize())
            .unwrap();

        // Sends the packet to the other party.
        let other = environment.network.other()?;
        environment.network.send_to(other, &packet).await?;

        // Waits to receive the packet from the other party.
        let received_packet = environment.network.recv_from(other).await?;
        environment.network.close().await?;

        // The output is the other party's id, returned as a typed value.
        let their_id: usize = received_packet.read(0).unwrap();
        Ok(their_id)
    }

    fn name(&self) -> &'static str {
        "SendRecvProtocol"
    }
}
```

### Simulated execution

To run the protocol under the deterministic simulator, pair each party with an
instance of the protocol and hand them to `simulate`, along with a network
configuration and an (optionally empty) list of hooks. The simulator returns a
`SimulationOutcome` holding every party's output and its event trace.

```rust
use scl_rs::net::simulation::channel::SimpleNetworkConfig;
use scl_rs::net::simulation::runtime::simulate;
use scl_rs::net::PartyId;

let p0 = PartyId::from(0_usize);
let p1 = PartyId::from(1_usize);

let outcome = simulate(
    SimpleNetworkConfig,
    vec![(p0, SendRecvProtocol), (p1, SendRecvProtocol)],
    vec![],
);

for party in [p0, p1] {
    println!("Party {party:?} output: {:?}", outcome.outputs[&party]);
}
```

The output of the protocol will be something as follows. Each party receives the
other party's id, so party 0 outputs the id of party 1 and vice versa:

```text
Party 0 output: 1
Party 1 output: 0
```

### Distributed execution

#### Configuration

To run one node of the protocol, you need to specify the network configuration
for that node. The configuration is specified using a JSON file. The following
example shows a basic configuration.

```json
{
  "base_port": 5000,
  "timeout": 5000,
  "sleep_time": 500,
  "peer_ips": ["127.0.0.1", "127.0.0.1", "127.0.0.1"],
  "server_cert": "./certs/server_cert_p0.crt",
  "priv_key": "./certs/priv_key_p0.pem",
  "trusted_certs": ["./certs/rootCA.crt"]
}
```

The fields above are explained next:

- The `base_port`, is the port that will be used as a base to compute the actual
  port in which the party will be listening to. For a party with index `i`, the
  listening port is `base_port + i`.
- The `timeout` is the number of milliseconds a party will repeatedly try to
  connect with another party. If the timeout is reached, the application returns
  an error.
- The `sleep_time` is the number of milliseconds that a party will wait before
  trying to connect again with another party in case the connection is not
  successful.
- The `peer_ips` is the list of IPs for all the peers engaged in the protocol.
  In this case, the array is specified in such a way that the party with index
  `i` has IP `peer_ips[i]`.
- The `server_cert` is the certificate path for that node for secure communication.
- The `priv_key` is the file with the private key associated with the
  certificate in `server_cert`. This private key is used for secure communication.
- `trusted_certs` is a list of paths with trusted CA certificates. This is useful
  in executions where the certificates are self-signed.

If you want to generate the certificates and private keys for a local execution
you can execute the following command:

```text
bash gen_self_signed_certs.sh <n_parties>
```

## Missing features

- [ ] Write missing tests for all the functionalities.
- [x] ~Implement secp256k1~.
- [x] ~Implement Feldman VSS~.
- [x] ~Document the source code.~
- [x] ~Implement Shamir's secret-sharing.~
- [x] ~Implement a fake network so that the final user can prototype MPC protocols locally.~
- [x] ~Improve the serialization and deserialization to optimize the communication.~
- [ ] Improve the finite field representation to represent any field modulo $p$, for $p$ prime.

## Acknowledgements

I want to thank [HashCloak Inc](https://hashcloak.com/) for allowing me to dedicate some time to the development of this
project as part of an internal learning initiative. I also want to thank Anders Dalskov for its support and help.
