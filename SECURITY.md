# Security Policy

## Status and Posture

**scl-rs is a research and prototyping toolkit**. It has NOT been security-audited in any way and it is NOT intended for production use. It stays on `0.x` indefinitely with no planned `1.0` release; breaking API changes may occur between `0.x` releases.

Although this tool uses TLS to secure point-to-point channels, there's no guarantee that the channels are implemented correctly as the code has not been audited.

If you use **scl-rs** in production, use it at your own risk. The authors and contributors will not be held responsible for any damage caused by an existing vulnerability in the library.

## Threat model and known limitations

scl-rs currently makes **no side-channel resistance guarantees**. In particular:

- Some field arithmetic uses **variable-time** operations (e.g. `random_mod_vartime` in the secp256k1 fields), so timing may depend on secret values.
- The secret-sharing APIs accept any `rand::Rng`; security requires the caller to supply a cryptographically secure RNG (CSPRNG). The library does not enforce this.
- No formal analysis or audit has been performed on the protocols, the networking, or the cryptographic primitives.

Treat all guarantees as "best effort for research" until a release explicitly states otherwise.
