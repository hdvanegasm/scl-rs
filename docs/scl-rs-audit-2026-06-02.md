# scl-rs MPC / Threshold-Cryptography Audit — 2026-06-02

## Scope

This audit covers the cryptographic core of `scl-rs`:

- Secret sharing: `src/ss/{shamir.rs, feldman.rs, additive.rs, mod.rs}`
- Math primitives: `src/math/poly.rs`, `src/math/ring.rs`,
  `src/math/field/{mod.rs, mersenne61.rs, secp256k1_prime.rs, secp256k1_scalar.rs, naf.rs}`,
  `src/math/ec/{mod.rs, secp256k1.rs}`
- Networking / channels: `src/net/{channel.rs, mod.rs}`, `src/protocol.rs`

`scl-rs` is a primitives library (Shamir/Feldman/additive secret sharing over
`Mersenne61` and the secp256k1 scalar field, plus a TLS-based point-to-point
network). The relevant adversary model for a secret-sharing / VSS library is a
malicious party that supplies crafted shares, commitments, party indices, and
curve points over the network, and that can observe timing of honest parties
operating on secret scalars.

## Summary of Findings

| # | Title | Severity |
|---|-------|----------|
| 1 | Non-constant-time scalar multiplication and field arithmetic leak secret scalars | HIGH |
| 2 | Adversary-supplied curve points never validated as on-curve | HIGH |
| 3 | Self-declared party identity over TLS without client authentication | HIGH |
| 4 | Feldman commitments not bound to dealer/session and not checked for consistency across shares | MEDIUM |
| 5 | Shamir/Feldman party indices not validated as non-zero in the scalar field | MEDIUM |
| 6 | Feldman verification does not reject the identity / does not validate commitment-vector length consistently | MEDIUM |
| 7 | `Mersenne61` random sampling is biased (off-by-one rejection bound) | LOW |
| 8 | `Polynomial` deserialization can produce an empty polynomial that panics on use | LOW |

---

### 1. Non-constant-time scalar multiplication and field arithmetic leak secret scalars : HIGH

**Vulnerability**

The scalar multiplication routine branches on the bits of the scalar, and the
NAF encoding runs a data-dependent number of iterations:

```text
FeldmanSS::shares_from_secret()        // commitments = g^{a_i}, a_i secret coeffs
  -> C::gen().scalar_mul(coeff)
       -> Secp256k1::scalar_mul(scalar) // branches on scalar bits
            -> scalar.to_naf()          // variable iteration count + per-bit branch
```

```rust
// src/math/ec/secp256k1.rs:199
fn scalar_mul(&self, scalar: &Self::ScalarField) -> Self {
    if !self.is_point_at_infinity() {
        let mut result = Self::POINT_AT_INFINITY;
        let naf = scalar.to_naf();
        for i in (0..naf.len()).rev() {
            result = result.dbl();
            if naf.pos(i) {
                result = result.add(self);      // executed only when bit is +1
            } else if naf.neg(i) {
                result = result.sub(self);      // executed only when bit is -1
            }
        }
        result
    } else {
        Self::POINT_AT_INFINITY
    }
}
```

```rust
// src/math/field/secp256k1_scalar.rs:16
pub fn to_naf(&self) -> NafEncoding {
    let mut naf = NafEncoding::new(Self::BIT_SIZE + 1);
    let mut val = self.0;
    while !bool::from(val.is_zero()) {   // loop count depends on the secret bit length
        if test_bit(val, 0) { ... }
        ...
    }
    naf
}
```

The number of loop iterations in `scalar_mul` is `naf.len()` (fixed), but
`to_naf` itself terminates when `val` becomes zero, so its run time depends on
the position of the highest set bit of the secret scalar. The per-iteration work
in `scalar_mul` also differs between `+1`, `-1`, and `0` digits (an extra group
addition/subtraction). Both are classic timing oracles.

The same class of problem appears throughout the field layer, which uses the
explicitly variable-time `crypto-bigint` APIs and hand-rolled loops:

