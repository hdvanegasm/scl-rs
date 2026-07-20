# Simulated vs. real execution

This harness measures how closely the deterministic simulator predicts a real
mutually-authenticated-TLS run, across the three network regimes the crate README reports on. It
produces one tidy CSV, `results/measurements.csv`, with every repetition recorded individually so
the *distribution* is visible — not just a mean. That matters most in the lossy regime, where the
README's finding is not that the model is off by some percentage but that real runs are not
reproducible at all.

## Quick start

```bash
./benches/comparison/run_all.sh              # ~26 min, prompts before touching the network
./benches/comparison/shape.sh teardown       # only if something is interrupted hard
```

Useful variations:

```bash
./run_all.sh --reps 5                        # a quick shakedown before committing to a full run
./run_all.sh --scenarios lossy               # one regime
./run_all.sh --protocols ping_pong --yes     # one protocol, no confirmation prompt
./run_all.sh --out /tmp/run2.csv             # somewhere other than results/
```

## The design

### Two protocols, one per regime

Both live in `protocols.rs`, are written once against `Environment`, and run unchanged on both
backends — that portability is the property under test.

| Protocol | Shape | Running time | Probes |
|---|---|---|---|
| `PingPong` | 30 sequential round trips of 1 KB | `rounds × RTT` | the latency term |
| `BulkTransfer` | one 0.5–1 MB message plus an ack | `bytes × 8 / throughput` | the throughput term |

They are synthetic on purpose. A real MPC protocol folds local compute into the wall clock, and the
simulator models no compute at all, so a gap between backends could not be attributed to the
network model. These two do nothing but move bytes.

### Three scenarios, one per binding term

Defined once in `scenarios.rs`, which is the single source of truth for *both* the simulator's
`ChannelConfig` and the `tc` shaping — so the two links cannot silently drift apart. The parameters
are chosen so a different term of the model binds in each, and `scenarios.rs` recomputes which one
actually binds rather than trusting the label:

```
$ ./benches/comparison/run_all.sh --help      # or, directly:
$ <comparison-bin> scenarios

bandwidth_limited  rtt=100ms bandwidth=1000000 loss=0 window=65536B mss=1460B
                     bdp=12500B -> binds on bandwidth at 1.00 Mbit/s
window_limited     rtt=100ms bandwidth=100000000 loss=0 window=65536B mss=1460B
                     bdp=1250000B -> binds on window at 5.24 Mbit/s
lossy              rtt=100ms bandwidth=100000000 loss=0.01 window=65536B mss=1460B
                     bdp=1250000B -> binds on loss at 1.43 Mbit/s
```

### What is timed

Each party's own view of the protocol span, on both backends, excluding connection setup:

- **Simulated** — virtual time between the party's `Start` and `Stop` trace events.
- **Real** — wall clock around `Protocol::execute`, started only after a barrier round trip has
  confirmed the TLS connection is established and warm. The TCP connect, the TLS handshake and the
  barrier are all excluded; the simulator models none of them, and under a lossy shape handshake
  retransmits vary far more than the protocol does.

Party 0 is the initiator and the headline figure. Party 1's row is recorded too and sits about half
a round trip lower — in both backends, for the same structural reason: it opens its span on a
receive rather than a send.

**Not corrected for:** every real repetition runs over a fresh connection and so pays TCP slow
start, which the simulator's steady-state throughput does not model. That is left in rather than
warmed away, because it is a cost a short MPC protocol genuinely pays.

## Network shaping

`shape.sh` applies each scenario to loopback. Four changes, each with a reason:

- **A `prio` qdisc with a netem band, selected by port filter.** Delay, rate and loss reach *only*
  ports 6000–6001. Shaping loopback wholesale would put 50 ms of latency and a 1 Mbit/s cap on
  every local service on the machine. The priomap is overridden to all-ones so no unclassified
  traffic can reach the shaped band via its TOS bits.
- **MTU lowered to 1500.** Loopback defaults to 65536, making the real MSS ~65483 rather than the
  1460 the simulator prices with. Mild distortion in the loss-less scenarios (per-segment header
  overhead charged against a segment 45× too large); severe in the lossy one, whose model term is
  *linearly* proportional to MSS.
- **GSO/TSO/GRO disabled.** Otherwise netem receives aggregated super-packets of up to 64 KB even
  at a 1500-byte MTU, so one drop discards ~44 segments. `shape.sh` verifies these actually went
  off and warns loudly if the kernel refused.
- **`tcp_rmem`/`tcp_wmem` pinned to 131072 — window-limited scenario only.** That scenario needs a
  window that does not autotune out of the regime. The other two leave autotuning alone; there, a
  large real window does not change which term binds.

The delay applied is **half** the RTT: a loopback packet crosses the egress qdisc once outbound and
once on the reply.

Every original value is saved to `.shape-state` and restored on exit, including on Ctrl-C. If a run
is killed hard enough to skip the trap, `./shape.sh teardown` restores by hand and
`./shape.sh status` shows what is currently applied.

## Calibration

The window-limited scenario gets a third set of rows. After the real runs, `calibrate` recovers the
window the kernel *actually* delivered — median real bulk span, minus one RTT of propagation, into
throughput, into `T × RTT / 8` — and re-simulates that scenario under it, tagged `calibrated`. This
is the procedure the `WindowSize` documentation prescribes, and it is what separates "the model's
form is wrong" from "the model's form is right but its default window is not the one Linux gave
you". The CSV carries both predictions so a plot can show the nominal miss and the calibrated fit
side by side.

## Output

`results/measurements.csv`, long form — one row per party per repetition, scenario parameters
repeated on each row so a plotting script needs no join:

```
scenario,protocol,source,variant,party,repetition,elapsed_secs,
rtt_ms,bandwidth_bps,loss_fraction,window_bytes,mss_bytes,payload_bytes,rounds
```

| Column | Values |
|---|---|
| `source` | `real` or `sim` |
| `variant` | `shaped` (real); `nominal` or `calibrated` (sim) |
| `party` | `0` (initiator, the headline figure) or `1` |
| `repetition` | `1..N` for real runs; `0` for the deterministic simulated ones |
| `window_bytes` | for `calibrated` rows, the recovered window rather than the configured one |

For plots: filter `party == 0`, then compare the `real` distribution against the single `sim` value
per (scenario, protocol). A strip or box plot of the real repetitions with the simulated prediction
drawn as a rule shows both the bias and the spread — and in the lossy scenario the spread is the
result. Reruns append, so delete the file for a clean sheet.

`results/` and `.shape-state` are gitignored.

## Files

| File | Role |
|---|---|
| `main.rs` | measurement binary: `scenarios`, `header`, `params`, `sim`, `real`, `calibrate` |
| `protocols.rs` | `PingPong`, `BulkTransfer`, and the pre-timing barrier |
| `scenarios.rs` | the scenario table; drives both the simulator config and the `tc` shaping |
| `run_all.sh` | the driver: shape, repeat, collect, simulate, calibrate, restore |
| `shape.sh` | `setup <scenario>` / `teardown` / `status` |
| `common.sh` | shared paths, ports, and the binary locator |
| `config_p0.json`, `config_p1.json` | TLS configs; `base_port` **must** match `BASE_PORT` in `common.sh` |

The binary is declared as a `[[bench]]` with `harness = false`, so it is built by
`cargo bench --bench comparison --no-run` and then invoked directly — a repetition is two
concurrent processes, and two `cargo` invocations would serialize on the build lock.
