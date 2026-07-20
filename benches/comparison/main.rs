//! Simulated-versus-real timing comparison across three network regimes.
//!
//! This binary is the measurement half of `benches/comparison`; `run_all.sh` is the driving half.
//! It runs one protocol, in one scenario, on one backend, and prints a single CSV row per party to
//! stdout — the shell driver appends those rows to `results/measurements.csv`. Printing rather
//! than writing is deliberate: a real repetition is two concurrent processes, and neither ever
//! touches the results file, so there is no interleaved-write hazard.
//!
//! # Subcommands
//!
//! ```text
//! comparison scenarios                 # list scenarios and the model term that binds in each
//! comparison header                    # CSV header line
//! comparison params <scenario>         # KEY=VALUE lines for the shell driver to eval
//! comparison sim <scenario|all> <protocol|all> [--window BYTES] [--variant NAME]
//! comparison real <scenario> <protocol> <party> <config> <repetition>
//! comparison calibrate <csv>           # recover the realized window, re-simulate with it
//! ```
//!
//! # What is timed
//!
//! Both backends report *each party's own view* of the protocol's span, and both exclude
//! connection setup. On the simulator that is the virtual time between the party's `Start` and
//! `Stop` events. On a real run it is wall-clock time around
//! [`Protocol::execute`](scl_rs::prelude::Protocol::execute), started only after
//! [`barrier`](protocols::barrier) has confirmed the TLS connection is established and warm.
//!
//! Party 0 is the initiator and is the headline figure; party 1's row is recorded too, and sits
//! about half a round trip lower in both backends for the same structural reason (it starts its
//! span on a receive rather than a send).
//!
//! # What is deliberately not corrected for
//!
//! Each real repetition runs over a fresh TCP connection, so it pays TCP slow start; the simulator
//! models a steady-state throughput and does not. That gap is left in rather than warmed away,
//! because it is a real cost a short MPC protocol actually pays and the comparison is of the model
//! as it stands.

mod protocols;
mod scenarios;

use std::{
    collections::HashMap,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::Serialize;

use scl_rs::{
    net::{
        simulation::{event::EventType, network::SimNetwork, simulator::simulate},
        Network, NetworkConfig as TlsNetworkConfig, PartyId, TcpNetwork,
    },
    prelude::{Environment, GeneralEnv, Protocol},
};

use protocols::{barrier, BulkTransfer, PingPong};
use scenarios::{Scenario, ScenarioNetwork};

/// The protocols under comparison, one per network regime they probe.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Which {
    /// [`PingPong`]: round-dominated.
    PingPong,
    /// [`BulkTransfer`]: bandwidth-dominated.
    BulkTransfer,
}

impl Which {
    /// Every protocol, in reporting order.
    const ALL: [Which; 2] = [Which::PingPong, Which::BulkTransfer];

    /// Slug used on the command line and in the `protocol` CSV column.
    fn name(&self) -> &'static str {
        match self {
            Which::PingPong => "ping_pong",
            Which::BulkTransfer => "bulk_transfer",
        }
    }

    /// Looks a protocol up by its [`name`](Which::name).
    fn by_name(name: &str) -> Option<Which> {
        Which::ALL.into_iter().find(|which| which.name() == name)
    }

    /// The payload size this protocol carries in `scenario`, and how many round trips it performs.
    /// Both land in the CSV so a plot can label a series without re-deriving the scenario table.
    fn workload(&self, scenario: &Scenario) -> (usize, usize) {
        match self {
            Which::PingPong => (scenario.ping_pong_payload_bytes, scenario.ping_pong_rounds),
            Which::BulkTransfer => (scenario.bulk_payload_bytes, 1),
        }
    }
}

/// One measurement: a single party's elapsed time for one protocol run on one backend.
struct Row {
    scenario: Scenario,
    protocol: Which,
    /// `real` or `sim`.
    source: &'static str,
    /// `shaped` for a real run; `nominal` or `calibrated` for a simulated one.
    variant: String,
    party: usize,
    /// 1-based for real runs; 0 for simulated ones, which are deterministic and run once.
    repetition: usize,
    elapsed: Duration,
    /// The window the row was produced under. For a real run this is the scenario's nominal value
    /// (what the simulator *would* assume), not a measured one.
    window_bytes: usize,
}

