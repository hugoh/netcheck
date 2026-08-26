#!/usr/bin/env bash
#MISE description="Run the Swift suite including live network/system tests"
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

NETCHECK_LIVE_TESTS=1 swift test --package-path "$ROOT_DIR/apps/NetCheck"
