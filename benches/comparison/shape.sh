#!/usr/bin/env bash
# Shapes loopback to match a scenario's simulated network, and restores it afterwards.
#
#   ./shape.sh setup <scenario>   apply a scenario's delay/rate/loss to the benchmark's ports
#   ./shape.sh teardown           restore everything this script changed
#   ./shape.sh status             show what is currently applied
#
# Needs root (it runs `tc`, `ip link` and `sysctl`); it re-invokes those through sudo when not
# already root. `teardown` is safe to run at any time, including after a crash or a reboot-less
# interruption — run_all.sh installs it as an exit trap, and it is idempotent.
#
# ## What it changes, and why each is necessary
#
# * **A `prio` qdisc with a netem band, selected by port filter.** Delay, rate and loss are applied
#   *only* to traffic on the benchmark's ports. Shaping loopback wholesale would put 50 ms of
#   latency and a 1 Mbit/s cap on every local service on the machine — editors, databases, display
#   servers all speak over `lo`. The qdisc's priomap is overridden to `1 1 1 ...` so that no
#   unclassified traffic can reach the shaped band through its type-of-service bits.
#
# * **Loopback MTU dropped to 1500.** Loopback defaults to a 65536-byte MTU, which makes the TCP
#   MSS about 65483 rather than the 1460 the simulator prices with. That distorts the loss-less
#   scenarios mildly (per-segment header overhead is charged against a segment 45x too large) and
#   the lossy one severely: the simulator's loss term is *linearly* proportional to MSS, and netem
#   would be dropping 64 KB units where the model assumes 1460-byte ones.
#
# * **GSO/TSO/GRO disabled on loopback.** Otherwise the kernel hands netem aggregated super-packets
#   of up to 64 KB even at a 1500-byte MTU, so a single netem drop discards ~44 segments at once
#   and its rate meter sees a handful of huge packets instead of a stream of small ones.
#
# * **`tcp_rmem`/`tcp_wmem` pinned — window-limited scenario only.** That scenario exists to test
#   the window term, which requires a window that does not autotune upward out of the regime. The
#   other two scenarios leave kernel autotuning alone, because there a large real window does not
#   change which term binds.
#
# The pre-existing value of every one of these is saved to `.shape-state` at setup and restored at
# teardown.

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

# netem's default backlog is 1000 packets, which a high bandwidth-delay-product shape can overrun —
# and an overrun shows up as extra loss, silently contaminating the loss-less scenarios. At
# 100 Mbit/s and 50 ms one way, about 417 segments are legitimately in flight.
NETEM_LIMIT=10000

# Handle of the netem band. Bands 1:1 and 1:2 stay plain pfifo and carry everything else.
SHAPED_BAND="1:3"

# Records the current value of everything setup is about to change, so teardown can put it back.
# Only written once per shaping session: re-running setup for a second scenario must not overwrite
# the saved originals with the first scenario's values.
save_original_state() {
    [[ -f "$SHAPE_STATE" ]] && return 0

    local mtu gso tso gro
    mtu="$(cat "/sys/class/net/$DEV/mtu")"
    gso="$(ethtool -k "$DEV" 2>/dev/null | awk '/^generic-segmentation-offload:/ {print $2}')"
    tso="$(ethtool -k "$DEV" 2>/dev/null | awk '/^tcp-segmentation-offload:/ {print $2}')"
    gro="$(ethtool -k "$DEV" 2>/dev/null | awk '/^generic-receive-offload:/ {print $2}')"

    {
        echo "ORIG_MTU=$mtu"
        echo "ORIG_GSO=${gso:-unknown}"
        echo "ORIG_TSO=${tso:-unknown}"
        echo "ORIG_GRO=${gro:-unknown}"
        # `sysctl -n` separates the three values with tabs; they are normalized to single spaces
        # here so teardown writes back the canonical form sysctl expects.
        echo "ORIG_TCP_RMEM='$(sysctl -n net.ipv4.tcp_rmem | tr -s '[:space:]' ' ' | sed 's/ $//')'"
        echo "ORIG_TCP_WMEM='$(sysctl -n net.ipv4.tcp_wmem | tr -s '[:space:]' ' ' | sed 's/ $//')'"
        echo "ORIG_CONGESTION=$(sysctl -n net.ipv4.tcp_congestion_control)"
    } > "$SHAPE_STATE"

    log "saved original $DEV state to $SHAPE_STATE"
}

# Sends traffic on the benchmark's ports, in both directions, into the shaped band.
#
# Both a source-port and a destination-port filter are installed per party: a loopback packet
# crosses this egress qdisc once on the way out and once on the reply, and only one of the two ends
# carries the listening port on any given packet.
add_port_filters() {
    local party port
    for (( party = 0; party < N_PARTIES; party++ )); do
        port=$(( BASE_PORT + party ))
        as_root tc filter add dev "$DEV" protocol ip parent 1: prio 1 u32 \
            match ip dport "$port" 0xffff flowid "$SHAPED_BAND"
        as_root tc filter add dev "$DEV" protocol ip parent 1: prio 1 u32 \
            match ip sport "$port" 0xffff flowid "$SHAPED_BAND"
    done
}

