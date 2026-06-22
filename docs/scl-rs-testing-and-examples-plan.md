# scl-rs — Testing & Examples Plan

**Date:** 2026-06-21
**Against:** `main` at v0.4.1 + the `[Unreleased]` `Environment`-trait refactor
(`Protocol<E: Environment>`, `GeneralEnv<N>`, factory-based `simulate`).
**Toolchain note:** written from reading the source tree, not from a `cargo test` run.
Anything marked *(verify)* should be confirmed once compiled.

---

## 1. Honest coverage snapshot

What the suite actually exercises today, by module:

| Area | State | Notes |
|---|---|---|
| `net/simulation` | **Strong** | `tests/simulator.rs`: 13 tests — send/recv, message ordering, chained-stage state, bandwidth+latency timing, trace rendering & per-party arrows, broadcast, `recv_any` quorum, hooks, payload eliding. |
| `net` (real TLS) | **Decent** | 3 inline `#[tokio::test]`: end-to-end public API (incl. 64 KiB multi-record frame), raw handshake, mTLS client-without-cert rejection. |
| `math/field/*` | **Decent** | mersenne61 (8), secp256k1 prime/scalar (2+3), curve (14), naf (1). Mostly happy-path. |
| `math/poly` | **Thin** | One interpolation round-trip (`lagrange.rs`). Error paths untested. |
| `math/vector` | **Thin** | 6 tests, basic ops. |
| `ss/additive` | **Thin** | One reconstruct round-trip. |
| `math/matrix` | **None** | Referenced by zero tests/examples. |
| `ss/shamir` | **None** | Reconstruction error paths, threshold behavior, index validation all unexercised. |
| `ss/feldman` | **None** | Share-validity / tamper detection — the whole point of VSS — untested. |
| `protocol` (generic-`E`) | **Indirect** | Tests pin `Protocol<GeneralEnv<SimNetwork>>` with `env.network.*` field access; the generic `impl<E: Environment>` form (what examples now use) is never compiled by a test. |

Two structural gaps cut across everything: **no property-based tests** (high leverage for
an algebra/crypto library) and **no negative/adversarial tests** (the security claims rest
on rejection paths that nothing currently triggers). `tests/net.rs` is a 1-byte stub —
delete it or fill it.

---

## 2. Testing plan

Six tiers, ordered by leverage-per-effort. Tiers 1–3 are pure unit/property work with no
protocol machinery and should land first; 4–6 build on the simulator and TLS paths.

### Tier 1 — Close the zero-coverage holes

These are correctness tests against existing public APIs. Target output correctness, not
internal representation, so they survive refactors.

- **Shamir** (`tests/shamir.rs`):
  - Round-trip: share a random secret at degree `t`, reconstruct from any `t+1` of `n`
    shares, assert equality — and assert it holds for *several different* `(t+1)`-subsets
    (subset-invariance is the real Shamir property, not just one happy path).
  - Threshold floor: reconstructing from `< t+1` shares returns `ShareError::NotEnoughShares`.
  - `EvalAndShareLenMismatch` when `shares.len() != party_indexes.len()`.
  - `SharesWithDifferentDegree` when shares carry mixed `degree` fields.
  - Duplicate `party_indexes` → surfaces as `ReconstructionError`
    (`NotAllDifferentInterpolation` from the poly layer). This pins a *security-relevant*
    guard that exists but is invisible. *(verify the error variant wraps as expected.)*

- **Feldman** (`tests/feldman.rs`) — the security-load-bearing one:
  - Round-trip on the secp256k1 scalar field.
  - `is_valid` is `true` for honest shares, `false` after flipping a single share value.
  - Tampered share → `secret_from_shares` returns `ShareError::InvalidShare { party_idx }`
    naming the right party.
  - Wrong commitment-vector length (`!= degree + 1`) → `is_valid` is `false`. This maps
    directly to the audit checklist's "commitment vector length not checked"; here the
    check exists, so the test locks it in.
  - `LengthMismatch` when `shares.len() != party_indexes.len()`.

- **Matrix** (`tests/matrix.rs`):
  - `from_vec` dimension mismatch → error; `identity`/`zero`/`random` shapes; `get`/`get_mut`
    in-bounds vs `None` out-of-bounds; `is_square`.
  - Pin `scalar_mut_in_place` vs `scalar_mult`. **Design flag:** `scalar_mult(&mut self,…) -> Self`
    takes `&mut self` but doesn't mutate (it builds a fresh matrix). The `&mut` is misleading
    and the name collides conceptually with the in-place variant. A test documents current
    behavior; consider renaming to `&self` before any 0.x that touches this module.

