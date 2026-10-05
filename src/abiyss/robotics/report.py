"""Dependency-free SVG plots for validation reports.

Each function returns an SVG document (str). Colours are fixed (light
background) so the files read the same on GitHub and in the lab report.
"""

from __future__ import annotations

import html
import math
from typing import Iterable, Sequence

INK = "#c0392b"
IDEAL = "#9aa4ae"
AXIS = "#56606a"
GRID = "#e3e6e9"
TEXT = "#22282e"
SERIES = ["#1f6fb2", "#d9822b", "#2e8b57", "#7a3fb0", "#b3261e", "#5b6770"]
FONT = "font-family='DejaVu Sans, Arial, sans-serif'"


def _svg(width: int, height: int, body: str, title: str) -> str:
    return (
        f"<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}' viewBox='0 0 {width} {height}'>"
        f"<rect width='100%' height='100%' fill='#ffffff'/>"
        f"<text x='16' y='24' {FONT} font-size='15' font-weight='600' fill='{TEXT}'>{html.escape(title)}</text>"
        f"{body}</svg>"
    )


class _Scale:
    def __init__(self, lo: float, hi: float, a: float, b: float) -> None:
        if hi - lo < 1e-12:
            hi = lo + 1.0
        self.lo, self.hi, self.a, self.b = lo, hi, a, b

    def __call__(self, v: float) -> float:
        return self.a + (v - self.lo) / (self.hi - self.lo) * (self.b - self.a)


def _ticks(lo: float, hi: float, n: int = 6) -> list[float]:
    span = hi - lo
    if span <= 0:
        return [lo]
    raw = span / n
    mag = 10 ** math.floor(math.log10(raw))
    step = min((m * mag for m in (1, 2, 5, 10) if m * mag >= raw), default=raw)
    start = math.ceil(lo / step) * step
    out = []
    v = start
    while v <= hi + 1e-12:
        out.append(round(v, 10))
        v += step
    return out


def _axes(sx: _Scale, sy: _Scale, xlabel: str, ylabel: str, fmt: str = "{:g}") -> str:
    parts = []
    for t in _ticks(sx.lo, sx.hi):
        x = sx(t)
        parts.append(f"<line x1='{x:.1f}' y1='{sy.a:.1f}' x2='{x:.1f}' y2='{sy.b:.1f}' stroke='{GRID}'/>")
        parts.append(f"<text x='{x:.1f}' y='{sy.a + 16:.1f}' {FONT} font-size='11' fill='{AXIS}' text-anchor='middle'>{fmt.format(t)}</text>")
    for t in _ticks(sy.lo, sy.hi):
        y = sy(t)
        parts.append(f"<line x1='{sx.a:.1f}' y1='{y:.1f}' x2='{sx.b:.1f}' y2='{y:.1f}' stroke='{GRID}'/>")
        parts.append(f"<text x='{sx.a - 6:.1f}' y='{y + 4:.1f}' {FONT} font-size='11' fill='{AXIS}' text-anchor='end'>{fmt.format(t)}</text>")
    parts.append(f"<rect x='{sx.a:.1f}' y='{sy.b:.1f}' width='{sx.b - sx.a:.1f}' height='{sy.a - sy.b:.1f}' fill='none' stroke='{AXIS}'/>")
    parts.append(f"<text x='{(sx.a + sx.b) / 2:.1f}' y='{sy.a + 34:.1f}' {FONT} font-size='12' fill='{TEXT}' text-anchor='middle'>{html.escape(xlabel)}</text>")
    parts.append(f"<text x='14' y='{(sy.a + sy.b) / 2:.1f}' {FONT} font-size='12' fill='{TEXT}' text-anchor='middle' transform='rotate(-90 14 {(sy.a + sy.b) / 2:.1f})'>{html.escape(ylabel)}</text>")
    return "".join(parts)


def _poly(points: Iterable[tuple[float, float]], color: str, width: float = 1.6, dash: str | None = None) -> str:
    pts = " ".join(f"{x:.2f},{y:.2f}" for x, y in points)
    d = f" stroke-dasharray='{dash}'" if dash else ""
    return f"<polyline points='{pts}' fill='none' stroke='{color}' stroke-width='{width}' stroke-linejoin='round' stroke-linecap='round'{d}/>"


def _legend(items: Sequence[tuple[str, str]], x: float, y: float) -> str:
    out = []
    for i, (label, color) in enumerate(items):
        yy = y + i * 18
        out.append(f"<line x1='{x}' y1='{yy}' x2='{x + 22}' y2='{yy}' stroke='{color}' stroke-width='3'/>")
        out.append(f"<text x='{x + 28}' y='{yy + 4}' {FONT} font-size='12' fill='{TEXT}'>{html.escape(label)}</text>")
    return "".join(out)


