"""Trajectory generation (time-parametrised, sampled at the control rate).

Trajectories are *references* for the servos. They never move the arm by
themselves: each sample becomes a pulse width that the (simulated or real)
servo tries to follow through its own dynamics.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Mapping, Sequence

Vec3 = tuple[float, float, float]


def min_jerk(s: float) -> float:
    """Minimum-jerk position profile on [0, 1] (zero vel/acc at both ends)."""
    s = min(1.0, max(0.0, s))
    return s * s * s * (10.0 - 15.0 * s + 6.0 * s * s)


MIN_JERK_PEAK_VELOCITY = 1.875   # peak of d/ds min_jerk(s)


@dataclass(frozen=True, slots=True)
class JointMove:
    start: dict[str, float]
    goal: dict[str, float]
    duration: float

    def at(self, t: float) -> dict[str, float]:
        s = min_jerk(t / self.duration) if self.duration > 0 else 1.0
        return {k: self.start[k] + (self.goal[k] - self.start[k]) * s for k in self.goal}

    def done(self, t: float) -> bool:
        return t >= self.duration


def plan_joint_move(start: Mapping[str, float], goal: Mapping[str, float], max_speed: Mapping[str, float], *, speed_scale: float = 0.35, min_duration: float = 0.15) -> JointMove:
    """Synchronised min-jerk move; duration set by the slowest joint so that
    no joint's peak reference speed exceeds ``speed_scale`` x its servo
    no-load speed (from the datasheet)."""
    if not 0.0 < speed_scale <= 1.0:
        raise ValueError("speed_scale must be in (0, 1]")
    duration = min_duration
    for k, g in goal.items():
        dq = abs(float(g) - float(start[k]))
        v = float(max_speed[k]) * speed_scale
        duration = max(duration, MIN_JERK_PEAK_VELOCITY * dq / v)
    return JointMove({k: float(start[k]) for k in goal}, {k: float(v) for k, v in goal.items()}, duration)


@dataclass(frozen=True, slots=True)
class LineMove:
    start: Vec3
    goal: Vec3
    duration: float

    def at(self, t: float) -> Vec3:
        s = min_jerk(t / self.duration) if self.duration > 0 else 1.0
        return tuple(a + (b - a) * s for a, b in zip(self.start, self.goal))  # type: ignore[return-value]

    def done(self, t: float) -> bool:
        return t >= self.duration


def plan_line(start: Sequence[float], goal: Sequence[float], speed: float, *, min_duration: float = 0.05) -> LineMove:
    """Straight Cartesian segment whose *average* speed is ``speed`` (m/s)."""
    if speed <= 0:
        raise ValueError("speed must be positive")
    length = math.dist(start, goal)
    return LineMove(tuple(map(float, start)), tuple(map(float, goal)), max(min_duration, length / speed))  # type: ignore[arg-type]


@dataclass(frozen=True, slots=True)
class PolylineMove:
    """Constant-speed traversal of a polyline with min-jerk ramps at the ends."""

    points: tuple[Vec3, ...]
    cumulative: tuple[float, ...]
    duration: float

    def at(self, t: float) -> Vec3:
        total = self.cumulative[-1]
        if total <= 0 or self.duration <= 0:
            return self.points[-1]
        s = _ramped(t / self.duration)
        d = s * total
        for i in range(1, len(self.cumulative)):
            if d <= self.cumulative[i] or i == len(self.cumulative) - 1:
                seg = self.cumulative[i] - self.cumulative[i - 1]
                u = 0.0 if seg <= 0 else (d - self.cumulative[i - 1]) / seg
                a, b = self.points[i - 1], self.points[i]
                return (a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u)
        return self.points[-1]

    def done(self, t: float) -> bool:
        return t >= self.duration


def _ramped(s: float, ramp: float = 0.15) -> float:
    """Trapezoid-like progress: smooth ramps over the first/last ``ramp``
    fraction of the time, constant speed in between."""
    s = min(1.0, max(0.0, s))
    v = 1.0 / (1.0 - ramp)       # cruise speed so that total progress = 1
    if s < ramp:
        return v * s * s / (2.0 * ramp)
    if s > 1.0 - ramp:
        r = 1.0 - s
        return 1.0 - v * r * r / (2.0 * ramp)
    return v * (s - ramp / 2.0)


def plan_polyline(points: Sequence[Sequence[float]], speed: float) -> PolylineMove:
    pts = tuple(tuple(map(float, p)) for p in points)
    if len(pts) < 2:
        pts = (pts[0], pts[0])
    cum = [0.0]
    for i in range(1, len(pts)):
        cum.append(cum[-1] + math.dist(pts[i - 1], pts[i]))
    # Cruise speed equals ``speed``; ramps add 1/(1 - ramp) to the duration.
    return PolylineMove(pts, tuple(cum), max(0.05, cum[-1] / speed / (1.0 - 0.15)))  # type: ignore[arg-type]