```rust
// src/math/field/secp256k1_scalar.rs:75
let value = Uint::<4>::random_mod_vartime(generator, &Self::MODULUS);
// src/math/field/secp256k1_scalar.rs:53
let inverse = self.0.invert_mod(&Self::MODULUS).unwrap();   // vartime inversion
// src/math/field/mersenne61.rs:21  (From<u64>)
while final_value >= u64::from(Self::MODULUS.to_limbs()[0]) { final_value -= ... }
```

**Impact**

`scalar_mul` is invoked on secret material: the secret polynomial coefficients
when producing Feldman commitments (`g^{a_i}`) and the shares themselves. An
attacker who can measure execution time (co-located process, network timing, or
a remote side channel) can recover information about the secret scalar, up to
full key recovery in the worst case. Variable-time modular inversion on secrets
(used during reconstruction / division) is similarly exploitable.

**Fix**

Use constant-time group and field operations:
- Replace the NAF/branching ladder with a constant-time Montgomery ladder or a
  fixed-window ladder with constant-time point selection, and remove the
  early-exit in `to_naf` (process a fixed number of digits).
- Use the constant-time `crypto-bigint` APIs (`invert_mod` constant-time
  variants / `CtChoice`-based selection) instead of `*_vartime` ones for any
  value derived from secrets.
- Replace the `Mersenne61` `From` subtraction loop with a constant-time
  Mersenne reduction.
- If constant-time guarantees are not the goal of this library, document the
  non-constant-time status prominently and gate secret-bearing operations.

---

### 2. Adversary-supplied curve points never validated as on-curve : HIGH

**Vulnerability**

`Secp256k1` derives `Deserialize` directly over three raw field coordinates with
no curve-equation check, and the existing on-curve predicate
`AffinePoint::is_valid()` is never called anywhere on received points
(`grep` shows the only `is_valid` callers are the unrelated Feldman share check
and the simulation channel).

```rust
// src/math/ec/secp256k1.rs:14
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq)]
pub struct Secp256k1(
    Secp256k1PrimeField,
    Secp256k1PrimeField,
    Secp256k1PrimeField,
);
```

Feldman commitments are received as `Vec<C>` and fed straight into the
verification exponentiation:

```rust
// src/ss/feldman.rs:29
pub fn is_valid(&self, owner: C::ScalarField) -> bool {
    if self.commitments.len() != self.shamir_share.degree() + 1 {
        return false;
    }
    let mut inner_prod = C::ZERO;
    for (exp, commitment) in self.commitments.iter().enumerate() {
        // commitment is adversary-controlled, never checked to be on-curve
        inner_prod = inner_prod.add(&commitment.scalar_mul(&owner.pow(exp as u64)));
    }
    inner_prod == C::gen().scalar_mul(self.shamir_share().share())
}
```

**Impact**

A malicious dealer/peer can submit points that are not on the curve. Because the
group-law formulas (`add`/`dbl`) are evaluated unconditionally, the verifier
performs arithmetic in a different group than secp256k1, which voids the binding
property of the Feldman commitment and the soundness of the VSS check. Invalid-
curve inputs are a well-known route to extracting secret scalars from honest
parties that multiply such points by secret values. (secp256k1 has cofactor 1,
so prime-order subgroup membership follows from a correct on-curve check, but the
on-curve check itself is the missing control.)

**Fix**

Validate every deserialized/received point before use: reject the identity where
the protocol forbids it, and require `y^2 == x^3 + 7` in the prime field.
Implement a custom `Deserialize` (or a checked constructor) that rejects
off-curve points, and call it at every network boundary. Apply the same to the
commitment bases and the generator if it can ever be configured.

---

### 3. Self-declared party identity over TLS without client authentication : HIGH

**Vulnerability**

Both TLS endpoints are configured with `with_no_client_auth()`, so the server
never authenticates the connecting peer. The connecting party then simply writes
its claimed ID, which the server reads and trusts:

```rust
// src/net/mod.rs:292
let client_conf = ClientConfig::builder()
    .with_root_certificates(config.root_cert_store.clone())
    .with_no_client_auth();
let server_conf = ServerConfig::builder()
    .with_no_client_auth()                       // server does not authenticate clients
    .with_single_cert(...);
```

