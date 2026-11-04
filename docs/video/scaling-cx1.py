#!/usr/bin/env python3
"""CX1 growth-exponent before/after chart for the submission video.

Reads docs/video/scaling-cx1.dat (logged timings + logged k values with
source citations), fits k with the same least-squares log-log estimator as
the CX1 cage (src/fabric/tests/decision_cx1.rs `growth_exponent`), asserts
the fits match the logged values, and writes docs/video/scaling-cx1.png.

Stdlib only (zlib + struct PNG writer, embedded 5x7 bitmap font): no
matplotlib, no PIL. Run: python3 docs/video/scaling-cx1.py.
"""
import math
import struct
import sys
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
DAT = HERE / "scaling-cx1.dat"
OUT = HERE / "scaling-cx1.png"

TOL = 0.01  # fit-vs-logged tolerance (logged points are 2-3 sig figs)


def growth_exponent(times):
    """Least-squares growth exponent: time = c*n^k => k = log-log slope."""
    n = len(times)
    sl = sum(math.log(r) for r, _ in times)
    st = sum(math.log(t) for _, t in times)
    sl2 = sum(math.log(r) ** 2 for r, _ in times)
    slt = sum(math.log(r) * math.log(t) for r, t in times)
    return (n * slt - sl * st) / (n * sl2 - sl * sl)


def parse_dat(path):
    series = {"BEFORE": [], "AFTER_RELEASE": [], "AFTER_DEBUG": []}
    logged = {}
    tol = {}
    ceil = {}
    paper = None
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if parts[0] in series:
            series[parts[0]].append((int(parts[1]), float(parts[2])))
        elif parts[0] == "LOGGED_K":
            logged[parts[1]] = float(parts[2])
        elif parts[0] == "LOGGED_TOL":
            tol[parts[1]] = float(parts[2])
        elif parts[0] == "CEIL":
            ceil[parts[1]] = float(parts[2])
        elif parts[0] == "PAPER_K":
            paper = float(parts[1])
        else:
            raise ValueError(f"bad line: {line}")
    return series, logged, tol, ceil, paper


