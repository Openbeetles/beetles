#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from collections import Counter
from pathlib import Path

from PIL import Image

from industrial_os3d.icon_groups import GROUP_MODULE_NAMES, iter_group_modules
from industrial_os3d.icon_groups.common import (
    ICON_OUTPUT_SIZE,
    PUBLIC_ICON_DIR,
    IconSpec,
    render_icon,
)


def collect_expected_icons() -> list[str]:
    return sorted(path.name for path in PUBLIC_ICON_DIR.glob("*_3d.png"))


def collect_specs(selected_groups: list[str]) -> list[tuple[str, IconSpec]]:
    specs: list[tuple[str, IconSpec]] = []
    for module in iter_group_modules(selected_groups):
        for spec in module.ICON_DEFINITIONS:
            specs.append((module.ICON_GROUP, spec))
    return specs


def validate_specs(grouped_specs: list[tuple[str, IconSpec]]) -> list[str]:
    errors: list[str] = []
    expected = collect_expected_icons()
    actual = [spec.name for _, spec in grouped_specs]
    counts = Counter(actual)

    duplicates = sorted(name for name, count in counts.items() if count > 1)
    if duplicates:
        errors.append(f"duplicate icon definitions: {', '.join(duplicates)}")

    missing = sorted(set(expected) - set(actual))
    unexpected = sorted(set(actual) - set(expected))
    if missing:
        errors.append(f"missing icon definitions: {', '.join(missing)}")
    if unexpected:
        errors.append(f"unexpected icon definitions: {', '.join(unexpected)}")

    if len(actual) != len(expected):
        errors.append(f"icon count mismatch: expected {len(expected)}, got {len(actual)}")

    return errors


def render_and_write(grouped_specs: list[tuple[str, IconSpec]]) -> None:
    for _, spec in grouped_specs:
        image = render_icon(spec)
        output = PUBLIC_ICON_DIR / spec.name
        image.save(output, optimize=True)
        print(f"wrote {output}")


def check_outputs(grouped_specs: list[tuple[str, IconSpec]]) -> list[str]:
    errors: list[str] = []
    expected_size = (ICON_OUTPUT_SIZE, ICON_OUTPUT_SIZE)
    for _, spec in grouped_specs:
        image = render_icon(spec)
        if image.size != expected_size:
            errors.append(f"{spec.name}: rendered size {image.size}, expected {expected_size}")
        output = PUBLIC_ICON_DIR / spec.name
        if output.exists():
            with Image.open(output) as existing:
                if existing.size != expected_size:
                    errors.append(f"{spec.name}: output file size {existing.size}, expected {expected_size}")
    return errors


def list_specs(grouped_specs: list[tuple[str, IconSpec]]) -> None:
    by_group: dict[str, list[str]] = {}
    for group, spec in grouped_specs:
        by_group.setdefault(group, []).append(spec.name)

    total = 0
    for group in GROUP_MODULE_NAMES:
        if group not in by_group:
            continue
        names = sorted(by_group[group])
        total += len(names)
        print(f"[{group}] {len(names)}")
        for name in names:
            print(f"  - {name}")
    print(f"total={total}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate configure-ui Industrial OS3D icons.")
    parser.add_argument("--group", action="append", default=[], choices=GROUP_MODULE_NAMES)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    grouped_specs = collect_specs(args.group)
    spec_errors = validate_specs(grouped_specs)

    if args.list:
        list_specs(grouped_specs)

    if args.check:
        spec_errors.extend(check_outputs(grouped_specs))

    if spec_errors:
        for error in spec_errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    if not args.list and not args.check:
        render_and_write(grouped_specs)

    if args.check:
        print(f"ok: validated {len(grouped_specs)} icon definitions")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