```rust
// src/net/channel.rs:139 (server side)
match tls_conn.reader().read_exact(&mut id_buffer) { ... }
let remote_id = usize::from_le_bytes(id_buffer);  // identity is whatever the peer claims
```

The channel-to-party mapping in `TcpNetwork` is by vector index, and the ID a
peer sends is not bound to any certificate or key.

**Impact**

MPC/VSS security proofs assume authenticated point-to-point channels with fixed
identities. Here any peer that can open a TLS connection can claim to be any
party ID. A single corrupted party (or a network MITM that can reach the
listener) can impersonate honest parties, submit shares/commitments on their
behalf, and defeat the corruption-threshold assumption the protocols rely on.

**Fix**

Require mutual TLS (`with_client_cert_verifier` on the server, client certs on
the connector) and derive the peer's `PartyId` from the authenticated
certificate, not from a self-declared integer. Reject connections whose
certificate identity does not match the expected party for that slot.

---

### 4. Feldman commitments not bound to dealer/session and not checked for consistency across shares : MEDIUM

**Vulnerability**

Each `FeldmanSS` carries its own copy of `commitments`. During reconstruction,
every share is validated only against the commitment vector that travels with
it; there is no check that all shares reference the *same* dealer commitment, nor
any binding of the commitment to a dealer identity or session id.

```rust
// src/ss/feldman.rs:79
for (share, party_idx) in shares.iter().zip(party_indexes) {
    if !share.is_valid(*party_idx) {              // validates against share's OWN commitments
        return Err(ShareError::InvalidShare { party_idx: *party_idx });
    }
}
```

**Impact**

A malicious dealer can equivocate: hand different parties different commitment
vectors (each internally consistent), so honest parties accept shares lying on
different polynomials. This biases the reconstructed value / public key and
defeats the agreement property a VSS is supposed to provide. The absence of a
dealer/session binding also allows a commitment from one execution to be
replayed into another.

**Fix**

- Store the dealer's commitment vector once and verify every share against that
  single canonical vector; reject if the per-share vectors disagree.
- Bind the commitment to the dealer's party id, the session identifier, the
  threshold `t`, and the participant set (hash these into a label that the
  verification consumes), so commitments cannot be replayed across dealers or
  sessions.
- For a full VSS, add a commit-before-reveal / broadcast step so the dealer
  cannot adapt commitments after seeing honest contributions.

---

### 5. Shamir/Feldman party indices not validated as non-zero in the scalar field : MEDIUM

**Vulnerability**

Reconstruction only enforces *distinctness* of the evaluation points (via
`all_different` inside `compute_lagrange_basis`). It never enforces that the
party indices are non-zero in the field. Index `0` is the secret position
`p(0)`.

```rust
// src/ss/shamir.rs:64  secret_from_shares
// ... length / degree checks only ...
let secret = interpolate_polynomial_at(&evaluations, party_indexes, &F::ZERO)
    .map_err(ShareError::ReconstructionError)?;
```

```rust
// src/math/poly.rs:107  compute_lagrange_basis
if !all_different(nodes) { return Err(...); }   // distinctness only, no non-zero check
```

There is also no check that an index is non-zero *after reduction modulo the
field order* (e.g. a value equal to `q` would reduce to zero in the field but
appear as a distinct host integer before reduction; the scalar-field type
reduces on construction, but the API accepts raw `F` values from callers/network
without an explicit domain check).

**Impact**

If a party index of `0` is supplied (or a value congruent to `0`/colliding at
the secret position), the interpolation degenerates: a "share" can be made to
equal the secret directly, or the Lagrange evaluation at `x = 0` becomes
ill-defined / trivially controllable. This breaks the privacy and correctness
guarantees of Shamir/Feldman.

**Fix**

At ingress in `secret_from_shares` (both Shamir and Feldman) and in
`shares_from_secret`, canonicalize indices into the field and reject any index
that is zero or not pairwise distinct, in a single validation pass, before
interpolation.

---

