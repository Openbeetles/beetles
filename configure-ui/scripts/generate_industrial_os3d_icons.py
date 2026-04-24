#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import math
import sys
from collections import Counter
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from industrial_os3d.icon_groups import GROUP_MODULE_NAMES, iter_group_modules
from industrial_os3d.icon_groups.common import (
    ICON_OUTPUT_SIZE,
    PUBLIC_ICON_DIR,
    IconSpec,
    render_icon,
)


def collect_expected_icons(selected_groups: list[str]) -> list[str]:
    if selected_groups:
        expected: list[str] = []
        for module in iter_group_modules(selected_groups):
            expected.extend(module.OWNED_ICONS)
        return sorted(expected)
    return sorted(path.name for path in PUBLIC_ICON_DIR.glob("*_3d.png"))


def collect_specs(selected_groups: list[str]) -> list[tuple[str, IconSpec]]:
    specs: list[tuple[str, IconSpec]] = []
    for module in iter_group_modules(selected_groups):
        for spec in module.ICON_DEFINITIONS:
            specs.append((module.ICON_GROUP, spec))
    return specs


def validate_specs(grouped_specs: list[tuple[str, IconSpec]], selected_groups: list[str]) -> list[str]:
    errors: list[str] = []
    expected = collect_expected_icons(selected_groups)
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


def icon_metrics(image: Image.Image, name: str) -> dict[str, str | int | float]:
    alpha = image.getchannel("A")
    strong = alpha.point(lambda value: 255 if value >= 96 else 0)
    bbox = strong.getbbox() or alpha.getbbox()
    if bbox is None:
        return {
            "name": name,
            "width": 0,
            "height": 0,
            "coverage": 0.0,
            "aspect": 0.0,
            "center_x": 0.0,
            "center_y": 0.0,
        }

    x1, y1, x2, y2 = bbox
    width = x2 - x1
    height = y2 - y1
    coverage = (width * height) / (image.width * image.height)
    return {
        "name": name,
        "width": width,
        "height": height,
        "coverage": round(coverage, 4),
        "aspect": round(width / height, 4) if height else 0.0,
        "center_x": round(((x1 + x2) / 2) / image.width, 4),
        "center_y": round(((y1 + y2) / 2) / image.height, 4),
    }


def load_audit_font(size: int) -> ImageFont.ImageFont:
    candidates = (
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
    )
    for candidate in candidates:
        path = Path(candidate)
        if path.exists():
            return ImageFont.truetype(str(path), size)
    return ImageFont.load_default()


def short_label(name: str, max_chars: int) -> str:
    label = name.removesuffix("_3d.png")
    if len(label) <= max_chars:
        return label
    return f"{label[: max_chars - 1]}."


def write_contact_sheet(
    rendered: list[tuple[str, Image.Image]],
    output: Path,
    *,
    columns: int,
    cell: tuple[int, int],
    tile: tuple[int, int],
    icon_size: int,
    label_chars: int,
) -> None:
    cell_width, cell_height = cell
    rows = math.ceil(len(rendered) / columns)
    sheet = Image.new("RGB", (columns * cell_width, rows * cell_height), (244, 247, 251))
    draw = ImageDraw.Draw(sheet)
    font = load_audit_font(10)

    for index, (name, image) in enumerate(rendered):
        row = index // columns
        col = index % columns
        x = col * cell_width
        y = row * cell_height
        tile_width, tile_height = tile
        tile_x1 = x + (cell_width - tile_width) // 2
        tile_y1 = y + 8
        draw.rounded_rectangle(
            (tile_x1, tile_y1, tile_x1 + tile_width, tile_y1 + tile_height),
            radius=max(14, tile_width // 6),
            fill=(255, 255, 255),
            outline=(224, 232, 242),
            width=1,
        )
        icon = image.resize((icon_size, icon_size), Image.Resampling.LANCZOS)
        sheet.paste(icon, (x + (cell_width - icon_size) // 2, tile_y1 + (tile_height - icon_size) // 2), icon)
        draw.text(
            (x + 5, tile_y1 + tile_height + 5),
            short_label(name, label_chars),
            fill=(59, 70, 90),
            font=font,
        )

    sheet.save(output)


def write_audit(grouped_specs: list[tuple[str, IconSpec]], audit_dir: Path) -> None:
    audit_dir.mkdir(parents=True, exist_ok=True)
    rendered = [(spec.name, render_icon(spec)) for _, spec in grouped_specs]

    metrics = [icon_metrics(image, name) for name, image in rendered]
    with (audit_dir / "metrics.tsv").open("w", newline="") as file:
        writer = csv.DictWriter(
            file,
            fieldnames=("name", "width", "height", "coverage", "aspect", "center_x", "center_y"),
            delimiter="\t",
        )
        writer.writeheader()
        writer.writerows(metrics)

    write_contact_sheet(
        rendered,
        audit_dir / "overview.png",
        columns=8,
        cell=(178, 180),
        tile=(150, 150),
        icon_size=112,
        label_chars=20,
    )
    write_contact_sheet(
        rendered,
        audit_dir / "dock.png",
        columns=12,
        cell=(92, 104),
        tile=(62, 62),
        icon_size=44,
        label_chars=11,
    )
    print(f"wrote audit outputs to {audit_dir}")


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
    parser.add_argument("--audit-dir", type=Path)
    args = parser.parse_args()

    grouped_specs = collect_specs(args.group)
    spec_errors = validate_specs(grouped_specs, args.group)

    if args.list:
        list_specs(grouped_specs)

    if args.check:
        spec_errors.extend(check_outputs(grouped_specs))

    if spec_errors:
        for error in spec_errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    if args.audit_dir is not None:
        write_audit(grouped_specs, args.audit_dir)

    if not args.list and not args.check and args.audit_dir is None:
        render_and_write(grouped_specs)

    if args.check:
        print(f"ok: validated {len(grouped_specs)} icon definitions")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