impl Row {
    /// The CSV header matching [`Row::to_csv`].
    fn header() -> &'static str {
        "scenario,protocol,source,variant,party,repetition,elapsed_secs,\
         rtt_ms,bandwidth_bps,loss_fraction,window_bytes,mss_bytes,payload_bytes,rounds"
    }

    /// Renders the row in the long ("tidy") layout: one measurement per line, with the scenario
    /// parameters repeated on each. Redundant on disk, but it means a plotting script needs no
    /// join and no lookup table.
    fn to_csv(&self) -> String {
        let (payload_bytes, rounds) = self.protocol.workload(&self.scenario);
        format!(
            "{},{},{},{},{},{},{:.6},{},{},{},{},{},{},{}",
            self.scenario.name,
            self.protocol.name(),
            self.source,
            self.variant,
            self.party,
            self.repetition,
            self.elapsed.as_secs_f64(),
            self.scenario.rtt_ms,
            self.scenario.bandwidth_bps,
            self.scenario.loss,
            self.window_bytes,
            self.scenario.mss_bytes,
            payload_bytes,
            rounds,
        )
    }
}

/// The environment both backends run under: no protocol here consumes randomness, so the RNG is
/// seeded to a constant and exists only to satisfy the bound.
type Env<N> = GeneralEnv<N, ChaCha20Rng>;

/// Builds that environment for `network`.
fn environment<N: Network>(network: N) -> Env<N> {
    GeneralEnv::new(network, ChaCha20Rng::seed_from_u64(0))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `--bench` is filtered defensively: `cargo bench` may append it, and this target parses its
    // own arguments rather than going through a test harness.
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();

    match args.first().map(String::as_str) {
        Some("scenarios") => list_scenarios(),
        Some("header") => println!("{}", Row::header()),
        Some("params") => {
            let scenario = lookup_scenario(args.get(1))?;
            println!("{}", scenario.shell_vars());
        }
        Some("sim") => run_sim(&args)?,
        Some("real") => run_real(&args)?,
        Some("calibrate") => calibrate(args.get(1).map(Path::new))?,
        _ => {
            eprintln!("{}", usage());
            std::process::exit(2);
        }
    }

    Ok(())
}

/// The command-line summary printed when a subcommand is missing or unrecognized.
fn usage() -> String {
    let scenario_names: Vec<_> = scenarios::ALL.iter().map(|s| s.name).collect();
    let protocol_names: Vec<_> = Which::ALL.iter().map(Which::name).collect();
    format!(
        "usage: comparison <subcommand>\n\n\
         \x20 scenarios                      list scenarios and the binding model term\n\
         \x20 header                         print the CSV header\n\
         \x20 params <scenario>              print KEY=VALUE lines for the shell driver\n\
         \x20 sim <scenario|all> <protocol|all> [--window BYTES] [--variant NAME]\n\
         \x20 real <scenario> <protocol> <party> <config> <repetition>\n\
         \x20 calibrate <csv>                recover the realized window and re-simulate\n\n\
         scenarios: {}, all\n\
         protocols: {}, all",
        scenario_names.join(", "),
        protocol_names.join(", "),
    )
}

/// Resolves a scenario name, reporting the valid options on failure.
fn lookup_scenario(name: Option<&String>) -> Result<Scenario, String> {
    let name = name.ok_or_else(|| format!("missing <scenario>\n\n{}", usage()))?;
    Scenario::by_name(name).ok_or_else(|| format!("unknown scenario {name:?}\n\n{}", usage()))
}

/// Resolves a protocol name, reporting the valid options on failure.
fn lookup_protocol(name: Option<&String>) -> Result<Which, String> {
    let name = name.ok_or_else(|| format!("missing <protocol>\n\n{}", usage()))?;
    Which::by_name(name).ok_or_else(|| format!("unknown protocol {name:?}\n\n{}", usage()))
}

/// Prints each scenario's parameters alongside the term that actually binds, recomputed from those
/// parameters. Useful as a sanity check before committing to a multi-hour shaped run.
fn list_scenarios() {
    for scenario in scenarios::ALL {
        let (term, throughput) = scenario.regime();
        println!(
            "{:<18} rtt={}ms bandwidth={} loss={} window={}B mss={}B",
            scenario.name,
            scenario.rtt_ms,
            scenario.bandwidth_bps,
            scenario.loss,
            scenario.window_bytes,
            scenario.mss_bytes,
        );
        println!(
            "{:<18}   bdp={:.0}B -> binds on {} at {:.0} bit/s ({:.2} Mbit/s)",
            "",
            scenario.bandwidth_delay_product_bytes(),
            term,
            throughput,
            throughput / 1e6,
        );
        println!(
            "{:<18}   ping_pong: {} rounds x {}B    bulk_transfer: {}B",
            "",
            scenario.ping_pong_rounds,
            scenario.ping_pong_payload_bytes,
            scenario.bulk_payload_bytes,
        );
    }
}