# 5x7 bitmap font (uppercase + digits + " .,;:=->()/+!"), '#' = ink.
FONT = {
    'A': (".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"),
    'B': ("####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."),
    'C': (".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."),
    'D': ("####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."),
    'E': ("#####", "#....", "#....", "####.", "#....", "#....", "#####"),
    'F': ("#####", "#....", "#....", "####.", "#....", "#....", "#...."),
    'G': (".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".###."),
    'H': ("#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"),
    'I': ("#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"),
    'J': ("..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."),
    'K': ("#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"),
    'L': ("#....", "#....", "#....", "#....", "#....", "#....", "#####"),
    'M': ("#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"),
    'N': ("#...#", "##..#", "##..#", "#.#.#", "#..##", "#..##", "#...#"),
    'O': (".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."),
    'P': ("####.", "#...#", "#...#", "####.", "#....", "#....", "#...."),
    'Q': (".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"),
    'R': ("####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"),
    'S': (".####", "#....", "#....", ".###.", "....#", "....#", "####."),
    'T': ("#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."),
    'U': ("#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."),
    'V': ("#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."),
    'W': ("#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"),
    'X': ("#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"),
    'Y': ("#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."),
    'Z': ("#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"),
    '0': (".###.", "#..##", "#.#.#", "#.#.#", "##..#", "#...#", ".###."),
    '1': ("..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."),
    '2': (".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"),
    '3': (".###.", "#...#", "....#", "..##.", "....#", "#...#", ".###."),
    '4': ("...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."),
    '5': ("#####", "#....", "####.", "....#", "....#", "#...#", ".###."),
    '6': (".###.", "#....", "#....", "####.", "#...#", "#...#", ".###."),
    '7': ("#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."),
    '8': (".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."),
    '9': (".###.", "#...#", "#...#", ".####", "....#", "....#", ".###."),
    ' ': (".....", ".....", ".....", ".....", ".....", ".....", "....."),
    '.': (".....", ".....", ".....", ".....", ".....", ".##..", ".##.."),
    ',': (".....", ".....", ".....", ".....", ".##..", ".##..", ".#..."),
    ';': (".....", ".##..", ".##..", ".....", ".##..", ".##..", ".#..."),
    ':': (".....", ".##..", ".##..", ".....", ".##..", ".##..", "....."),
    '=': (".....", ".....", "#####", ".....", "#####", ".....", "....."),
    '-': (".....", ".....", ".....", "#####", ".....", ".....", "....."),
    '>': ("#....", ".#...", "..#..", "...#.", "..#..", ".#...", "#...."),
    '(': ("...#.", "..#..", ".#...", ".#...", ".#...", "..#..", "...#."),
    ')': (".#...", "..#..", "...#.", "...#.", "...#.", "..#..", ".#..."),
    '/': ("....#", "....#", "...#.", "..#..", ".#...", "#....", "#...."),
    '+': (".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."),
    '!': ("..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#.."),
}

W, H = 1280, 800
WHITE = (255, 255, 255)
BLACK = (26, 26, 26)
GREY = (216, 216, 216)
DGREY = (120, 120, 120)
RED = (192, 57, 43)
GREEN = (47, 125, 50)
BLUE = (74, 111, 165)
ORANGE = (217, 123, 0)


class Canvas:
    def __init__(self, w, h, bg):
        self.w, self.h = w, h
        self.px = bytearray(bg * (w * h))

    def rect(self, x0, y0, x1, y1, rgb):
        for y in range(max(0, y0), min(self.h, y1)):
            row = y * self.w * 3
            for x in range(max(0, x0), min(self.w, x1)):
                o = row + x * 3
                self.px[o:o + 3] = bytes(rgb)

    def hline(self, x0, x1, y, rgb, dash=0):
        for x in range(x0, x1):
            if dash and (x // dash) % 2:
                continue
            if 0 <= x < self.w and 0 <= y < self.h:
                o = (y * self.w + x) * 3
                self.px[o:o + 3] = bytes(rgb)

    def vline(self, x, y0, y1, rgb):
        for y in range(y0, y1):
            if 0 <= x < self.w and 0 <= y < self.h:
                o = (y * self.w + x) * 3
                self.px[o:o + 3] = bytes(rgb)

    def text(self, x, y, s, rgb, scale=2):
        cx = x
        for ch in s.upper():
            glyph = FONT.get(ch)
            if glyph is None:
                raise ValueError(f"font lacks {ch!r} (keep labels ASCII/upper)")
            for gy, row in enumerate(glyph):
                for gx, bit in enumerate(row):
                    if bit == '#':
                        self.rect(cx + gx * scale, y + gy * scale,
                                  cx + gx * scale + scale,
                                  y + gy * scale + scale, rgb)
            cx += 6 * scale
        return cx

    def text_center(self, x0, x1, y, s, rgb, scale=2):
        w = len(s) * 6 * scale - scale
        self.text((x0 + x1 - w) // 2, y, s, rgb, scale)

    def png_bytes(self):
        raw = bytearray()
        for y in range(self.h):
            raw.append(0)
            raw += self.px[y * self.w * 3:(y + 1) * self.w * 3]
        def chunk(tag, data):
            return (struct.pack(">I", len(data)) + tag + data +
                    struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))
        return (b"\x89PNG\r\n\x1a\n" +
                chunk(b"IHDR", struct.pack(">IIBBBBB", self.w, self.h,
                                           8, 2, 0, 0, 0)) +
                chunk(b"IDAT", zlib.compress(bytes(raw), 9)) +
                chunk(b"IEND", b""))


def main():
    series, logged, tol, ceil, paper = parse_dat(DAT)
    k = {name: growth_exponent(pts) for name, pts in series.items()}

    # Verification: fits must reproduce the logged k values (per-series
    # tolerance from the dat file, default TOL), and the paper's value
    # must be round(logged_release, 2). Any mismatch aborts before any
    # PNG is written. Every numeric expectation comes from the dat file.
    for name, want in logged.items():
        got = k[name]
        t = tol.get(name, TOL)
        print(f"{name}: fit k={got:.4f} logged k={want:.4f} "
              f"tol={t} diff={abs(got - want):.4f}")
        assert abs(got - want) <= t, \
            f"{name}: fit {got:.4f} != logged {want:.4f}"
    assert round(logged["AFTER_RELEASE"], 2) == paper, \
        f"paper {paper} != round(logged {logged['AFTER_RELEASE']})"
    print(f"paper {paper} == round({logged['AFTER_RELEASE']}, 2): "
          f"NO MISMATCH")

    c = Canvas(W, H, WHITE)
    c.text_center(0, W, 28, "CX1 GROWTH EXPONENT: BEFORE -> AFTER",
                  BLACK, scale=4)
    c.text_center(0, W, 92,
                  f"FIT K: BEFORE {k['BEFORE']:.2f} -> RELEASE "
                  f"{k['AFTER_RELEASE']:.2f} (CEIL {ceil['RELEASE']})",
                  DGREY, scale=2)

    # Plot geometry.
    L, R, T, B = 110, 1210, 150, 590
    KMAX = 4.2
    def y_of(kv):
        return int(B - (B - T) * kv / KMAX)
    c.rect(L, T, R, B, WHITE)
    for tick in (0, 1, 2, 3, 4):
        y = y_of(tick)
        c.hline(L, R, y, GREY)
        c.text(72, y - 7, str(tick), BLACK, scale=2)
    c.vline(L, T, B + 1, BLACK)
    c.hline(L, R, B, BLACK)
    c.text(16, (T + B) // 2 - 40, "K", BLACK, scale=2)

    # Ceiling lines (CX1 gates from the nlvs bead). The 1.4/1.5 lines
    # sit ~10 px apart, so their tags go on opposite sides of the lines.
    for key, color, dy in (("RELEASE", RED, 6), ("DEBUG", ORANGE, -22)):
        y = y_of(ceil[key])
        c.hline(L + 2, R, y, color, dash=10)
        tag = f"{'REL' if key == 'RELEASE' else 'DBG'} CEIL {ceil[key]}"
        c.text(R - 24 * 11, y + dy, tag, color, scale=2)

    # Bars: (label, sublabel, k, color).
    bars = (("BEFORE", "(BRIDGE)", k["BEFORE"], RED),
            ("AFTER", "(CX1 REL)", k["AFTER_RELEASE"], GREEN),
            ("AFTER", "(CX1 DBG)", k["AFTER_DEBUG"], BLUE))
    slot = (R - L) // 3
    for i, (lab, sub, kv, color) in enumerate(bars):
        x0 = L + i * slot + 78
        x1 = L + (i + 1) * slot - 78
        y = y_of(kv)
        c.rect(x0, y, x1, B, color)
        c.rect(x0, y, x1, y + 3, BLACK)
        # Value in white inside the bar base: short bars sit under the
        # ceiling dashes, so a top label would collide with them.
        val = f"K={kv:.2f}"
        c.text_center(x0, x1, B - 36, val, WHITE, scale=2)
        c.text_center(x0 - 40, x1 + 40, B + 12, lab, BLACK, scale=2)
        c.text_center(x0 - 40, x1 + 40, B + 38, sub, DGREY, scale=2)

    # Footer: logged timing points + paper check.
    b0, b1 = series["BEFORE"][0], series["BEFORE"][-1]
    a0, a1 = series["AFTER_RELEASE"][0], series["AFTER_RELEASE"][-1]
    foot = [
        f"BEFORE: BRIDGE {b0[0]}->{b1[0]} TOKENS, "
        f"{b0[1]}S->{b1[1]}S (A6 BLOCKED TABLE).",
        f"AFTER: CX1 {a0[0] // 1000}K->{a1[0] // 1000}K TOKENS, "
        f"REL {a0[1]}S->{a1[1]}S K={logged['AFTER_RELEASE']:.4f} "
        f"LOGGED (NLVS).",
        f"PAPER REPORTS {paper:.2f} = ROUND({logged['AFTER_RELEASE']:.4f}). "
        f"NO MISMATCH.",
    ]
    y = 668
    for line in foot:
        c.text_center(0, W, y, line, DGREY, scale=2)
        y += 30

    OUT.write_bytes(c.png_bytes())
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

