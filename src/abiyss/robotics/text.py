"""Single-stroke font and text layout for ``write(text)``.

Glyphs are polylines in em units: x in [0, 1] (scaled by the letter width),
y in [0, 1] from baseline to cap height. Each glyph is a list of strokes;
the pen is lifted between strokes. The font is intentionally simple: the
goal is to expose the mechanics of the arm, not typography.
"""

from __future__ import annotations

import math
from typing import Sequence

from ..errors import ValidationError

Stroke = list[tuple[float, float]]


def _arc(cx: float, cy: float, rx: float, ry: float, a0: float, a1: float, n: int = 16) -> Stroke:
    return [(cx + rx * math.cos(math.radians(a0 + (a1 - a0) * i / n)), cy + ry * math.sin(math.radians(a0 + (a1 - a0) * i / n))) for i in range(n + 1)]


# The "O" starts at the top and runs counter-clockwise (as seen with +y up),
# closing on itself: backlash and dead band show up as a gap/overlap.
GLYPHS: dict[str, list[Stroke]] = {
    "O": [_arc(0.5, 0.5, 0.5, 0.5, 90, 450, 36)],
    "I": [[(0.5, 1.0), (0.5, 0.0)]],
    "A": [[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)], [(0.2, 0.4), (0.8, 0.4)]],
    "B": [[(0.0, 0.0), (0.0, 1.0), (0.65, 1.0)] + _arc(0.65, 0.75, 0.3, 0.25, 90, -90, 8)[1:] + [(0.0, 0.5), (0.7, 0.5)] + _arc(0.7, 0.25, 0.3, 0.25, 90, -90, 8)[1:] + [(0.0, 0.0)]],
    "C": [_arc(0.55, 0.5, 0.5, 0.5, 45, 315, 24)],
    "D": [[(0.0, 0.0), (0.0, 1.0), (0.5, 1.0)] + _arc(0.5, 0.5, 0.5, 0.5, 90, -90, 16)[1:] + [(0.0, 0.0)]],
    "E": [[(1.0, 1.0), (0.0, 1.0), (0.0, 0.0), (1.0, 0.0)], [(0.0, 0.5), (0.7, 0.5)]],
    "F": [[(1.0, 1.0), (0.0, 1.0), (0.0, 0.0)], [(0.0, 0.5), (0.7, 0.5)]],
    "G": [_arc(0.55, 0.5, 0.5, 0.5, 45, 340, 24) + [(1.0, 0.45), (0.6, 0.45)]],
    "H": [[(0.0, 1.0), (0.0, 0.0)], [(1.0, 1.0), (1.0, 0.0)], [(0.0, 0.5), (1.0, 0.5)]],
    "J": [[(1.0, 1.0)] + _arc(0.5, 0.3, 0.5, 0.3, 0, -180, 12)],
    "K": [[(0.0, 1.0), (0.0, 0.0)], [(1.0, 1.0), (0.0, 0.4)], [(0.3, 0.6), (1.0, 0.0)]],
    "L": [[(0.0, 1.0), (0.0, 0.0), (1.0, 0.0)]],
    "M": [[(0.0, 0.0), (0.0, 1.0), (0.5, 0.4), (1.0, 1.0), (1.0, 0.0)]],
    "N": [[(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]],
    "P": [[(0.0, 0.0), (0.0, 1.0), (0.7, 1.0)] + _arc(0.7, 0.75, 0.3, 0.25, 90, -90, 8)[1:] + [(0.0, 0.5)]],
    "Q": [_arc(0.5, 0.5, 0.5, 0.5, 90, 450, 32), [(0.6, 0.3), (1.0, -0.05)]],
    "R": [[(0.0, 0.0), (0.0, 1.0), (0.7, 1.0)] + _arc(0.7, 0.75, 0.3, 0.25, 90, -90, 8)[1:] + [(0.0, 0.5), (1.0, 0.0)]],
    "S": [_arc(0.5, 0.75, 0.45, 0.25, 20, 270, 12) + _arc(0.5, 0.25, 0.45, 0.25, 90, -160, 12)[1:]],
    "T": [[(0.0, 1.0), (1.0, 1.0)], [(0.5, 1.0), (0.5, 0.0)]],
    "U": [[(0.0, 1.0)] + _arc(0.5, 0.35, 0.5, 0.35, 180, 360, 14) + [(1.0, 1.0)]],
    "V": [[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)]],
    "W": [[(0.0, 1.0), (0.25, 0.0), (0.5, 0.6), (0.75, 0.0), (1.0, 1.0)]],
    "X": [[(0.0, 1.0), (1.0, 0.0)], [(1.0, 1.0), (0.0, 0.0)]],
    "Y": [[(0.0, 1.0), (0.5, 0.5), (1.0, 1.0)], [(0.5, 0.5), (0.5, 0.0)]],
    "Z": [[(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)]],
    "0": [_arc(0.5, 0.5, 0.5, 0.5, 90, 450, 32), [(0.85, 0.85), (0.15, 0.15)]],
    "1": [[(0.25, 0.75), (0.55, 1.0), (0.55, 0.0)]],
    "2": [_arc(0.5, 0.72, 0.45, 0.28, 160, -30, 10) + [(0.0, 0.0), (1.0, 0.0)]],
    "3": [_arc(0.5, 0.75, 0.42, 0.25, 160, -90, 10) + _arc(0.5, 0.25, 0.48, 0.25, 90, -160, 10)[1:]],
    "-": [[(0.15, 0.5), (0.85, 0.5)]],
    ".": [[(0.5, 0.0), (0.5, 0.04)]],
    "!": [[(0.5, 1.0), (0.5, 0.3)], [(0.5, 0.02), (0.5, 0.06)]],
    " ": [],
}


def supported(text: str) -> bool:
    return all(ch in GLYPHS for ch in text.upper())


def layout(
    text: str,
    *,
    origin: Sequence[float],
    letter_height: float,
    letter_width: float,
    spacing: float,
    up_axis: Sequence[float] = (1.0, 0.0),
    advance_axis: Sequence[float] = (0.0, -1.0),
) -> list[list[tuple[float, float]]]:
    """Strokes of ``text`` in robot XY coordinates (metres).

    ``origin`` is the baseline-left corner of the first letter; ``up_axis``
    points from baseline to cap height, ``advance_axis`` along the line.
    """
    text = text.upper()
    unknown = sorted({ch for ch in text if ch not in GLYPHS})
    if unknown:
        raise ValidationError(f"unsupported characters for write(): {''.join(unknown)!r}")
    if len(text) > 32:
        raise ValidationError("write() text limited to 32 characters")
    ox, oy = float(origin[0]), float(origin[1])
    ux, uy = float(up_axis[0]), float(up_axis[1])
    ax, ay = float(advance_axis[0]), float(advance_axis[1])
    strokes: list[list[tuple[float, float]]] = []
    for index, ch in enumerate(text):
        x0 = index * (letter_width + spacing)
        for stroke in GLYPHS[ch]:
            pts = []
            for (gx, gy) in stroke:
                along = x0 + gx * letter_width
                up = gy * letter_height
                pts.append((ox + along * ax + up * ux, oy + along * ay + up * uy))
            strokes.append(pts)
    return strokes
