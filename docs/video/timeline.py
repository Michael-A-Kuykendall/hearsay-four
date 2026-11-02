#!/usr/bin/env python3
"""Lane/bead timeline graphic for the submission video (shot E).

Reads lanes.tsv (same dir, verified bead/git facts — see its header),
writes timeline.svg. Stdlib only. Run: python3 docs/video/timeline.py.
"""
import datetime as dt
import html
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
TSV = HERE / "lanes.tsv"
OUT = HERE / "timeline.svg"

# Display window: event build window Oct 26 -> deadline Nov 4.
T0 = dt.date(2026, 10, 26)
T1 = dt.date(2026, 11, 4)
DEADLINE = dt.date(2026, 11, 4)  # 9:00 PM ET Nov 4
NOW = dt.date(2026, 11, 2)  # data verified date (lanes.tsv header)

W, H_LEFT, ROW_H, TOP, BOTTOM = 1280, 300, 44, 70, 60
COLORS = {"closed": "#2f7d32", "in_progress": "#d97b00", "open": "#4a6fa5"}
BG, FG, GRID = "#ffffff", "#1a1a1a", "#d8d8d8"


def x_of(day: dt.date) -> float:
    span = (T1 - T0).days
    return H_LEFT + (W - H_LEFT - 40) * ((day - T0).days / span)


def parse():
    rows = []
    for line in TSV.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        assert len(parts) == 7, f"want 7 cols, got {len(parts)}: {line[:60]}"
        lane, bead, label, start, end, status, note = parts
        assert status in COLORS, f"bad status {status}"
        s = dt.date.fromisoformat(start)
        e = NOW if end == ">" else dt.date.fromisoformat(end)
        # Clamp pre-event history to the window with a "<" marker.
        clipped = s < T0
        if clipped:
            s = T0
        rows.append(dict(lane=lane, bead=bead, label=label, start=s, end=e,
                         status=status, note=note, clipped=clipped,
                         ongoing=end == ">"))
    return rows


def esc(s: str) -> str:
    return html.escape(s, quote=True)


def main() -> None:
    rows = parse()
    h = TOP + ROW_H * len(rows) + BOTTOM
    el = []
    el.append(f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" '
              f'height="{h}" viewBox="0 0 {W} {h}" font-family="sans-serif">')
    el.append(f'<rect width="{W}" height="{h}" fill="{BG}"/>')
    el.append(f'<text x="20" y="30" font-size="22" font-weight="bold" '
              f'fill="{FG}">Hearsay-IV — build lanes (bead-verified, Nov 2)</text>')
    el.append(f'<text x="20" y="52" font-size="13" fill="#555">green=closed '
              f'· orange=in progress · blue=open · data: lanes.tsv '
              f'(bd + git log)</text>')
    # Day gridlines + labels.
    day = T0
    while day <= T1:
        x = x_of(day)
        el.append(f'<line x1="{x:.1f}" y1="{TOP - 12}" x2="{x:.1f}" '
                  f'y2="{h - BOTTOM + 8}" stroke="{GRID}"/>')
        el.append(f'<text x="{x:.1f}" y="{TOP - 18}" font-size="12" '
                  f'text-anchor="middle" fill="#555">{day.strftime("%b %d")}</text>')
        day += dt.timedelta(days=1)
    # Deadline + today markers.
    el.append(f'<line x1="{x_of(DEADLINE):.1f}" y1="{TOP - 12}" '
              f'x2="{x_of(DEADLINE):.1f}" y2="{h - BOTTOM + 8}" '
              f'stroke="#c00" stroke-dasharray="5,4" stroke-width="2"/>')
    el.append(f'<text x="{x_of(DEADLINE):.1f}" y="{h - BOTTOM + 28}" '
              f'font-size="12" text-anchor="middle" fill="#c00">deadline Nov 4 9PM ET</text>')
    el.append(f'<line x1="{x_of(NOW):.1f}" y1="{TOP - 12}" '
              f'x2="{x_of(NOW):.1f}" y2="{h - BOTTOM + 8}" '
              f'stroke="#888" stroke-dasharray="3,3"/>')
    el.append(f'<text x="{x_of(NOW):.1f}" y="{h - BOTTOM + 28}" '
              f'font-size="12" text-anchor="middle" fill="#555">today Nov 2</text>')
    # Rows.
    for i, r in enumerate(rows):
        y = TOP + i * ROW_H
        x0, x1 = x_of(r["start"]), x_of(r["end"])
        if x1 - x0 < 6:  # single-day items stay visible
            x1 = x0 + 6
        color = COLORS[r["status"]]
        tag = f'{r["lane"]}' + (f' · {r["bead"]}' if r["bead"] != "—" else "")
        el.append(f'<text x="20" y="{y + 20}" font-size="14" font-weight="bold" '
                  f'fill="{FG}">{esc(tag)}</text>')
        el.append(f'<rect x="{x0:.1f}" y="{y + 4}" width="{x1 - x0:.1f}" '
                  f'height="22" rx="4" fill="{color}" opacity="0.85"/>')
        if r["ongoing"]:
            el.append(f'<polygon points="{x1:.1f},{y + 4} {x1 + 12:.1f},{y + 15} '
                      f'{x1:.1f},{y + 26}" fill="{color}"/>')
        lab = ("◀ " if r["clipped"] else "") + r["label"]
        el.append(f'<text x="{x0 + 6:.1f}" y="{y + 21}" font-size="12.5" fill="#fff">'
                  f'{esc(lab[:72])}</text>')
        el.append(f'<title>{esc(r["label"])} — {esc(r["note"])}</title>')
    el.append("</svg>")
    OUT.write_text("\n".join(el) + "\n")
    print(f"wrote {OUT} ({len(rows)} lanes)")


if __name__ == "__main__":
    sys.exit(main())
