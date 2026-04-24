#!/usr/bin/env python3
"""Resolve Beetle Cargo feature closures and package-profile metadata."""

from __future__ import annotations

import argparse
import json
import os
import subprocess

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11 fallback
    tomllib = None


def load_package_data_from_toml(manifest_path: str) -> dict[str, object] | None:
    if tomllib is None:
        return None
    with open(manifest_path, "rb") as handle:
        return tomllib.load(handle)


def load_package_data_from_metadata(manifest_path: str) -> dict[str, object]:
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
            "Error: cargo metadata failed while resolving Cargo manifest data:\n"
            f"{result.stderr.strip()}"
        )
    metadata = json.loads(result.stdout)
    manifest_realpath = os.path.realpath(manifest_path)
    for package in metadata.get("packages", []):
        if os.path.realpath(package.get("manifest_path", "")) == manifest_realpath:
            return {
                "features": package.get("features", {}),
                "package": {"metadata": package.get("metadata", {})},
            }
    raise SystemExit(f"Error: package for manifest not found in cargo metadata: {manifest_path}")


def load_package_data(manifest_path: str) -> dict[str, object]:
    return load_package_data_from_toml(manifest_path) or load_package_data_from_metadata(
        manifest_path
    )


def load_feature_graph(package_data: dict[str, object], manifest_path: str) -> dict[str, list[str]]:
    features = package_data.get("features")
    if not isinstance(features, dict):
        raise SystemExit(f"Error: missing [features] table in {manifest_path}")
    return {
        str(name): [str(entry) for entry in entries]
        for name, entries in features.items()
    }
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


def load_package_profiles(
    package_data: dict[str, object], manifest_path: str
) -> tuple[dict[str, list[str]], dict[str, str]]:
    package = package_data.get("package")
    if not isinstance(package, dict):
        raise SystemExit(f"Error: missing [package] table in {manifest_path}")
    metadata = package.get("metadata")
    if not isinstance(metadata, dict):
        raise SystemExit(f"Error: missing [package.metadata] table in {manifest_path}")
    beetle = metadata.get("beetle")
    if not isinstance(beetle, dict):
        raise SystemExit(
            f"Error: missing [package.metadata.beetle] table in {manifest_path}"
        )
    package_profiles = beetle.get("package_profiles")
    if not isinstance(package_profiles, dict):
        raise SystemExit(
            "Error: missing [package.metadata.beetle.package_profiles] table in "
            f"{manifest_path}"
        )
    raw_profiles = package_profiles.get("profiles")
    if not isinstance(raw_profiles, dict):
        raise SystemExit(
            "Error: missing [package.metadata.beetle.package_profiles.profiles] table in "
            f"{manifest_path}"
        )
    raw_defaults = package_profiles.get("defaults")
    if not isinstance(raw_defaults, dict):
        raise SystemExit(
            "Error: missing [package.metadata.beetle.package_profiles.defaults] table in "
            f"{manifest_path}"
        )
    profiles = {
        str(name): [str(entry) for entry in entries]
        for name, entries in raw_profiles.items()
    }
    defaults = {str(name): str(value) for name, value in raw_defaults.items()}
    return profiles, defaults


def roots_from_package_profile(
    profile_name: str, profiles: dict[str, list[str]]
) -> list[str]:
    roots = profiles.get(profile_name)
    if roots is None:
        raise SystemExit(f"Error: package profile not found: {profile_name}")
    if not roots:
        raise SystemExit(f"Error: package profile has no roots: {profile_name}")
    return roots


def default_package_profile_for_target_kind(
    target_kind: str, defaults: dict[str, str], profiles: dict[str, list[str]]
) -> str:
    profile_name = defaults.get(target_kind)
    if profile_name is None:
        raise SystemExit(f"Error: default package profile not found for target kind: {target_kind}")
    if profile_name not in profiles:
        raise SystemExit(
            "Error: default package profile points to an unknown profile: "
            f"{target_kind} -> {profile_name}"
        )
    return profile_name


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Resolve Beetle Cargo feature closures and package-profile metadata."
    )
    parser.add_argument("--manifest", required=True, help="Path to Cargo.toml")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--roots",
        help="Comma-separated Cargo feature roots to expand",
    )
    group.add_argument(
        "--package-profile",
        help="Named Beetle package profile to resolve from Cargo metadata",
    )
    group.add_argument(
        "--default-target-kind",
        choices=("esp", "linux"),
        help="Resolve the default Beetle package profile for the target kind",
    )
    parser.add_argument(
        "--format",
        choices=("csv", "shell-args", "roots-csv", "value"),
        default="csv",
        help="Output format",
    )
    args = parser.parse_args()

    manifest_path = os.path.realpath(args.manifest)
    package_data = load_package_data(manifest_path)
    feature_graph = load_feature_graph(package_data, manifest_path)
    profiles: dict[str, list[str]] = {}
    defaults: dict[str, str] = {}
    profile_name: str | None = None

    if args.roots:
        roots = parse_roots(args.roots)
    else:
        profiles, defaults = load_package_profiles(package_data, manifest_path)
        if args.package_profile:
            profile_name = args.package_profile
        else:
            profile_name = default_package_profile_for_target_kind(
                args.default_target_kind, defaults, profiles
            )
        roots = roots_from_package_profile(profile_name, profiles)

    if args.format == "value":
        if profile_name is None:
            raise SystemExit("Error: --format value requires --package-profile or --default-target-kind")
        print(profile_name)
        return 0

    if args.format == "roots-csv":
        print(",".join(roots))
        return 0

    expanded = expand_feature_roots(feature_graph, roots)
    csv = ",".join(expanded)
    if args.format == "shell-args":
        print(f"--no-default-features --features {csv}")
    else:
        print(csv)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