/// Runs a protocol on the deterministic simulator and returns each party's virtual elapsed time.
///
/// The span is the party's `Start`-to-`Stop` interval read off its recorded trace, which brackets
/// exactly the protocol body — the same span a real run stopwatches around `execute`.
fn simulated_elapsed<P>(
    config: ScenarioNetwork,
    make_protocol: impl Fn(PartyId) -> P,
) -> HashMap<PartyId, Duration>
where
    P: Protocol<Env<SimNetwork>> + 'static,
    P::Output: Serialize + Send + Clone + 'static,
{
    let parties = vec![PartyId::from(0), PartyId::from(1)];
    let outcome = simulate(
        config,
        parties,
        make_protocol,
        |_, network| environment(network),
        vec![],
    );

    outcome
        .traces
        .iter()
        .map(|(party, trace)| {
            let stamp_of = |wanted: EventType| {
                trace
                    .events()
                    .iter()
                    .find(|event| event.event_type() == wanted)
                    .map(|event| event.timestamp())
                    .unwrap_or_else(|| panic!("party {party:?} recorded no {wanted:?} event"))
            };
            (
                *party,
                stamp_of(EventType::Stop).saturating_sub(stamp_of(EventType::Start)),
            )
        })
        .collect()
}

/// `sim <scenario|all> <protocol|all> [--window BYTES] [--variant NAME]`
///
/// Prints one CSV row per party per (scenario, protocol) pair. `--window` overrides the window the
/// simulator prices with, which is how [`calibrate`] feeds a measured value back in.
fn run_sim(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let scenario_arg = args.get(1).map(String::as_str).unwrap_or("all");
    let protocol_arg = args.get(2).map(String::as_str).unwrap_or("all");

    let selected_scenarios: Vec<Scenario> = if scenario_arg == "all" {
        scenarios::ALL.to_vec()
    } else {
        vec![lookup_scenario(args.get(1))?]
    };
    let selected_protocols: Vec<Which> = if protocol_arg == "all" {
        Which::ALL.to_vec()
    } else {
        vec![lookup_protocol(args.get(2))?]
    };

    let window_override = flag_value(args, "--window")
        .map(|v| v.parse::<usize>())
        .transpose()?;
    let variant = flag_value(args, "--variant").unwrap_or_else(|| "nominal".to_string());

    for scenario in selected_scenarios {
        for protocol in &selected_protocols {
            let window = window_override.unwrap_or(scenario.window_bytes);
            emit_simulated(scenario, *protocol, window, &variant);
        }
    }

    Ok(())
}

/// Simulates one (scenario, protocol) pair at `window_bytes` and prints both parties' rows.
fn emit_simulated(scenario: Scenario, protocol: Which, window_bytes: usize, variant: &str) {
    let config = scenario.sim_config(window_bytes);
    let elapsed = match protocol {
        Which::PingPong => simulated_elapsed(config, |_| PingPong {
            rounds: scenario.ping_pong_rounds,
            payload_bytes: scenario.ping_pong_payload_bytes,
        }),
        Which::BulkTransfer => simulated_elapsed(config, |_| BulkTransfer {
            payload_bytes: scenario.bulk_payload_bytes,
        }),
    };

    let mut parties: Vec<_> = elapsed.into_iter().collect();
    parties.sort_by_key(|(party, _)| party.as_usize());

    for (party, span) in parties {
        println!(
            "{}",
            Row {
                scenario,
                protocol,
                source: "sim",
                variant: variant.to_string(),
                party: party.as_usize(),
                repetition: 0,
                elapsed: span,
                window_bytes,
            }
            .to_csv()
        );
    }
}

/// `real <scenario> <protocol> <party> <config> <repetition>`
///
/// Runs this party's half of one repetition over mutually authenticated TLS and prints its row.
/// Both party processes must be running: `TcpNetwork::create` blocks until the peer connects.
fn run_real(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let scenario = lookup_scenario(args.get(1))?;
    let protocol = lookup_protocol(args.get(2))?;
    let party: usize = args
        .get(3)
        .ok_or("missing <party>")?
        .parse()
        .map_err(|_| "<party> must be 0 or 1")?;
    let config_path = args.get(4).ok_or("missing <config>")?;
    let repetition: usize = args
        .get(5)
        .ok_or("missing <repetition>")?
        .parse()
        .map_err(|_| "<repetition> must be a positive integer")?;

    let runtime = tokio::runtime::Runtime::new()?;
    let elapsed = runtime.block_on(real_elapsed(scenario, protocol, party, config_path))?;

    println!(
        "{}",
        Row {
            scenario,
            protocol,
            source: "real",
            variant: "shaped".to_string(),
            party,
            repetition,
            elapsed,
            window_bytes: scenario.window_bytes,
        }
        .to_csv()
    );

    Ok(())
}

