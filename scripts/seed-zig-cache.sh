#!/usr/bin/env bash
# scripts/seed-zig-cache.sh
#
# One-time helper: build hom-terminal with the ghostty-backend feature on a
# machine with internet access so Zig downloads and caches its C source
# packages under ~/.cache/zig/p/.
#
# Run this once on a new self-hosted CI runner (or developer machine) BEFORE
# going offline or before first-time CI use.  After seeding, the runner's Zig
# cache can be preserved via GitHub Actions `actions/cache` to avoid repeated
# network hits.
#
# Prerequisites:
#   - Zig >= 0.15.x  (verify: zig version)
#   - Rust stable    (verify: cargo --version)
#   - Network access to deps.files.ghostty.org

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

echo "=== HOM: seeding Zig package cache for ghostty-backend ==="
echo ""
echo "Zig:   $(zig version)"
echo "Cargo: $(cargo --version)"
echo ""
echo "Building hom-terminal --features ghostty-backend ..."
echo "(First build fetches C sources from deps.files.ghostty.org)"
echo ""

cargo build --features ghostty-backend -p hom-terminal

echo ""
echo "=== Zig cache seeded ==="
ZIG_CACHE="${HOME}/.cache/zig/p"
if [ -d "$ZIG_CACHE" ]; then
    echo "Zig packages at: ${ZIG_CACHE}"
    ls "$ZIG_CACHE"
else
    echo "Note: ${ZIG_CACHE} not found — check your Zig version."
fi

echo ""
echo "Next: register a GitHub Actions self-hosted runner with the 'zig' label."
echo "See .github/workflows/ci.yml (ghostty job) for the CI configuration."