### 6. Feldman verification does not reject the identity / commitment-length only checked on the validity path : MEDIUM

**Vulnerability**

`is_valid` returns `false` if `commitments.len() != degree + 1`, which is good,
but:

- It does not reject the identity/zero commitment `C::ZERO` or `g^0` as a
  coefficient commitment, so a dealer can place a trivial/identity commitment
  for some coefficients.
- The constant-term commitment (`commitments[0]`, the public key / `g^{secret}`)
  is never separately checked against an expected public key, so reconstruction
  cannot detect a dealer that commits to one secret but shares another consistent
  polynomial as long as each share matches its own commitments.

```rust
// src/ss/feldman.rs:30
if self.commitments.len() != self.shamir_share.degree() + 1 {
    return false;
}
```

**Impact**

Combined with finding #4, this lets a malicious dealer steer the reconstructed
public value while still passing per-share verification, and accept degenerate
commitments. On its own it is a soundness gap in the VSS check.

**Fix**

Reject identity commitments where the protocol requires non-trivial bases,
verify the constant-term commitment against the agreed group public key, and
ensure the commitment-length check is applied to the canonical dealer commitment
(see #4) rather than to per-share copies.

---

### 7. `Mersenne61` random sampling is biased (off-by-one rejection bound) : LOW

**Vulnerability**

```rust
// src/math/field/mersenne61.rs:43
fn random<R: Rng>(generator: &mut R) -> Self {
    let mut value: u64 = generator.next_u64();
    let modulus = u64::from(Self::MODULUS.to_limbs()[0]);  // 2^61 - 1
    while value > modulus {            // accepts value == modulus
        value = generator.next_u64();
    }
    Self::from(value)                  // From maps `modulus` -> 0
}
```

The rejection bound is `value > modulus`, so the accepted set is
`[0, 2^61 - 1]` (i.e. `2^61` distinct values). `From` reduces the largest
accepted value (`2^61 - 1`, equal to the modulus) to `0`, so `0` is produced
with twice the probability of every other element. The same off-by-one is in
`random_non_zero`.

**Impact**

A small, fixed statistical bias toward `0` in field elements used as secret
shares and polynomial coefficients. The bias is negligible for a 61-bit field
but is a real deviation from uniform and an avoidable defect in a crypto
primitive.

**Fix**

Use the bound `while value >= modulus` so the accepted range is exactly
`[0, 2^61 - 2]` … `[0, modulus - 1]`, i.e. one full residue system, then drop the
now-unnecessary reduction. Prefer a vetted uniform-sampling routine.

---

### 8. `Polynomial` deserialization can produce an empty polynomial that panics on use : LOW

**Vulnerability**

`Polynomial::new` rejects empty coefficient vectors, but the type derives
`Deserialize` directly over `Vec<T>`, bypassing that invariant. `evaluate` and
`degree` then assume non-emptiness:

```rust
// src/math/poly.rs:35
pub fn evaluate(&self, value: &T) -> T {
    let mut result = *self.0.last().unwrap();   // panics if empty
    ...
}
// src/math/poly.rs:49
pub fn degree(&self) -> usize { self.0.len() - 1 }   // underflow panic if empty
```

**Impact**

A peer that supplies a serialized empty polynomial (or any path that
deserializes one) can cause a panic / `usize` underflow, a denial-of-service on
the honest party.

**Fix**

Implement a custom `Deserialize` (or a `#[serde(try_from)]`) that enforces the
non-empty invariant, or validate length at every deserialization boundary before
calling `evaluate`/`degree`.

---

## General Recommendations

1. Treat every value crossing the network (points, commitments, shares, indices,
   polynomials) as adversary-controlled and validate it at ingress against its
   algebraic domain.
2. Decide explicitly whether the library targets constant-time security; if so,
   replace the entire `*_vartime` / branching arithmetic layer.
3. Add session/dealer/threshold binding to VSS commitments and require
   authenticated, identity-bound channels.
4. Add negative tests: off-curve points, zero/duplicate indices, equivocating
   dealer commitments, empty polynomials, and wrong-length commitment vectors.
