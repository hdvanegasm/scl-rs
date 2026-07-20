#!/usr/bin/env bash
# Shared paths, ports and helpers for the comparison harness.
#
# Sourced by shape.sh and run_all.sh; not meant to be run on its own.

set -euo pipefail

# Crate root, resolved from this script's own location so the harness works from any cwd. The TLS
# config files reference `./certs/...` relative to the working directory, so every command below is
# run from here.
COMPARISON_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_ROOT="$(cd "$COMPARISON_DIR/../.." && pwd)"

# Port party i listens on is BASE_PORT + i. This *must* agree with "base_port" in config_p0.json
# and config_p1.json: shape.sh installs its tc filters by port number, so a mismatch would leave
# the benchmark traffic unshaped while reporting success.
BASE_PORT=6000
N_PARTIES=2

# Where teardown state and results live.
SHAPE_STATE="$COMPARISON_DIR/.shape-state"
RESULTS_DIR="$COMPARISON_DIR/results"

# Interface to shape. Loopback, because the TLS certificates carry 127.0.0.1 as their only subject
# alternative name.
DEV=lo

# Prints an error and exits.
die() {
    echo "error: $*" >&2
    exit 1
}

log() {
    echo "[$(date +%H:%M:%S)] $*" >&2
}

# Locates the built comparison binary, building it if needed.
#
# The binary is built once and invoked directly for every repetition rather than going through
# `cargo bench` each time: a repetition runs two party processes concurrently, and two cargo
# invocations would serialize on the build lock.
find_comparison_bin() {
    if [[ -n "${COMPARISON_BIN:-}" && -x "${COMPARISON_BIN}" ]]; then
        echo "$COMPARISON_BIN"
        return
    fi

    (cd "$CRATE_ROOT" && cargo bench --bench comparison --no-run) >&2

    local bin
    bin="$(ls -t "$CRATE_ROOT"/target/release/deps/comparison-* 2>/dev/null \
        | grep -v '\.d$' | head -1)"
    [[ -n "$bin" && -x "$bin" ]] || die "could not locate the built comparison binary"
    echo "$bin"
}

# Runs a command with root privileges, preferring an already-root shell over invoking sudo.
as_root() {
    if [[ "$(id -u)" -eq 0 ]]; then
        "$@"
    else
        sudo "$@"
    fi
}
