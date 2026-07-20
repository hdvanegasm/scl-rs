#!/usr/bin/env python3
"""Render the simulated-versus-real comparison from ``results/measurements.csv``.

Produces one figure per scenario plus a summary table:

    results/fig_bandwidth_limited.png
    results/fig_window_limited.png
    results/fig_lossy.png
    results/summary.md

Each figure shows, for both protocols in that scenario, the distribution of the real
repetitions (box + every individual run) with the simulator's point prediction marked on
top and labelled with its relative error.

Why one figure per scenario rather than one combined chart: the relative errors span
roughly 1% to 130%, so on a shared axis the validated regime collapses to a single pixel
beside the lossy one. Splitting keeps each panel on a scale where its own result is
legible, and the cross-scenario comparison moves to the summary table, where a 100x
dynamic range is not a problem.

Why the raw points and not just a box: in the lossy scenario the finding is the *spread*
-- identical trials landing seconds apart -- and a five-number summary hides exactly that.

Usage
-----
    python3 benches/comparison/plot.py                     # light theme, PNG
    python3 benches/comparison/plot.py --dark              # dark theme
    python3 benches/comparison/plot.py --format pdf        # vector output
    python3 benches/comparison/plot.py --csv path/to.csv --outdir path/
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from matplotlib.lines import Line2D

# ---------------------------------------------------------------------------------------
# Theme
#
# Series hues are slots 1-3 of the reference categorical palette, in fixed order. The set
# was checked with the palette validator over all pairs in both modes rather than picked
# by eye: an earlier blue/orange/violet choice passed in light mode but collapsed in dark
# (violet against blue measured OKLab dE 1.9 under protanopia -- indistinguishable).
# ---------------------------------------------------------------------------------------

THEMES = {
    "light": {
        "surface": "#fcfcfb",
        "page": "#f9f9f7",
        "ink": "#0b0b0b",
        "secondary": "#52514e",
        "muted": "#898781",
        "grid": "#e1e0d9",
        "baseline": "#c3c2b7",
        "real": "#2a78d6",
        "nominal": "#008300",
        "calibrated": "#e87ba4",
    },
    "dark": {
        "surface": "#1a1a19",
        "page": "#0d0d0d",
        "ink": "#ffffff",
        "secondary": "#c3c2b7",
        "muted": "#898781",
        "grid": "#2c2c2a",
        "baseline": "#383835",
        "real": "#3987e5",
        "nominal": "#008300",
        "calibrated": "#d55181",
    },
}

# Headline for each scenario: what the regime tests and what the crate README concludes.
SCENARIO_TITLES = {
    "bandwidth_limited": (
        "Bandwidth-limited, loss-less",
        "the serialization term -- the regime the model is validated in",
    ),
    "window_limited": (
        "Window-limited, loss-less",
        "the window term -- the model's form holds, but the window needs calibrating",
    ),
    "lossy": (
        "Lossy",
        "the sqrt(3/2p) term -- a standard formula applied outside its validity domain",
    ),
}

PROTOCOL_LABELS = {
    "ping_pong": "PingPong\n(round-dominated)",
    "bulk_transfer": "BulkTransfer\n(bandwidth-dominated)",
}

# Order scenarios and protocols are reported in, matching scenarios.rs.
SCENARIO_ORDER = ["bandwidth_limited", "window_limited", "lossy"]
PROTOCOL_ORDER = ["ping_pong", "bulk_transfer"]


def load(csv_path: Path) -> pd.DataFrame:
    """Reads the measurements CSV, keeping only the initiator's rows.

    Party 1 is recorded too, but it opens its span on a receive rather than a send and so
    sits about half a round trip lower in *both* backends -- it is a robustness check, not
    a second series to plot.
    """
    if not csv_path.exists():
        sys.exit(
            f"error: {csv_path} not found.\n"
            "Run the suite first:  ./benches/comparison/run_all.sh"
        )

    frame = pd.read_csv(csv_path)
    missing = {"scenario", "protocol", "source", "variant", "party", "elapsed_secs"} - set(
        frame.columns
    )
    if missing:
        sys.exit(f"error: {csv_path} is missing columns: {', '.join(sorted(missing))}")

    frame = frame[frame["party"] == 0]
    if frame.empty:
        sys.exit(f"error: no party-0 rows in {csv_path}")
    return frame


def relative_error(simulated: float, observed: float) -> float:
    """Signed error of the prediction against an observed centre, as a fraction.

    Positive means the simulator over-predicts (predicted slower than reality). This is
    the same direction the crate README states its numbers in.
    """
    return (simulated - observed) / observed


def format_error(value: float) -> str:
    return f"{value:+.1%}".replace("%", " %")


def draw_panel(
    axis,
    theme: dict,
    row: int,
    real: np.ndarray,
    predictions: list[tuple[str, float]],
    rng: np.random.Generator,
) -> None:
    """Draws one protocol's distribution and its prediction marker(s) at height ``row``."""
    # The box carries the five-number summary; fliers are suppressed because every point
    # is drawn anyway and a doubled mark reads as two observations.
    box = axis.boxplot(
        [real],
        positions=[row],
        widths=0.32,
        orientation="horizontal",
        showfliers=False,
        patch_artist=True,
        medianprops={"color": theme["ink"], "linewidth": 1.6},
        whiskerprops={"color": theme["baseline"], "linewidth": 1.0},
        capprops={"color": theme["baseline"], "linewidth": 1.0},
    )
    for patch in box["boxes"]:
        patch.set_facecolor(theme["real"])
        patch.set_alpha(0.14)
        patch.set_edgecolor(theme["real"])
        patch.set_linewidth(1.2)

    # Every repetition, jittered off the centre line so overlapping runs stay countable.
    # The surface-coloured ring is the separator between coincident marks -- not an
    # outline drawn around each one.
    jitter = rng.uniform(-0.085, 0.085, size=real.size)
    axis.plot(
        real,
        np.full(real.size, row) + jitter,
        linestyle="none",
        marker="o",
        markersize=4.5,
        markerfacecolor=theme["real"],
        markeredgecolor=theme["surface"],
        markeredgewidth=0.7,
        alpha=0.75,
        zorder=3,
    )

    # The mean is marked separately from the box's median. For the lossy regime this is
    # not decoration: the sqrt(3/2p) formula predicts an ensemble *mean*, while a box's
    # centre line is the median -- and on a right-skewed distribution the median quietly
    # flatters the model against what it actually claims. It sits above the box and wears
    # a triangle rather than a tick, because a second vertical line beside the median
    # reads as a second median.
    axis.plot(
        [real.mean()],
        [row - 0.26],
        marker="v",
        markersize=7,
        markerfacecolor=theme["secondary"],
        markeredgecolor=theme["surface"],
        markeredgewidth=0.8,
        zorder=4,
    )

    # Prediction markers, stacked downward if a scenario has more than one. The stem runs
    # from the distribution to the diamond so the two read as one mark: in the lossy panel
    # the prediction lands far outside the box, and a detached rule there looks like a
    # stray gridline rather than the same object as its label.
    for index, (variant, predicted) in enumerate(predictions):
        offset = 0.30 + 0.22 * index
        colour = theme[variant]
        axis.plot(
            [predicted, predicted],
            [row - 0.19, row + offset],
            color=colour,
            linewidth=1.3,
            zorder=5,
        )
        axis.plot(
            [predicted],
            [row + offset],
            marker="D",
            markersize=9,
            markerfacecolor=colour,
            markeredgecolor=theme["surface"],
            markeredgewidth=1.4,
            zorder=6,
        )
        # Direct label. Also the mandated relief for the calibrated hue, which sits below
        # 3:1 against the light surface and so may not carry meaning by colour alone.
        axis.annotate(
            format_error(relative_error(predicted, float(np.median(real)))),
            xy=(predicted, row + offset),
            xytext=(9, 0),
            textcoords="offset points",
            va="center",
            ha="left",
            fontsize=9,
            color=theme["secondary"],
        )


