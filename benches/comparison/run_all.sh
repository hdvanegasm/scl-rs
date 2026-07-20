#!/usr/bin/env bash
# Runs the full simulated-versus-real comparison and writes one tidy CSV.
#
#   ./run_all.sh [--reps N] [--scenarios "a b"] [--protocols "p q"] [--out FILE] [--yes]
#
# For every (scenario, protocol) pair it shapes loopback to the scenario's parameters, runs the
# protocol over real mutually authenticated TLS `--reps` times (default 50), and appends every
# repetition's timing to the results CSV. It then runs the same pairs on the deterministic
# simulator, and finally re-simulates the window-limited scenario using the window the kernel
# actually delivered.
#
# Loopback shaping is torn down on every exit path, including Ctrl-C and a failure mid-run. If the
# script is killed hard enough to skip its trap, `./shape.sh teardown` restores things by hand.
#
# Results land in `results/measurements.csv` in long form — one row per party per repetition, with
# the scenario parameters repeated on each row, so a plotting script needs no join:
#
#   scenario,protocol,source,variant,party,repetition,elapsed_secs,rtt_ms,bandwidth_bps,
#   loss_fraction,window_bytes,mss_bytes,payload_bytes,rounds
#
# Filter on `source` to compare backends (`real` against `sim`), and on `party` — party 0 is the
# initiator and the headline figure. Reruns append, so delete the file first for a clean sheet.

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

REPS=50
SCENARIOS=""
PROTOCOLS="ping_pong bulk_transfer"
OUT=""
ASSUME_YES=0
# Ceiling on a single party process. A lossy repetition can stall on retransmits; without a cap one
# wedged run would hang the whole suite. Generous enough that no healthy run can trip it.
RUN_TIMEOUT=300

while [[ $# -gt 0 ]]; do
    case "$1" in
        --reps)      REPS="$2"; shift 2 ;;
        --scenarios) SCENARIOS="$2"; shift 2 ;;
        --protocols) PROTOCOLS="$2"; shift 2 ;;
        --out)       OUT="$2"; shift 2 ;;
        --timeout)   RUN_TIMEOUT="$2"; shift 2 ;;
        --yes|-y)    ASSUME_YES=1; shift ;;
        -h|--help)   sed -n '2,25p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *)           die "unknown argument '$1'" ;;
    esac
done

OUT="${OUT:-$RESULTS_DIR/measurements.csv}"

cd "$CRATE_ROOT"

# ---------------------------------------------------------------------------------------------
# Preflight
# ---------------------------------------------------------------------------------------------

for tool in tc ip sysctl timeout; do
    command -v "$tool" >/dev/null || die "'$tool' is required but not on PATH"
done
command -v ethtool >/dev/null || log "warning: ethtool not found; offloads cannot be disabled"

BIN="$(find_comparison_bin)"
# Exported so the shape.sh subprocesses reuse this build instead of re-entering cargo three times.
export COMPARISON_BIN="$BIN"
[[ -z "$SCENARIOS" ]] && SCENARIOS="$("$BIN" scenarios | awk '/^[a-z]/ {print $1}' | tr '\n' ' ')"

# The TLS material the config files point at. Generated once and reused; the certificates carry
# 127.0.0.1 as their subject alternative name, which is why this benchmark runs over loopback.
if [[ ! -f "$CRATE_ROOT/certs/rootCA.crt" ]]; then
    log "generating TLS certificates for $N_PARTIES parties"
    bash "$CRATE_ROOT/gen_self_signed_certs.sh" "$N_PARTIES" >/dev/null
fi

CONFIG_0="$COMPARISON_DIR/config_p0.json"
CONFIG_1="$COMPARISON_DIR/config_p1.json"
for config in "$CONFIG_0" "$CONFIG_1"; do
    [[ -f "$config" ]] || die "missing TLS config $config"
done

# Estimate the wall clock from the simulator's own predictions, which is what the real runs are
# being checked against — accurate enough to decide whether to walk away, and free.
estimate_seconds() {
    local total=0 scenario protocol predicted
    for scenario in $SCENARIOS; do
        for protocol in $PROTOCOLS; do
            predicted="$("$BIN" sim "$scenario" "$protocol" | awk -F, '$5=="0" {print $7}')"
            # Plus a flat allowance per repetition for process spawn, TCP connect and the TLS
            # handshake, all of which sit outside the timed span.
            total="$(awk -v t="$total" -v p="$predicted" -v r="$REPS" \
                'BEGIN {printf "%.0f", t + r * (p + 1.5)}')"
        done
    done
    echo "$total"
}

ESTIMATE="$(estimate_seconds)"
N_PAIRS=$(( $(wc -w <<< "$SCENARIOS") * $(wc -w <<< "$PROTOCOLS") ))

cat >&2 <<BANNER

  scl-rs simulated-vs-real comparison
  -----------------------------------
  scenarios : $SCENARIOS
  protocols : $PROTOCOLS
  reps      : $REPS  ($N_PAIRS pairs => $(( N_PAIRS * REPS )) real runs, 2 processes each)
  output    : $OUT
  estimate  : ~$(( ESTIMATE / 60 )) min

  This needs root to shape loopback with tc. While it runs it will, on device '$DEV':
    * add a netem qdisc affecting ONLY ports $BASE_PORT-$(( BASE_PORT + N_PARTIES - 1 ))
    * lower the MTU to 1500 (loopback-wide, affects other local traffic mildly)
    * disable GSO/TSO/GRO (loopback-wide)
    * pin tcp_rmem/tcp_wmem for the window-limited scenario only (system-wide)

  All of these are restored on exit. To restore by hand: ./benches/comparison/shape.sh teardown

