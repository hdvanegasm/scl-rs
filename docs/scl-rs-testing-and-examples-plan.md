# scl-rs — Testing & Examples Plan

**Date:** 2026-06-21, updated 2026-11-12.
**Against:** the 0.15.0 release (`main` at `8c34ace` plus the 0.15.0 changes). The plan was first
written against v0.4.1; §2 and §3 keep its items, each now marked done, open or declined.
**How this was checked:** the test names and counts below come from reading `tests/` and the inline
`#[cfg(test)]` modules on 2026-11-12. `cargo test --all-features --no-fail-fast` passed 288 tests
with 5 ignored, the doctests included (21 passed, 5 ignored).

---

## 1. Coverage snapshot

What the suite exercises, by module. Counts are `#[test]` / `#[tokio::test]` functions, property
tests included.

| Area | State | Where |
|---|---|---|
| `net/simulation` | **Strong** | `tests/simulator.rs` (20): send/recv, ordering, chained stages, bandwidth and latency timing, trace rendering and per-party arrows, broadcast, `recv_any` quorum, hooks, reproducibility, deadlock reports. `tests/recv_timeout.rs` (5): virtual-time timeouts. `tests/golden.rs` (11): byte-for-byte snapshots of the simulator examples and four timing shapes (traces, exact timestamps, bandwidth trees, metrics). `tests/trace_tree.rs` (4): traces and bandwidth trees under concurrency. `tests/occupancy.rs` (3): link occupancy. Inline: `switchboard/mod.rs` (9), `channel.rs` (4). |
| `net` sessions | **Strong** | Inline in `src/net/session.rs` (21): `SessionId` construction, ordering, `Display`, `digest`. |
| `net` (real TLS) | **Strong** | Inline in `src/net/tcp.rs` (21): public API end to end, handshake, mTLS rejection, multi-party `recv_any`, closed peers, sessions over one link, early arrival, a `Join` over TLS, receive and send cancel safety, close and drop, the loop-back after `close()`, and `PeerFailed` for an undecodable frame, a cut stream and a failed write (these three run over in-memory streams, not TLS). Config and PEM errors. |
| `net::Packet` | **Decent** | `tests/packet.rs` (7) and inline in `src/net/mod.rs` (3): round trips, malformed reads, element labels. |
| `math/field/*` | **Strong** | `tests/mersenne61.rs` (23), `tests/prime_secp256k1.rs` (18), `tests/scalar_secp256k1.rs` (19): field laws as property tests, inverses, division by zero. |
| `math/ec` | **Decent** | `tests/secp256k1.rs` (15) and inline in `src/math/ec/secp256k1.rs` (4): group law, scalar multiplication, on-curve checks. `tests/naf.rs` (3). |
| `math/poly` | **Decent** | `tests/poly.rs` (3): interpolation as a property test, empty and length-mismatch inputs as typed errors. `tests/lagrange.rs` (1). |
| `math/matrix` | **Decent** | `tests/matrix.rs` (15): constructors, shapes, bounds, dimension errors, non-square products, matrices of shares. |
| `math/vector` | **Decent** | `tests/vector.rs` (15): ops, dimension errors, vectors of shares. |
| `ss/additive` | **Thin** | `tests/additive.rs` (2): reconstruction and `LinearShare` dealing. |
| `ss/shamir` | **Decent** | `tests/shamir.rs` (5): threshold floor, subset invariance as a property test, threshold-aware dealing, serialization. Three error paths untested (§2, Tier 1). |
| `ss/feldman` | **Strong** | `tests/feldman.rs` (2) and inline in `src/ss/feldman.rs` (5): round trip, tamper detection naming the party, off-curve and wrong-length commitments, length mismatch. |
| `protocol` | **Decent** | `tests/protocol_share.rs` (6): passive deal and open. `tests/passive_shamir.rs` (6): DN07 `Random`, `Double-Random`, triples and Beaver multiplication. `tests/schnorr_pok.rs` (4). `tests/concurrent.rs` (13): `Join`, `JoinAll`, boxed protocols, timing, session isolation, determinism, deadlock reports. All protocols are generic over `E: Environment`. |

The two structural gaps of June are closed: there are property tests (`proptest`, in nine test
files), and the rejection paths have negative tests (Tier 3). The empty `tests/net.rs` stub is gone.

---

## 2. Testing plan

Six tiers, ordered by leverage-per-effort. Tiers 1–3 are unit and property work with no protocol
machinery; 4–6 build on the simulator and the TLS paths.

### Tier 1 — Close the zero-coverage holes

Correctness tests against public APIs, targeting outputs rather than representations.

