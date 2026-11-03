#!/usr/bin/env python3
"""A6 chart generator (stdlib only).

Reads a TSV with fixed columns:
  budget arm WER S D I RTF peak_live peak_dormant late_rescues dWER CI_lo CI_hi

Writes SVG line charts (no matplotlib):
  - A6-wer-vs-budget.svg  (WER vs budget by arm, with CI error bars when present)
  - A6-rtf-vs-budget.svg  (RTF vs budget by arm)

Usage:
  python3 docs/results/A6-charts.py --sample
      Read docs/results/A6-sample.tsv (next to this script) and write SVGs
      into the same directory.

  python3 docs/results/A6-charts.py --input path/to/real.tsv --outdir docs/results
      Read a real TSV with the same schema and write SVGs to outdir.

  python3 docs/results/A6-charts.py --input in.tsv --outdir out --prefix REAL-
      Custom output filename prefix (default: A6-).

Only the Python standard library is used.
"""

import argparse
import csv
import sys
from pathlib import Path

REQUIRED_COLUMNS = [
    "budget", "arm", "WER", "S", "D", "I", "RTF",
    "peak_live", "peak_dormant", "late_rescues",
    "dWER", "CI_lo", "CI_hi",
]

ARM_ORDER = ["A", "B", "C"]
ARM_COLORS = {
    "A": "#1f77b4",
    "B": "#ff7f0e",
    "C": "#2ca02c",
}
ARM_DASH = {
    "A": "",
    "B": "",
    "C": "",
}

MISSING_TOKENS = {"", "n/a", "N/A", "--", "-", "na", "null", "None"}


def parse_float(s):
    if s is None:
        return None
    t = str(s).strip()
    if t in MISSING_TOKENS:
        return None
    try:
        return float(t)
    except ValueError:
        return None


def parse_int(s):
    f = parse_float(s)
    if f is None:
        return None
    return int(round(f))


