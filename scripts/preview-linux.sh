#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -s) == Linux ]] || { echo 'Run this preview inside Linux.' >&2; exit 1; }
cargo build --release --locked
export AI_MD_HOME="${AI_MD_HOME:-${QUIET_MD_HOME:-$(mktemp -d "${TMPDIR:-/tmp}/ai-md-linux-preview.XXXXXX")}}"
printf 'Preview library: %s\n' "$AI_MD_HOME"
exec "${CARGO_TARGET_DIR:-target}/release/ai_md"