### Tier 2 — Property-based tests (`proptest`)

Highest leverage for this codebase: algebraic structures have laws that example-based tests
sample sparsely. Add `proptest` to `[dev-dependencies]`; gate behind a generator strategy per
field. Candidates:

- **Ring/field laws** over Mersenne61, secp256k1 prime & scalar: associativity, commutativity,
  distributivity, additive/multiplicative identity & inverse, `a - a == 0`,
  `a * a⁻¹ == 1` for `a ≠ 0`.
- **Shamir:** for random `(secret, t, n)` with `t < n`, every `(t+1)`-subset reconstructs the
  same secret. This is the property the one-shot Tier-1 test only samples.
- **Poly:** evaluate-then-interpolate is identity for random polynomials and random node sets
  (generalizes the single `lagrange.rs` case).
- **Serialization round-trip:** `T -> postcard/serde -> T` for every `Serialize`/`Deserialize`
  type that crosses the wire — `Packet` contents, `ShamirSS`, `FeldmanSS`, field elements,
  curve points. The simulator already serializes outputs with `postcard`; a malformed
  round-trip would surface as a runtime `expect` panic in `drive`, so this guards a real path.

### Tier 3 — Negative / adversarial tests (security rejection paths)

The library's value proposition is structural guarantees; these tests assert the guards fire.
Each maps to a specific item in the `mpc-audit` checklist and to real code:

- **On-curve rejection** (`AffinePoint::is_valid`, `math/ec/secp256k1.rs`): construct a point
  with a y that fails y²=x³+7 and assert `is_valid()` is `false`. Then the sharper question:
  **does Feldman validate dealer commitments are on-curve before using them in `scalar_mul`?**
  Reading `FeldmanSS::is_valid`, commitments feed straight into `scalar_mul` with no on-curve
  check. A test that feeds an off-curve commitment is either a passing guard or a **finding**
  ("adversary-supplied point not validated as on-curve"). Write the test; let it adjudicate.
- **Packet malformed reads** (`net/mod.rs`): `read`/`pop` now return `Result`. Assert
  `WrongPacketIdx` on out-of-range index and a deserialize-failure variant on a buffer that
  holds the wrong type. This is the 0.4.0 error-API change; nothing tests it yet.
- **Poly guard surfaces as error, not panic:** `interpolate_polynomial_at` uses `assert!`/
  `assert_eq!` for empty and length-mismatch inputs — those are panics on adversary-reachable
  shapes. A test pins current behavior and flags whether these should become typed errors
  (audit checklist: "panic instead of structured abort"). Likely a small 0.x API change.
- **CSPRNG bound (documentation test):** secret-gen accepts any `Rng`. Until the `CryptoRng`
  bound lands, a test can at least assert determinism-from-seed is *possible* (the footgun),
  documenting the gap the roadmap already tracks.

### Tier 4 — Protocol layer (generic over `E: Environment`)

The refactor's whole point is protocols generic over the environment; tests should exercise
that, not just the concrete `GeneralEnv<SimNetwork>` shape.

- **Migrate `tests/simulator.rs` protocols to `impl<E: Environment> Protocol<E>`** with
  `env.network()/network_mut()`, matching the examples. This compiles the generic path and
  catches capability-bound regressions the concrete tests miss.
- **Determinism / reproducibility:** run the *same* `simulate(...)` twice and assert byte-equal
  outputs **and** identical event traces. This is the core promise of the deterministic
  executor and currently nothing asserts run-to-run stability (the `seq` tiebreaker exists
  precisely for this — test it).
- **A capability-carrying environment:** define a tiny test-only `Env` supertrait (e.g. a
  counter or a Δ value) and a protocol bounded on it, proving the "capabilities accumulate up
  the composition and the factory must supply them" claim from the CHANGELOG compiles and runs.
  This is the dogfood for the MASCOT direction.
- **Adversarial reordering harness** (roadmap "deferred to post-1.0", but the explicit-blocking
  state makes it cheap now): a `Switchboard` variant or hook that delays/reorders deliveries
  within the model, asserting a correct protocol still converges. Even a minimal version is a
  strong robustness signal and exercises `recv_any` under reorder.

