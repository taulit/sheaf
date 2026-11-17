#!/usr/bin/env bash
# Unit tests of the pure logic of the Visor module (visor-module/lib), with plain gjs.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec gjs -m "$repo/visor-module/tests/run.js"