- **Shamir** (`tests/shamir.rs`):
  - Round trip and subset invariance. ✅ **done** — `reconstruction_is_subset_invariant_across_configs`
    (property test over `n`, `t` and two random `(t + 1)`-subsets).
  - Threshold floor. ✅ **done** — `too_few_shares_is_not_enough` (every subset smaller than
    `t + 1` returns `NotEnoughShares`).
  - `EvalAndShareLenMismatch` when `shares.len() != party_indexes.len()`. ❌ **open** — no test
    names the variant (`src/ss/shamir.rs:120`).
  - `SharesWithDifferentDegree` when shares carry mixed degrees. ❌ **open** — no test names the
    variant (`src/ss/shamir.rs:128`).
  - Duplicate `party_indexes` surface as `ReconstructionError` (`NotAllDifferentInterpolation`
    from the poly layer). ❌ **open** — untested.
- **Feldman** (`tests/feldman.rs`, inline in `src/ss/feldman.rs`): ✅ **done** — `round_trip`;
  `honest_shares_are_valid`; `tampered_share_is_detected` (`InvalidShare` names the party);
  `wrong_commitment_vector_length_is_rejected`; `length_mismatch_is_rejected`.
- **Matrix** (`tests/matrix.rs`): ✅ **done** — `from_vec_dimension_mismatch`,
  `constructor_shapes` (also `is_square`), `get_out_of_bounds_is_none`,
  `get_returns_row_major_element`, `get_mut_writes_through`,
  `scalar_mult_does_not_mutate_but_in_place_does`.
  - **Design flag, still open:** `scalar_mult` still takes `&mut self` without mutating
    (`src/math/matrix.rs:167`). The test pins the behaviour; changing the receiver to `&self` is a
    breaking change for a later 0.x.

### Tier 2 — Property-based tests (`proptest`)

✅ **done** (`proptest` is a dev-dependency):

- **Field laws** over Mersenne61 and the secp256k1 prime and scalar fields: associativity,
  commutativity, distributivity, identities, inverses, `a - a == 0`, `a * a⁻¹ == 1`, division
  (`tests/mersenne61.rs`, `tests/prime_secp256k1.rs`, `tests/scalar_secp256k1.rs`).
- **Shamir:** subset invariance (Tier 1 above).
- **Poly:** `interpolation_recovers_polynomial` (random polynomial of degree below 16, evaluated
  then interpolated at a random point and at 0).
- **Serialization round trips** (`postcard`): `Packet`, `ShamirSS`, `FeldmanSS`, curve points,
  the secp256k1 fields. Not covered: `Mersenne61` has no round trip of its own; it is an alias of
  the same `Fp` type as the secp256k1 fields since 0.14.0.

### Tier 3 — Negative / adversarial tests (security rejection paths)

✅ **done**, mostly in 0.7.0:

- **On-curve rejection:** `off_curve_point_is_rejected` (`tests/secp256k1.rs`) and the inline
  `src/math/ec/secp256k1.rs` tests. The sharper question, whether Feldman checks that dealer
  commitments are on the curve, was answered by adding the check:
  `off_curve_commitment_is_rejected` (`src/ss/feldman.rs`).
- **Packet malformed reads:** `pop_on_empty_packet_is_rejected`, `read_out_of_range_index_is_rejected`,
  `read_wrong_type_is_a_serialization_error`, `pop_wrong_type_is_a_serialization_error`.
- **Poly guards as errors, not panics:** the `assert!`s became typed errors (`EmptyInterpolation`,
  `LengthMismatch` in `src/math/poly.rs`), tested by `interpolate_with_empty_input_is_rejected`
  and `interpolate_with_length_mismatch_is_rejected`.
- **CSPRNG bound:** superseded. Share generation now requires `CryptoRng`
  (`ShamirSS::shares_from_secret`, `FeldmanSS::shares_from_secret`, `LinearShare`), so the
  footgun the documentation test was meant to show no longer compiles.

### Tier 4 — Protocol layer (generic over `E: Environment`)

- Migrate `tests/simulator.rs` protocols to `impl<E: Environment> Protocol<E>`. ✅ **done**.
- Determinism: the same `simulate(...)` twice gives equal outputs and traces. ✅ **done** —
  `simulation_is_reproducible_across_runs`; under concurrency, `same_seeds_give_the_same_events_and_outputs`
  (`tests/concurrent.rs`); the golden snapshot pins exact timestamps.
- A capability-carrying environment. ✅ **done** — `DeltaEnv` and `DeltaExchange` in
  `capability_env_supplies_extra_state_to_protocol`.
- Adversarial reordering harness. ❌ **declined** (2026-06-30): deliberately left out.