def render_scenario(
    frame: pd.DataFrame,
    scenario: str,
    theme: dict,
    outdir: Path,
    suffix: str,
    dpi: int,
    stem: str,
) -> Path:
    """Renders one scenario's figure and returns the path written."""
    subset = frame[frame["scenario"] == scenario]
    protocols = [p for p in PROTOCOL_ORDER if p in set(subset["protocol"])]

    # One subplot per protocol, each on its own x-scale -- small multiples, not a shared
    # axis. Within a scenario the two protocols can sit far apart (3.03 s against 1.63 s
    # in the window-limited regime) while each distribution is tighter than 2 % of its own
    # value, so a shared axis renders both boxes as invisible slivers. Independent scales
    # is the same argument that splits the scenarios into separate figures, one level down.
    figure, axes = plt.subplots(
        nrows=len(protocols),
        figsize=(9.0, 2.4 + 1.35 * len(protocols)),
        squeeze=False,
    )
    axes = axes.ravel()
    figure.patch.set_facecolor(theme["page"])
    rng = np.random.default_rng(20260720)

    seen_variants: list[str] = []
    for axis, protocol in zip(axes, protocols):
        axis.set_facecolor(theme["surface"])
        rows = subset[subset["protocol"] == protocol]
        real = rows[rows["source"] == "real"]["elapsed_secs"].to_numpy()

        simulated = rows[rows["source"] == "sim"]
        predictions = [
            (variant, float(simulated[simulated["variant"] == variant]["elapsed_secs"].iloc[0]))
            for variant in ("nominal", "calibrated")
            if not simulated[simulated["variant"] == variant].empty
        ]
        seen_variants += [v for v, _ in predictions if v not in seen_variants]
        if real.size:
            draw_panel(axis, theme, 0, real, predictions, rng)

        axis.set_yticks([0])
        axis.set_yticklabels([PROTOCOL_LABELS.get(protocol, protocol)], fontsize=9.5)
        axis.invert_yaxis()
        # Room above the row for the mean triangle and below it for the prediction
        # diamonds, without leaving the row floating in empty space.
        axis.set_ylim(0.34 + 0.22 * len(predictions), -0.46)

        # Deliberately not anchored at zero. A zero baseline is required when mark
        # *length* encodes magnitude (bars); here position encodes a timing, and these
        # distributions are in places tighter than 1 % of their value.
        values = np.concatenate([real, [p for _, p in predictions]]) if real.size else None
        if values is not None:
            low, high = float(values.min()), float(values.max())
            pad = max((high - low) * 0.14, high * 0.012)
            axis.set_xlim(low - pad, high + pad * 2.6)

        # Recessive chrome: solid hairlines only, on the value axis only.
        axis.grid(axis="x", color=theme["grid"], linewidth=0.8, linestyle="-")
        axis.set_axisbelow(True)
        for side in ("top", "right", "left"):
            axis.spines[side].set_visible(False)
        axis.spines["bottom"].set_color(theme["baseline"])
        axis.spines["bottom"].set_linewidth(0.8)
        axis.tick_params(colors=theme["muted"], labelsize=9, length=0)
        for label in axis.get_yticklabels():
            label.set_color(theme["ink"])

    axis = axes[-1]
    axis.set_xlabel("elapsed time (seconds)", fontsize=9.5, color=theme["secondary"])

    heading, explanation = SCENARIO_TITLES.get(scenario, (scenario, ""))
    params = subset.iloc[0]
    repetitions = len(subset[subset["source"] == "real"]) // max(len(protocols), 1)
    figure.suptitle(
        heading, x=0.015, ha="left", fontsize=13, color=theme["ink"], fontweight="semibold"
    )
    figure.text(
        0.015,
        0.945,
        f"{explanation}\n"
        f"RTT {params['rtt_ms']} ms · {int(params['bandwidth_bps']) / 1e6:g} Mbit/s · "
        f"loss {float(params['loss_fraction']):.2%} · "
        f"{repetitions} repetitions per protocol · each panel has its own time scale",
        ha="left",
        va="top",
        fontsize=9,
        color=theme["muted"],
    )

    handles = [
        Line2D(
            [], [], linestyle="none", marker="o", markersize=6,
            markerfacecolor=theme["real"], markeredgecolor=theme["surface"],
            label="real run (one per repetition)",
        ),
        Line2D(
            [], [], linestyle="none", marker="v", markersize=7,
            markerfacecolor=theme["secondary"], markeredgecolor=theme["surface"],
            label="real mean",
        ),
    ]
    handles += [
        Line2D(
            [], [], linestyle="none", marker="D", markersize=7,
            markerfacecolor=theme[variant], markeredgecolor=theme["surface"],
            label=f"simulated ({variant})",
        )
        for variant in seen_variants
    ]
    # Below the panels rather than above them: the header already carries a title and
    # three lines of parameters, and a legend up there collides with them.
    legend = figure.legend(
        handles=handles,
        loc="lower center",
        ncol=len(handles),
        frameon=False,
        fontsize=8.5,
        handletextpad=0.4,
        columnspacing=1.8,
    )
    for text in legend.get_texts():
        text.set_color(theme["secondary"])

    figure.tight_layout(rect=(0, 0.06, 1, 0.90))
    path = outdir / f"{stem}{scenario}.{suffix}"
    figure.savefig(path, dpi=dpi, facecolor=figure.get_facecolor())
    plt.close(figure)
    return path


