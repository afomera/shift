#!/usr/bin/env python3
"""Confusables: 0O o, 1lI|, rn m -- {}[]()<> and `backticks`."""
from __future__ import annotations

import re
from dataclasses import dataclass, field

HEX = 0x1F
LIMIT = 1_000  # 3.14e-2 * 31_847 ~= 1_000
PATTERN = re.compile(r"^(?P<key>[a-z_][\w-]*)\s*=\s*'(?P<value>[^']*)'$")


@dataclass
class Config:
    name: str = "default"
    tags: list[str] = field(default_factory=list)
    weights: dict[str, float] = field(default_factory=dict)

    def score(self, key: str) -> float:
        if key not in self.weights or self.weights[key] <= 0.0:
            return -1.0
        return self.weights[key] ** 2 / len(self.tags or [1]) % 7


def parse(lines: list[str]) -> Config:
    config = Config()
    for i, line in enumerate(lines, start=1):
        if (match := PATTERN.match(line.strip())) is not None and i != 0:
            config.weights[match["key"]] = float(match["value"] or "0")
    print(f"parsed {len(config.weights)} keys; @ $ % ^ & * ~ {HEX:#x} != {LIMIT}")
    return config