def load_tsv(path):
    path = Path(path)
    with path.open("r", newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        if reader.fieldnames is None:
            raise ValueError("empty TSV: %s" % path)
        header = [h.strip() for h in reader.fieldnames]
        missing = [c for c in REQUIRED_COLUMNS if c not in header]
        if missing:
            raise ValueError(
                "TSV %s missing columns: %s (header=%s)"
                % (path, ",".join(missing), ",".join(header))
            )
        rows = []
        for i, rec in enumerate(reader, start=2):
            budget = parse_int(rec.get("budget"))
            arm = (rec.get("arm") or "").strip().upper()
            if budget is None or arm == "":
                raise ValueError("row %d: bad budget/arm: %r" % (i, rec))
            rows.append({
                "budget": budget,
                "arm": arm,
                "WER": parse_float(rec.get("WER")),
                "S": parse_int(rec.get("S")),
                "D": parse_int(rec.get("D")),
                "I": parse_int(rec.get("I")),
                "RTF": parse_float(rec.get("RTF")),
                "peak_live": parse_int(rec.get("peak_live")),
                "peak_dormant": parse_int(rec.get("peak_dormant")),
                "late_rescues": parse_int(rec.get("late_rescues")),
                "dWER": parse_float(rec.get("dWER")),
                "CI_lo": parse_float(rec.get("CI_lo")),
                "CI_hi": parse_float(rec.get("CI_hi")),
            })
    if not rows:
        raise ValueError("no data rows in %s" % path)
    budgets = sorted({r["budget"] for r in rows})
    arms = [a for a in ARM_ORDER if any(r["arm"] == a for r in rows)]
    extra = sorted({r["arm"] for r in rows} - set(ARM_ORDER))
    arms.extend(extra)
    return rows, budgets, arms


def esc(s):
    return (
        str(s).replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def nice_ticks(vmin, vmax, n=5):
    if vmin == vmax:
        pad = abs(vmin) * 0.1 if vmin != 0 else 0.05
        vmin -= pad
        vmax += pad
    span = vmax - vmin
    step0 = span / max(n - 1, 1)
    mag = 10.0 ** (-3)
    # find order of magnitude
    import math
    if step0 > 0:
        exp = math.floor(math.log10(step0))
        mag = 10.0 ** exp
    for mult in (1, 2, 2.5, 5, 10):
        step = mult * mag
        if step >= step0:
            break
    else:
        step = 10 * mag
    import math as _m
    lo = _m.floor(vmin / step) * step
    hi = _m.ceil(vmax / step) * step
    ticks = []
    v = lo
    for _ in range(64):
        if v > hi + 1e-12:
            break
        ticks.append(v)
        v += step
    return lo, hi, ticks


def fmt_tick(v):
    av = abs(v)
    if av >= 100:
        return "%d" % round(v)
    if av >= 10:
        s = "%.1f" % v
    elif av >= 1:
        s = "%.2f" % v
    else:
        s = "%.3f" % v
    # strip trailing zeros but keep at least one decimal for small values
    if "." in s:
        s = s.rstrip("0").rstrip(".")
        if s == "-0":
            s = "0"
    return s


def line_chart_svg(rows, budgets, arms, value_key, ci_lo_key, ci_hi_key,
                   title, ylabel, source_note):
    W, H = 720, 440
    L, R, T, B = 72, 150, 46, 62
    pw = W - L - R
    ph = H - T - B

    vals = []
    for r in rows:
        v = r.get(value_key)
        if v is not None:
            vals.append(v)
        if ci_lo_key and ci_hi_key:
            lo = r.get(ci_lo_key)
            hi = r.get(ci_hi_key)
            # CI columns for WER rows hold absolute WER bounds; for
            # dWER rows they would be deltas. Here we only use them
            # when they look like absolute bounds on the same scale:
            # caller passes CI keys only for the WER chart where the
            # sample TSV stores absolute WER CI per row is NOT present;
            # error bars instead come from per-arm CI only if provided.
            # We treat CI_lo/CI_hi as absolute bounds when they bracket v.
            if lo is not None and hi is not None and v is not None:
                # Heuristic: if lo/hi bracket v, include them in range.
                if lo <= v <= hi or (lo <= hi and abs(hi - lo) < 1.0):
                    vals.append(lo)
                    vals.append(hi)
    if not vals:
        raise ValueError("no plottable values for %s" % value_key)
    vmin, vmax = min(vals), max(vals)
    pad = (vmax - vmin) * 0.12 if vmax != vmin else (abs(vmax) * 0.1 or 0.05)
    lo, hi, ticks = nice_ticks(vmin - pad, vmax + pad, n=6)

    def x_of(budget):
        if len(budgets) == 1:
            return L + pw / 2.0
        i = budgets.index(budget)
        return L + pw * i / (len(budgets) - 1)

    def y_of(v):
        if hi == lo:
            return T + ph / 2.0
        return T + ph * (1.0 - (v - lo) / (hi - lo))

    parts = []
    parts.append('<?xml version="1.0" encoding="UTF-8"?>')
    parts.append(
        '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" '
        'viewBox="0 0 %d %d" role="img">' % (W, H, W, H)
    )
    parts.append('<rect x="0" y="0" width="%d" height="%d" fill="white"/>' % (W, H))
    parts.append(
        '<text x="%d" y="26" text-anchor="middle" font-family="sans-serif" '
        'font-size="16" font-weight="bold">%s</text>' % (W // 2, esc(title))
    )
    # axes
    parts.append(
        '<line x1="%d" y1="%d" x2="%d" y2="%d" stroke="black" stroke-width="1.5"/>'
        % (L, T, L, T + ph)
    )
    parts.append(
        '<line x1="%d" y1="%d" x2="%d" y2="%d" stroke="black" stroke-width="1.5"/>'
        % (L, T + ph, L + pw, T + ph)
    )
    # grid + y ticks
    for tv in ticks:
        y = y_of(tv)
        parts.append(
            '<line x1="%d" y1="%.1f" x2="%d" y2="%.1f" stroke="#ddd" stroke-width="1"/>'
            % (L, y, L + pw, y)
        )
        parts.append(
            '<text x="%d" y="%.1f" text-anchor="end" dominant-baseline="middle" '
            'font-family="sans-serif" font-size="11">%s</text>'
            % (L - 8, y + 1, esc(fmt_tick(tv)))
        )
    # x ticks
    for b in budgets:
        x = x_of(b)
        parts.append(
            '<line x1="%.1f" y1="%d" x2="%.1f" y2="%d" stroke="#ddd" stroke-width="1"/>'
            % (x, T, x, T + ph)
        )
        parts.append(
            '<text x="%.1f" y="%d" text-anchor="middle" font-family="sans-serif" '
            'font-size="12">%s</text>' % (x, T + ph + 22, esc(str(b)))
        )
    parts.append(
        '<text x="%d" y="%d" text-anchor="middle" font-family="sans-serif" '
        'font-size="12">Budget</text>' % (L + pw // 2, H - 8)
    )
    parts.append(
        '<text x="16" y="%d" text-anchor="middle" font-family="sans-serif" '
        'font-size="12" transform="rotate(-90 16 %d)">%s</text>'
        % (T + ph // 2, T + ph // 2, esc(ylabel))
    )
    # series
    for arm in arms:
        color = ARM_COLORS.get(arm, "#333333")
        pts = []
        for b in budgets:
            cell = next((r for r in rows if r["budget"] == b and r["arm"] == arm), None)
            if cell is None or cell.get(value_key) is None:
                continue
            pts.append((b, cell[value_key], cell))
        if not pts:
            continue
        # error bars first (under line)
        if ci_lo_key and ci_hi_key:
            for b, v, cell in pts:
                lo_v = cell.get(ci_lo_key)
                hi_v = cell.get(ci_hi_key)
                # Only draw when lo/hi plausibly bracket v on the same
                # absolute scale (WER chart with absolute CI). Skip
                # dWER-style deltas.
                if lo_v is None or hi_v is None or v is None:
                    continue
                if not (lo_v <= v <= hi_v):
                    continue
                x = x_of(b)
                yv = y_of(v)
                ylo = y_of(lo_v)
                yhi = y_of(hi_v)
                parts.append(
                    '<line x1="%.1f" y1="%.1f" x2="%.1f" y2="%.1f" '
                    'stroke="%s" stroke-width="2" opacity="0.55"/>'
                    % (x, ylo, x, yhi, color)
                )
                parts.append(
                    '<line x1="%.1f" y1="%.1f" x2="%.1f" y2="%.1f" '
                    'stroke="%s" stroke-width="2" opacity="0.55"/>'
                    % (x - 5, ylo, x + 5, ylo, color)
                )
                parts.append(
                    '<line x1="%.1f" y1="%.1f" x2="%.1f" y2="%.1f" '
                    'stroke="%s" stroke-width="2" opacity="0.55"/>'
                    % (x - 5, yhi, x + 5, yhi, color)
                )
                _ = yv
        # polyline
        coord = " ".join("%.1f,%.1f" % (x_of(b), y_of(v)) for b, v, _ in pts)
        dash = ARM_DASH.get(arm, "")
        dash_attr = (' stroke-dasharray="%s"' % dash) if dash else ""
        parts.append(
            '<polyline points="%s" fill="none" stroke="%s" stroke-width="2.5"%s/>'
            % (coord, color, dash_attr)
        )
        for b, v, _ in pts:
            x = x_of(b)
            y = y_of(v)
            parts.append(
                '<circle cx="%.1f" cy="%.1f" r="4.5" fill="%s" stroke="white" '
                'stroke-width="1.5"/>' % (x, y, color)
            )
    # legend
    lx = L + pw + 16
    ly = T + 6
    parts.append(
        '<text x="%d" y="%d" font-family="sans-serif" font-size="12" '
        'font-weight="bold">Arm</text>' % (lx, ly)
    )
    for i, arm in enumerate(arms):
        color = ARM_COLORS.get(arm, "#333333")
        y = ly + 20 + i * 22
        parts.append(
            '<line x1="%d" y1="%d" x2="%d" y2="%d" stroke="%s" stroke-width="3"/>'
            % (lx, y, lx + 28, y, color)
        )
        parts.append(
            '<circle cx="%d" cy="%d" r="4" fill="%s"/>' % (lx + 14, y, color)
        )
        label = "Arm %s" % arm
        if arm == "A":
            label += " (1-best)"
        elif arm == "B":
            label += " (prune)"
        elif arm == "C":
            label += " (dormant)"
        parts.append(
            '<text x="%d" y="%d" dominant-baseline="middle" '
            'font-family="sans-serif" font-size="12">%s</text>'
            % (lx + 34, y + 1, esc(label))
        )
    # source note
    parts.append(
        '<text x="%d" y="%d" font-family="sans-serif" font-size="10" fill="#555">%s</text>'
        % (L, H - 26, esc(source_note))
    )
    parts.append("</svg>")
    return "\n".join(parts) + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description="A6 chart generator (stdlib only).")
    ap.add_argument("--sample", action="store_true",
                    help="use the bundled A6-sample.tsv next to this script")
    ap.add_argument("--input", default=None,
                    help="input TSV path (same schema as the sample)")
    ap.add_argument("--outdir", default=None,
                    help="output directory for SVGs (default: TSV dir)")
    ap.add_argument("--prefix", default="A6-",
                    help="output filename prefix (default: A6-)")
    args = ap.parse_args(argv)

    script_dir = Path(__file__).resolve().parent
    if args.sample:
        in_path = script_dir / "A6-sample.tsv"
        outdir = Path(args.outdir) if args.outdir else script_dir
        source_note = "Sample synthetic TSV: A6-sample.tsv (11 ref words, 4 utts) — NOT a real result."
    else:
        if not args.input:
            ap.error("need --sample or --input PATH")
        in_path = Path(args.input)
        outdir = Path(args.outdir) if args.outdir else in_path.resolve().parent
        source_note = "Source: %s" % in_path.name

    if not in_path.exists():
        print("input not found: %s" % in_path, file=sys.stderr)
        return 2

    rows, budgets, arms = load_tsv(in_path)
    outdir.mkdir(parents=True, exist_ok=True)

    # WER chart: error bars only when CI_lo/CI_hi bracket WER on the
    # same absolute scale. The sample TSV stores dWER-style CI for C
    # rows (deltas), so no bars are drawn for the sample — real TSVs
    # with absolute per-cell WER CI will get bars automatically.
    wer_svg = line_chart_svg(
        rows, budgets, arms, "WER", "CI_lo", "CI_hi",
        "A6: WER vs budget by arm", "WER", source_note,
    )
    rtf_svg = line_chart_svg(
        rows, budgets, arms, "RTF", None, None,
        "A6: RTF vs budget by arm", "RTF", source_note,
    )

    wer_path = outdir / (args.prefix + "wer-vs-budget.svg")
    rtf_path = outdir / (args.prefix + "rtf-vs-budget.svg")
    wer_path.write_text(wer_svg, encoding="utf-8")
    rtf_path.write_text(rtf_svg, encoding="utf-8")
    print("wrote %s (%d budgets, arms %s)" % (wer_path, len(budgets), ",".join(arms)))
    print("wrote %s" % rtf_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