def ink_plot(ideal: list[list[Sequence[float]]], ink: list[list[Sequence[float]]], title: str, *, up_axis=(1.0, 0.0), advance_axis=(0.0, -1.0), notes: Sequence[str] = ()) -> str:
    """Ink vs ideal strokes, drawn in the writer's frame (text upright)."""
    def to_page(p: Sequence[float]) -> tuple[float, float]:
        along = p[0] * advance_axis[0] + p[1] * advance_axis[1]
        up = p[0] * up_axis[0] + p[1] * up_axis[1]
        return along * 1000.0, up * 1000.0
    pts = [to_page(p) for s in [*ideal, *ink] for p in s]
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    pad = 4.0
    xlo, xhi, ylo, yhi = min(xs) - pad, max(xs) + pad, min(ys) - pad, max(ys) + pad
    span = max(xhi - xlo, yhi - ylo)
    xhi, yhi = xlo + span, ylo + span
    W, H = 640, 640
    sx = _Scale(xlo, xhi, 70, W - 30)
    sy = _Scale(ylo, yhi, H - 60, 50)
    body = _axes(sx, sy, "along the line (mm)", "letter height (mm)")
    for s in ideal:
        body += _poly([(sx(a), sy(b)) for a, b in map(to_page, s)], IDEAL, 3.0, "6 4")
    for s in ink:
        body += _poly([(sx(a), sy(b)) for a, b in map(to_page, s)], INK, 2.0)
    body += _legend([("ideal stroke", IDEAL), ("ink (physics ground truth)", INK)], W - 230, 70)
    for i, n in enumerate(notes):
        body += f"<text x='80' y='{H - 18 + i * 0:.0f}' {FONT} font-size='11' fill='{AXIS}'>{html.escape(n)}</text>" if i == 0 else ""
    if len(notes) > 1:
        for i, n in enumerate(notes[1:]):
            body += f"<text x='80' y='{70 + i * 16}' {FONT} font-size='11' fill='{AXIS}'>{html.escape(n)}</text>"
    return _svg(W, H, body, title)


def line_plot(series: Sequence[tuple[str, Sequence[tuple[float, float]]]], title: str, xlabel: str, ylabel: str, *, hlines: Sequence[tuple[str, float]] = (), width: int = 760, height: int = 420) -> str:
    pts = [p for _, s in series for p in s]
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts] + [v for _, v in hlines]
    ypad = (max(ys) - min(ys)) * 0.08 or 1.0
    sx = _Scale(min(xs), max(xs), 70, width - 20)
    sy = _Scale(min(ys) - ypad, max(ys) + ypad, height - 60, 44)
    body = _axes(sx, sy, xlabel, ylabel)
    for label, v in hlines:
        y = sy(v)
        body += f"<line x1='{sx.a}' y1='{y:.1f}' x2='{sx.b}' y2='{y:.1f}' stroke='#b3261e' stroke-dasharray='5 4'/>"
        body += f"<text x='{sx.b - 4}' y='{y - 4:.1f}' {FONT} font-size='11' fill='#b3261e' text-anchor='end'>{html.escape(label)}</text>"
    for i, (label, s) in enumerate(series):
        body += _poly([(sx(x), sy(y)) for x, y in s], SERIES[i % len(SERIES)], 1.8)
    body += _legend([(label, SERIES[i % len(SERIES)]) for i, (label, _) in enumerate(series)], 90, 62)
    return _svg(width, height, body, title)


def scatter_plot(points: Sequence[tuple[float, float]], title: str, xlabel: str, ylabel: str, *, center: tuple[float, float] | None = None, circle_r: float | None = None, width: int = 520, height: int = 520) -> str:
    xs = [p[0] for p in points] + ([center[0]] if center else [])
    ys = [p[1] for p in points] + ([center[1]] if center else [])
    span = max(max(xs) - min(xs), max(ys) - min(ys), (circle_r or 0) * 2.2, 0.2)
    cx, cy = (max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2
    sx = _Scale(cx - span * 0.6, cx + span * 0.6, 70, width - 20)
    sy = _Scale(cy - span * 0.6, cy + span * 0.6, height - 60, 44)
    body = _axes(sx, sy, xlabel, ylabel, "{:.2f}")
    if center and circle_r:
        r = abs(sx(center[0] + circle_r) - sx(center[0]))
        body += f"<circle cx='{sx(center[0]):.1f}' cy='{sy(center[1]):.1f}' r='{r:.1f}' fill='none' stroke='#d9822b' stroke-dasharray='4 3'/>"
    if center:
        body += f"<path d='M {sx(center[0]) - 7:.1f} {sy(center[1]):.1f} h 14 M {sx(center[0]):.1f} {sy(center[1]) - 7:.1f} v 14' stroke='#22282e' stroke-width='1.5'/>"
    for i, (x, y) in enumerate(points):
        body += f"<circle cx='{sx(x):.1f}' cy='{sy(y):.1f}' r='4' fill='#1f6fb2' fill-opacity='0.75'/>"
        body += f"<text x='{sx(x) + 6:.1f}' y='{sy(y) - 5:.1f}' {FONT} font-size='9' fill='{AXIS}'>{i + 1}</text>"
    return _svg(width, height, body, title)