BANNER

if [[ "$ASSUME_YES" -ne 1 ]]; then
    read -r -p "  proceed? [y/N] " reply
    [[ "$reply" =~ ^[Yy]$ ]] || die "aborted"
fi

# ---------------------------------------------------------------------------------------------
# Teardown wiring
# ---------------------------------------------------------------------------------------------

TMPDIR_RUN="$(mktemp -d)"
KEEPALIVE_PID=""

cleanup() {
    local status=$?
    trap - EXIT INT TERM
    [[ -n "$KEEPALIVE_PID" ]] && kill "$KEEPALIVE_PID" 2>/dev/null || true
    rm -rf "$TMPDIR_RUN"
    log "restoring $DEV"
    bash "$COMPARISON_DIR/shape.sh" teardown || log "warning: teardown failed; run shape.sh teardown"
    exit "$status"
}
trap cleanup EXIT INT TERM

# The suite outlives sudo's default credential lifetime, and a password prompt appearing 20 minutes
# in — between two shaping steps, with nobody at the keyboard — would strand the run with loopback
# still shaped. Refresh the timestamp in the background instead.
if [[ "$(id -u)" -ne 0 ]]; then
    sudo -v || die "sudo is required to shape the network"
    while true; do sudo -n true; sleep 60; done 2>/dev/null &
    KEEPALIVE_PID=$!
fi

# ---------------------------------------------------------------------------------------------
# Measurement
# ---------------------------------------------------------------------------------------------

mkdir -p "$RESULTS_DIR"
if [[ ! -f "$OUT" ]]; then
    "$BIN" header > "$OUT"
    log "created $OUT"
else
    log "appending to existing $OUT"
fi

FAILURES=0

# Runs both halves of one repetition concurrently and appends their rows.
#
# Each party process prints its own CSV row to its own stdout and never touches the results file,
# so the two concurrent processes cannot interleave a write. The rows are appended here, after both
# have exited.
run_repetition() {
    local scenario="$1" protocol="$2" rep="$3"
    local out0="$TMPDIR_RUN/p0.csv" out1="$TMPDIR_RUN/p1.csv"
    local err0="$TMPDIR_RUN/p0.log" err1="$TMPDIR_RUN/p1.log"
    local status0=0 status1=0

    # Party 0 listens and party 1 dials, but both are started together: the client retries until
    # the listener is up, so the ordering does not need to be enforced here.
    timeout "$RUN_TIMEOUT" "$BIN" real "$scenario" "$protocol" 0 "$CONFIG_0" "$rep" \
        >"$out0" 2>"$err0" &
    local pid0=$!
    timeout "$RUN_TIMEOUT" "$BIN" real "$scenario" "$protocol" 1 "$CONFIG_1" "$rep" \
        >"$out1" 2>"$err1" &
    local pid1=$!

    wait "$pid0" || status0=$?
    wait "$pid1" || status1=$?

    if [[ "$status0" -ne 0 || "$status1" -ne 0 ]]; then
        FAILURES=$(( FAILURES + 1 ))
        log "  rep $rep FAILED (p0=$status0 p1=$status1): $(tail -1 "$err0") $(tail -1 "$err1")"
        return 0
    fi

    cat "$out0" "$out1" >> "$OUT"
    awk -F, '{printf "  %s rep %s party %s: %.3fs\n", $2, $6, $5, $7}' "$out0" >&2
}

STARTED_AT=$SECONDS

for scenario in $SCENARIOS; do
    log "=== scenario: $scenario ==="
    bash "$COMPARISON_DIR/shape.sh" setup "$scenario"

    for protocol in $PROTOCOLS; do
        log "--- $scenario / $protocol: $REPS repetitions ---"
        for (( rep = 1; rep <= REPS; rep++ )); do
            run_repetition "$scenario" "$protocol" "$rep"
            # Lets the previous pair's sockets leave TIME_WAIT before the ports are rebound.
            sleep 0.2
        done
    done
done

log "=== unshaping before the simulated runs ==="
bash "$COMPARISON_DIR/shape.sh" teardown

# ---------------------------------------------------------------------------------------------
# Simulated counterparts
# ---------------------------------------------------------------------------------------------

log "=== simulated runs (nominal parameters) ==="
for scenario in $SCENARIOS; do
    for protocol in $PROTOCOLS; do
        "$BIN" sim "$scenario" "$protocol" >> "$OUT"
    done
done

# The window-limited scenario is the one the README reports as needing calibration: the model's
# form holds, but the window Linux delivers is not the one the simulator assumes. Recover it from
# the real runs and re-simulate, so the CSV carries both the nominal and the calibrated prediction.
if grep -q '^window_limited,bulk_transfer,real,' "$OUT"; then
    log "=== simulated runs (window calibrated from the real bulk transfers) ==="
    "$BIN" calibrate "$OUT" >> "$OUT"
fi

ELAPSED=$(( SECONDS - STARTED_AT ))
log "done in $(( ELAPSED / 60 ))m $(( ELAPSED % 60 ))s; $(( $(wc -l < "$OUT") - 1 )) rows in $OUT"
[[ "$FAILURES" -gt 0 ]] && log "warning: $FAILURES repetition(s) failed and were not recorded" || true
exit 0