/// Connects over TLS, warms the connection with a barrier round trip, then times the protocol.
///
/// Everything before the stopwatch — the TCP connect, the TLS handshake, the barrier — is excluded,
/// because the simulator models none of it and, under a lossy shape, handshake retransmits vary far
/// more than the protocol does.
async fn real_elapsed(
    scenario: Scenario,
    protocol: Which,
    party: usize,
    config_path: &str,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let config = TlsNetworkConfig::new(Path::new(config_path))?;
    let network = TcpNetwork::create(party, config).await?;
    let mut env = environment(network);

    barrier(&mut env).await?;

    let started = Instant::now();
    match protocol {
        Which::PingPong => {
            PingPong {
                rounds: scenario.ping_pong_rounds,
                payload_bytes: scenario.ping_pong_payload_bytes,
            }
            .execute(&mut env)
            .await?;
        }
        Which::BulkTransfer => {
            BulkTransfer {
                payload_bytes: scenario.bulk_payload_bytes,
            }
            .execute(&mut env)
            .await?;
        }
    }
    let elapsed = started.elapsed();

    env.network_mut().close().await?;
    Ok(elapsed)
}

/// `calibrate <csv>`
///
/// Recovers the window the kernel actually delivered in the window-limited scenario and re-emits
/// that scenario's simulated rows under it, tagged `calibrated`.
///
/// This is the procedure the [`WindowSize`](scl_rs::net::simulation::channel::WindowSize)
/// documentation prescribes — time a bulk transfer of known size over a link of known RTT, take its
/// throughput `T`, and set the window to `T * RTT / 8` — applied to the median of the real bulk
/// repetitions. The median rather than the mean because a single stalled run should not move it.
///
/// The measured span covers the bulk message plus its acknowledgement, so one full round trip of
/// propagation is subtracted before the rate is taken; the payload is counted with the same
/// per-segment header overhead the simulator charges, so the recovered window is directly
/// comparable to the configured one.
fn calibrate(csv_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let path = csv_path.ok_or("missing <csv>")?;
    let scenario = scenarios::WINDOW_LIMITED;
    let contents = fs::read_to_string(path)?;

    let mut spans: Vec<f64> = contents
        .lines()
        .skip(1)
        .filter_map(|line| {
            let field: Vec<&str> = line.split(',').collect();
            let matches = field.len() > 6
                && field[0] == scenario.name
                && field[1] == Which::BulkTransfer.name()
                && field[2] == "real"
                && field[4] == "0";
            matches.then(|| field[6].parse::<f64>().ok()).flatten()
        })
        .collect();

    if spans.is_empty() {
        return Err(format!(
            "no real {} bulk_transfer rows for party 0 in {}; run the shaped suite first",
            scenario.name,
            path.display()
        )
        .into());
    }

    spans.sort_by(f64::total_cmp);
    let median = spans[spans.len() / 2];

    let transfer_secs = median - scenario.rtt_secs();
    if transfer_secs <= 0.0 {
        return Err(format!(
            "median bulk span {median:.6}s does not exceed one RTT ({:.3}s); \
             the link does not look shaped",
            scenario.rtt_secs()
        )
        .into());
    }

    let segments = (scenario.bulk_payload_bytes as f64 / scenario.mss_bytes as f64).ceil();
    let payload_bits = 8.0 * (scenario.bulk_payload_bytes as f64 + segments * 40.0);
    let throughput_bps = payload_bits / transfer_secs;
    let realized_window = (throughput_bps * scenario.rtt_secs() / 8.0).round() as usize;

    eprintln!(
        "calibration: median span {:.3}s over {} runs -> {:.2} Mbit/s -> window {} B \
         ({:+.1} % against the configured {} B)",
        median,
        spans.len(),
        throughput_bps / 1e6,
        realized_window,
        100.0 * (realized_window as f64 - scenario.window_bytes as f64)
            / scenario.window_bytes as f64,
        scenario.window_bytes,
    );

    for protocol in Which::ALL {
        emit_simulated(scenario, protocol, realized_window, "calibrated");
    }

    Ok(())
}

/// Reads the value following `flag` in `args`, if the flag is present.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let position = args.iter().position(|arg| arg == flag)?;
    args.get(position + 1).cloned()
}
