# scl-rs

**scl-rs** is a port of the [Secure Computation Library](https://github.com/anderspkd/secure-computation-library)
to Rust. **scl-rs** has a set of utilities for prototyping secure multiparty
computation protocols (MPC).

**scl-rs** provides the following features:

- Traits for finite field arithmetic and an implementation of Mersenne61.
- Communication point-to-point using TCP and secured using TLS.
- Support for Lagrange interpolation over finite fields.

## How to run

### Configuration

To run one node of the protocol, you need to specify the network configuration
for that node. The configuration is specified using a JSON file. The following
example shows a basic configuration.

```json
{
  "base_port": 5000,
  "timeout": 5000,
  "sleep_time": 500,
  "peer_ips": [
    "127.0.0.1",
    "127.0.0.1",
    "127.0.0.1"
  ],
  "server_cert": "./certs/server_cert_p0.crt",
  "priv_key": "./certs/priv_key_p0.pem",
  "trusted_certs": [
    "./certs/rootCA.crt"
  ]
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
- The server_cert is the certificate path for that node for secure communication.
- The `priv_key` is the file with the private key associated with the
  certificate in server_cert. This private key is used for secure communication.
- `trusted_certs` is a list of paths with trusted CA certificates. This is useful
  in executions where the certificates are self-signed.

If you want to generate the certificates and private keys for a local execution
you can execute the following command:

```text
bash gen_self_signed_certs.sh <n_parties>
```

### Usage

First, you need to load the configuration for the node using the `NetworkConfig`
struct. To create a network configuration, you provide the path of the JSON
configuration file to the constructor function of the `NetworkConfig` instance.
Once the network configuration is loaded, you create a `Network` instance that
contains the streams to the peers.

The streams for communication send `Packet` instances, which is an 
encapsulation of bytes. As an example, the packets may contain information of 
shares, field elements, polynomials, or any other serializable type in the 
library. Theinteraction between parties are done using the functions `send` and 
`recv` defined in the `Network` implementation.
