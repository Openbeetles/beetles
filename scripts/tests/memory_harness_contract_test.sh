#!/usr/bin/env bash
set -euo pipefail

cargo test --lib memory_harness -- --nocapture
cargo test --lib recall_benchmark -- --nocapture
cargo test --lib archive_benchmark -- --nocapture
cargo test --lib persona_regression -- --nocapture
bash scripts/check_platform_isolation.sh
bash scripts/check_runtime_governance.sh