def write_summary(frame: pd.DataFrame, outdir: Path) -> Path:
    """Writes the cross-scenario error table.

    This is where the comparison the split figures give up is recovered. A table handles
    the ~100x dynamic range between the validated and the lossy regime that no shared
    axis can, and it carries the mean-based error alongside the median-based one.
    """
    lines = [
        "# Simulated vs. real: relative error",
        "",
        "Positive means the simulator over-predicts (predicted slower than reality).",
        "Party 0 only. `vs mean` matters most in the lossy regime, where the underlying",
        "formula predicts an ensemble mean rather than any single run.",
        "",
        "**The calibrated `bulk_transfer` row is circular** and is not evidence: the window",
        "it uses was recovered from that protocol's own median, so it agrees with it by",
        "construction. The out-of-sample check is the calibrated `ping_pong` row, which was",
        "not used to fit the window.",
        "",
        "| Scenario | Protocol | Prediction | Sim (s) | Real median (s) | Real mean (s) | "
        "Real min-max (s) | vs median | vs mean |",
        "|---|---|---|---|---|---|---|---|---|",
    ]

    for scenario in [s for s in SCENARIO_ORDER if s in set(frame["scenario"])]:
        subset = frame[frame["scenario"] == scenario]
        for protocol in [p for p in PROTOCOL_ORDER if p in set(subset["protocol"])]:
            rows = subset[subset["protocol"] == protocol]
            real = rows[rows["source"] == "real"]["elapsed_secs"].to_numpy()
            if real.size == 0:
                continue
            median, mean = float(np.median(real)), float(real.mean())
            simulated = rows[rows["source"] == "sim"]
            for variant in ("nominal", "calibrated"):
                match = simulated[simulated["variant"] == variant]
                if match.empty:
                    continue
                predicted = float(match["elapsed_secs"].iloc[0])
                lines.append(
                    f"| {scenario} | {protocol} | {variant} | {predicted:.3f} | "
                    f"{median:.3f} | {mean:.3f} | {real.min():.3f}-{real.max():.3f} | "
                    f"{format_error(relative_error(predicted, median))} | "
                    f"{format_error(relative_error(predicted, mean))} |"
                )

    path = outdir / "summary.md"
    path.write_text("\n".join(lines) + "\n")
    return path