### Tier 5 — Real network, failure injection

✅ **done**, inline in `src/net/tcp.rs`:

- Multi-party `recv_any` over real TLS: `recv_any_collects_from_multiple_peers_over_tls`.
- Failure paths: `recv_from_closed_peer_reports_connection_closed`,
  `malformed_config_json_is_rejected`, `unloadable_pem_material_is_rejected`.
- Since 0.15.0: a closing peer ends the receives of every session; dropping the network closes
  its connections; `close()` ends receives and sends to every party, this node included; and a
  failed peer (undecodable frame, stream cut inside a frame, failed write) is reported as
  `PeerFailed` in every session. The failure tests start the network over in-memory streams, so
  they do not exercise TLS itself.

### Tier 6 — Cross-cutting gates (CI)

- **MSRV job:** ✅ **done**. `.github/workflows/rust.yml` builds with `cargo build --all-features`
  on 1.88, kept in sync with `rust-version` in `Cargo.toml`. It builds rather than tests, so
  dev-dependencies such as `proptest`, whose own MSRV is newer, stay out of the constraint.
- **Doctests:** ✅ **done** — the crate-doc examples, and one doctest per public constructor on
  `Packet::empty`, `ShamirSS::{new, shares_from_secret}` and `FeldmanSS::{new, shares_from_secret}`.
- **`cargo-audit`:** ✅ wired (`.github/workflows/audit.yml`). `cargo-deny` was considered and
  **declined** (2026-06-30): its advisories check duplicates `cargo-audit`, and the rest does not
  justify a second tool for a dependency set this small.
- **Examples in CI:** ❌ **open**. `cargo test` compiles every example but runs none. The golden
  snapshot (`tests/golden.rs`) runs copies of the protocols of seven of the eight simulator
  examples (all but `concurrent_preprocessing`), which catches behaviour changes in them, but not a
  broken `main`.

---

## 3. Examples plan

Examples are the on-ramp, and where they assert their result they double as smoke tests.

| # | Example | Teaches | Status |
|---|---|---|---|
| 1 | `simple_send_recv` | minimal protocol, generic over `E` | exists |
| 2 | `additive_shr_secure_sum` | additive sharing, an all-to-all round | exists |
| 3 | `shamir_reconstruct` | the `ss` API on its own: share, reconstruct, threshold | ❌ not added |
| 4 | `feldman_vss` | VSS: the dealer commits, a party verifies, a tamper is caught | ❌ not added |
| 5 | `real_tls_two_party` | deployment: `NetworkConfig` + `TcpNetwork`, two processes over mTLS | ✅ as `real_tls_send_recv` |
| 6 | `broadcast_round` | fan-out and fan-in | ❌ not added as such; `additive_shr_secure_sum` and `secure_stats_flamegraph` both have rounds in which every party sends to every other |
| 7 | `composed_protocol` | nesting with typed outputs; a capability-carrying `Environment` | partly: `secure_stats_flamegraph` and `secure_covariance` nest protocols with typed outputs; no example defines its own environment (the test `capability_env_supplies_extra_state_to_protocol` does) |

Added since, beyond the ladder:

- `send_different_types` — packets holding values of different types.
- `bandwidth_flamegraph`, `secure_stats_flamegraph` — per-protocol bandwidth profiling, exported
  as flamegraphs.
- `secure_covariance` — a larger computation with a preprocessing phase.
- `schnorr_pok` — a self-contained Schnorr proof of knowledge between two parties.
- `concurrent_preprocessing` — the same preprocessing one step after another and under `Join`,
  with the two runs' metrics side by side.

**Convention: every example ends with an `assert!` on a known-correct result**, so that running it
is a pass/fail check. ❌ **Partly followed:** `concurrent_preprocessing`, `secure_stats_flamegraph`
and `secure_covariance` assert their results; `simple_send_recv`, `send_different_types`,
`additive_shr_secure_sum`, `bandwidth_flamegraph`, `schnorr_pok` and `real_tls_send_recv` do not.

---

## 4. What is still open

1. **Shamir error paths** (Tier 1): `EvalAndShareLenMismatch`, `SharesWithDifferentDegree`, and
   duplicate party indexes.
2. **`Matrix::scalar_mult` receiver** (Tier 1 design flag): `&mut self` → `&self`, breaking.
3. **`additive` coverage** is still two tests: no property test, no error paths.
4. **Examples #3 and #4** (`shamir_reconstruct`, `feldman_vss`): the on-ramps for the `ss` API.
5. **Assertions in the six examples** listed above, then **running the examples in CI**, which
   needs the TLS example left out or given generated certificates.