setup() {
    local scenario="${1:-}"
    [[ -n "$scenario" ]] || die "usage: shape.sh setup <scenario>"

    local bin
    bin="$(find_comparison_bin)"

    # The scenario table in scenarios.rs is the single source of truth: the same numbers that build
    # the simulator's ChannelConfig are read back here as shell variables, so the shaped link and
    # the simulated link cannot drift apart.
    local params
    params="$("$bin" params "$scenario")" || die "unknown scenario '$scenario'"
    eval "$params"

    save_original_state

    log "shaping $DEV for '$scenario': delay ${TC_DELAY_MS}ms (RTT ${RTT_MS}ms), rate ${TC_RATE_BITS}bit, loss ${TC_LOSS_PERCENT}%"

    # Start from a clean slate; a leftover qdisc from an interrupted run would otherwise stack.
    as_root tc qdisc del dev "$DEV" root 2>/dev/null || true

    as_root ip link set dev "$DEV" mtu "$TC_MTU_BYTES"

    # Verified rather than assumed: whether loopback lets these be turned off varies by kernel, and
    # a silent failure would leave netem metering and dropping 64 KB super-packets while every
    # other signal said the shape was applied. That distorts the lossy scenario most, since the
    # model's loss term is priced per 1460-byte segment.
    as_root ethtool -K "$DEV" gso off tso off gro off 2>/dev/null || true
    local still_on
    still_on="$(ethtool -k "$DEV" 2>/dev/null \
        | awk '/^(generic-segmentation|tcp-segmentation|generic-receive)-offload: on/ {print $1}')"
    if [[ -n "$still_on" ]]; then
        log "WARNING: could not disable on $DEV: ${still_on//$'\n'/ }"
        log "WARNING: netem will see aggregated packets; treat lossy results as unreliable"
    fi

    if [[ "$PIN_TCP_WINDOW" == "1" ]]; then
        log "pinning tcp_rmem/tcp_wmem to $PINNED_TCP_MEM_BYTES bytes for the window-limited regime"
        as_root sysctl -qw "net.ipv4.tcp_rmem=4096 $PINNED_TCP_MEM_BYTES $PINNED_TCP_MEM_BYTES"
        as_root sysctl -qw "net.ipv4.tcp_wmem=4096 $PINNED_TCP_MEM_BYTES $PINNED_TCP_MEM_BYTES"
    else
        # Restored explicitly rather than left alone. These settings are system-wide and survive a
        # scenario change, so an earlier scenario's pin would otherwise carry into this one --
        # running it against a constrained window instead of the kernel autotuning it assumes, and
        # keeping every other TCP connection on the machine capped for the rest of the suite.
        # shellcheck source=/dev/null
        source "$SHAPE_STATE"
        as_root sysctl -qw "net.ipv4.tcp_rmem=$ORIG_TCP_RMEM"
        as_root sysctl -qw "net.ipv4.tcp_wmem=$ORIG_TCP_WMEM"
    fi

    # priomap sends every unclassified packet to band 1:2 regardless of its TOS bits, leaving 1:3
    # reachable only through the explicit port filters below.
    as_root tc qdisc add dev "$DEV" root handle 1: prio bands 3 \
        priomap 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1

    local netem_args=(delay "${TC_DELAY_MS}ms" rate "${TC_RATE_BITS}bit" limit "$NETEM_LIMIT")
    if [[ "$TC_HAS_LOSS" == "1" ]]; then
        netem_args+=(loss "${TC_LOSS_PERCENT}%")
    fi

    as_root tc qdisc add dev "$DEV" parent "$SHAPED_BAND" handle 30: netem "${netem_args[@]}"
    add_port_filters

    log "shaped: ports $BASE_PORT-$(( BASE_PORT + N_PARTIES - 1 )) only; congestion control is $(sysctl -n net.ipv4.tcp_congestion_control)"
}

teardown() {
    as_root tc qdisc del dev "$DEV" root 2>/dev/null || true

    if [[ -f "$SHAPE_STATE" ]]; then
        # shellcheck source=/dev/null
        source "$SHAPE_STATE"

        as_root ip link set dev "$DEV" mtu "$ORIG_MTU"
        [[ "$ORIG_GSO" == "on" ]] && as_root ethtool -K "$DEV" gso on 2>/dev/null || true
        [[ "$ORIG_TSO" == "on" ]] && as_root ethtool -K "$DEV" tso on 2>/dev/null || true
        [[ "$ORIG_GRO" == "on" ]] && as_root ethtool -K "$DEV" gro on 2>/dev/null || true
        as_root sysctl -qw "net.ipv4.tcp_rmem=$ORIG_TCP_RMEM"
        as_root sysctl -qw "net.ipv4.tcp_wmem=$ORIG_TCP_WMEM"

        rm -f "$SHAPE_STATE"
        log "restored $DEV (mtu $ORIG_MTU) and TCP buffer settings"
    else
        log "no saved state; removed any root qdisc on $DEV and left the rest alone"
    fi
}

status() {
    echo "== $DEV mtu =="
    cat "/sys/class/net/$DEV/mtu"
    echo "== $DEV qdisc =="
    tc -s qdisc show dev "$DEV"
    echo "== $DEV filters =="
    tc filter show dev "$DEV" parent 1: 2>/dev/null || echo "(none)"
    echo "== tcp buffers =="
    sysctl net.ipv4.tcp_rmem net.ipv4.tcp_wmem net.ipv4.tcp_congestion_control
    echo "== saved state =="
    [[ -f "$SHAPE_STATE" ]] && cat "$SHAPE_STATE" || echo "(none: nothing to restore)"
}

case "${1:-}" in
    setup)    setup "${2:-}" ;;
    teardown) teardown ;;
    status)   status ;;
    *)        die "usage: shape.sh {setup <scenario>|teardown|status}" ;;
esac