def main() -> None:
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--csv", type=Path, default=here / "results" / "measurements.csv")
    parser.add_argument("--outdir", type=Path, default=None)
    parser.add_argument("--dark", action="store_true", help="render for a dark surface")
    parser.add_argument("--format", default="png", choices=("png", "pdf", "svg"))
    parser.add_argument(
        "--dpi",
        type=int,
        default=200,
        help="raster resolution; 300 for the figures committed under docs/ (default: 200)",
    )
    parser.add_argument(
        "--stem",
        default="fig_",
        help="output filename prefix (default: fig_)",
    )
    parser.add_argument(
        "--no-summary",
        action="store_true",
        help="skip summary.md, e.g. when re-rendering only the committed images",
    )
    args = parser.parse_args()

    outdir = args.outdir or args.csv.parent
    outdir.mkdir(parents=True, exist_ok=True)

    frame = load(args.csv)
    theme = THEMES["dark" if args.dark else "light"]

    scenarios = [s for s in SCENARIO_ORDER if s in set(frame["scenario"])]
    if not scenarios:
        sys.exit(f"error: no known scenarios in {args.csv}")

    for scenario in scenarios:
        if frame[(frame["scenario"] == scenario) & (frame["source"] == "real")].empty:
            print(f"skipping {scenario}: no real runs recorded", file=sys.stderr)
            continue
        written = render_scenario(
            frame, scenario, theme, outdir, args.format, args.dpi, args.stem
        )
        print(f"wrote {written}")

    if not args.no_summary:
        print(f"wrote {write_summary(frame, outdir)}")


if __name__ == "__main__":
    main()