### Tier 5 — Real network, failure injection

- Fill `tests/net.rs` (or delete it and keep the inline ones — pick one home). If kept as an
  integration test, cover the multi-party (`n > 2`) `recv_any` path over real TLS, which the
  current 2-party inline test doesn't reach.
- **Failure paths:** connection closed mid-recv → `ConnectionClosed`; malformed config JSON →
  `ConfigParse`; unloadable PEM → `InvalidPemFile`. These variants were added in 0.4.0 and are
  untested.

### Tier 6 — Cross-cutting gates (CI)

- **MSRV job:** the roadmap's standing open item — `rust-version = "1.85.1"` is declared but no
  CI matrix pins it. Add a `1.85.1` + `stable` matrix.
- **Doctests:** the crate-doc protocol/simulator examples are compiled doctests; keep them green
  and add a doctest to each public constructor on `Packet`, `ShamirSS`, `FeldmanSS`.
- Wire `cargo-deny`/`cargo-audit` (already roadmap §6/§8) once the above lands.

---

## 3. Examples plan

Examples are the fastest on-ramp **and** double as smoke tests (`cargo run --example` in CI).
Today there are two (`simple_send_recv`, `additive_shr_secure_sum`), both simulator-only and
both already on the new generic-`E` API. Build a deliberate ladder; each rung should be
runnable and, where cheap, assert its own result so CI catches rot.

| # | Example | Teaches | Status |
|---|---|---|---|
| 1 | `simple_send_recv` | minimal protocol, generic over `E` | exists |
| 2 | `additive_shr_secure_sum` | additive sharing + all-to-all round | exists |
| 3 | `shamir_reconstruct` | `ss` API standalone (no network) — share/reconstruct, show threshold | **add** |
| 4 | `feldman_vss` | VSS: dealer commits, party verifies, tamper is caught | **add** |
| 5 | `real_tls_two_party` | the deployment path: `NetworkConfig` + `TcpNetwork`, two processes/tasks over mTLS | **add** (sketched in crate docs; make it real) |
| 6 | `broadcast_round` | `futures` combinators for fan-out/fan-in under the simulator | **add** |
| 7 | `composed_protocol` | call-and-return nesting with a typed `Output`; capability-carrying `Env` | **add** (showcase; pairs with MASCOT direction) |

Notes on priorities:

- **#3 and #4 are the urgent ones** — they're the missing on-ramps for the two completely
  untested modules, and writing the example *is* the first integration test for them. Do these
  alongside Tier 1.
- **#5** closes the gap the roadmap keeps flagging: the simulator suite never shows a user how
  to actually deploy. Reuse the cert-generation pattern already in `net/mod.rs`'s inline tests.
- **#7** is the dogfooding example for the typed-composition / `Env`-capability design. Worth
  doing once a second real sub-protocol exists (e.g. an `Open`), not before — "bake before
  freeze" applies to examples too.

Convention to adopt: every example ends with an `assert!` on a known-correct result (the
secure-sum example can check the sum equals the cleartext sum of inputs), so `cargo run
--example X` is a pass/fail smoke test, not just output-on-stdout.

---

## 4. Sequencing

Bottom-up, mirroring the project's own build order:

1. **Tier 1 + examples #3/#4** — kills the three zero-coverage modules with both a test file
   and a runnable example each. Highest correctness-per-hour; no new deps except maybe none.
2. **Tier 2** — add `proptest`; convert the strongest happy-path tests (fields, Shamir, poly,
   serialization) into laws. This is where latent algebra bugs surface.
3. **Tier 3** — adversarial rejection tests. The on-curve/Feldman-commitment test may produce
   the first real finding; budget for a small fix.
4. **Tier 4** — migrate simulator tests to generic `E`, add determinism assertions, then the
   reordering harness. Lands with the `[Unreleased]` refactor since it's the thing that
   validates it.
5. **Example #5 + Tier 5** — real-TLS deployment example and failure-path tests together
   (shared cert/config scaffolding).
6. **Tier 6 + examples #6/#7** — CI MSRV matrix, then the showcase examples once the
   composition surface is stable.

The first batch (step 1) is independent of the `Environment` refactor and can land on `main`
immediately; steps 4+ should ride with the 0.5.0 trait change so tests and API move together.
