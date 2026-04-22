#!/usr/bin/env python3
"""Expand local Cargo features from Cargo.toml in declaration order."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11 fallback
    tomllib = None


def load_features_from_toml(manifest_path: str) -> dict[str, list[str]] | None:
    if tomllib is None:
        return None
    with open(manifest_path, "rb") as handle:
        data = tomllib.load(handle)
    features = data.get("features")
    if not isinstance(features, dict):
        raise SystemExit(f"Error: missing [features] table in {manifest_path}")
    return {
        str(name): [str(entry) for entry in entries]
        for name, entries in features.items()
    }


def load_features_from_metadata(manifest_path: str) -> dict[str, list[str]]:
    metadata_cmd = ["cargo", "metadata", "--no-deps", "--format-version", "1"]
    result = subprocess.run(
        metadata_cmd,
        cwd=os.path.dirname(manifest_path),
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(
            "Error: cargo metadata failed while resolving Cargo features:\n"
            f"{result.stderr.strip()}"
        )
    metadata = json.loads(result.stdout)
    manifest_realpath = os.path.realpath(manifest_path)
    for package in metadata.get("packages", []):
        if os.path.realpath(package.get("manifest_path", "")) == manifest_realpath:
            features = package.get("features")
            if not isinstance(features, dict):
                break
            return {
                str(name): [str(entry) for entry in entries]
                for name, entries in features.items()
            }
    raise SystemExit(f"Error: package for manifest not found in cargo metadata: {manifest_path}")


def load_feature_graph(manifest_path: str) -> dict[str, list[str]]:
    return load_features_from_toml(manifest_path) or load_features_from_metadata(manifest_path)


def local_feature_refs(entries: list[str], feature_graph: dict[str, list[str]]) -> list[str]:
    refs: list[str] = []
    for entry in entries:
        if entry in feature_graph:
            refs.append(entry)
    return refs


def expand_feature_roots(
    feature_graph: dict[str, list[str]], roots: list[str]
) -> list[str]:
    ordered: list[str] = []
    seen: set[str] = set()

    def visit(feature_name: str) -> None:
        if feature_name in seen:
            return
        if feature_name not in feature_graph:
            raise SystemExit(f"Error: Cargo feature not found: {feature_name}")
        seen.add(feature_name)
        ordered.append(feature_name)
        for child in local_feature_refs(feature_graph.get(feature_name, []), feature_graph):
            visit(child)

    for root in roots:
        visit(root)
    return ordered


def parse_roots(roots_csv: str) -> list[str]:
    roots = [item.strip() for item in roots_csv.split(",") if item.strip()]
    if not roots:
        raise SystemExit("Error: --roots requires at least one Cargo feature")
    return roots


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Expand local Cargo feature roots from Cargo.toml."
    )
    parser.add_argument("--manifest", required=True, help="Path to Cargo.toml")
    parser.add_argument(
        "--roots",
        required=True,
        help="Comma-separated Cargo feature roots to expand",
    )
    parser.add_argument(
        "--format",
        choices=("csv", "shell-args"),
        default="csv",
        help="Output format",
    )
    args = parser.parse_args()

    feature_graph = load_feature_graph(os.path.realpath(args.manifest))
    expanded = expand_feature_roots(feature_graph, parse_roots(args.roots))
    csv = ",".join(expanded)
    if args.format == "shell-args":
        print(f"--no-default-features --features {csv}")
    else:
        print(csv)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
