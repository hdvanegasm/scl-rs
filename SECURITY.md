# Security Policy

## Status and Posture

**scl-rs is a research and prototyping toolkit**. It has NOT been security-audited in any way and it is NOT intended for production use. It stays on `0.x` indefinitely with no planned `1.0` release; breaking API changes may occur between `0.x` releases.

Although this tool uses TLS to secure point-to-point channels, there's no guarantee that the channels are implemented correctly as the code has not been audited.

If you use **scl-rs** in production, use it at your own risk. The authors and contributors will not be held responsible for any damage caused by an existing vulnerability in the library.

## Threat model and known limitations

scl-rs currently makes **no side-channel resistance guarantees**. In particular:

- Some field arithmetic uses **variable-time** operations (e.g. `random_mod_vartime` in the secp256k1 fields), so timing may depend on secret values.
- The secret-generation APIs (`AdditiveSS::shares_from_secret`, `ShamirSS::shares_from_secret`, `FeldmanSS::shares_from_secret`) now require a cryptographically secure RNG by bounding their generator on `rand::CryptoRng`, so secret material cannot be seeded from a predictable generator. Lower-level sampling that is not inherently secret material (e.g. `Ring::random`, `Polynomial::random`) still accepts any `rand::Rng`; supplying secret inputs through those is the caller's responsibility.
- No formal analysis or audit has been performed on the protocols, the networking, or the cryptographic primitives.

Treat all guarantees as "best effort for research" until a release explicitly states otherwise.
