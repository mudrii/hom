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
echo "=== Next: register a GitHub Actions self-hosted runner ==="
echo ""
echo "The CI ghostty job requires a runner with labels [self-hosted, zig]."
echo "Steps to register (run once per machine):"
echo ""
echo "  1. Go to: https://github.com/<owner>/<repo>/settings/actions/runners/new"
echo "     (or for an org: https://github.com/organizations/<org>/settings/actions/runners/new)"
echo ""
echo "  2. Download and configure the runner:"
echo "       mkdir -p ~/actions-runner && cd ~/actions-runner"
echo "       curl -o actions-runner.tar.gz -L <download URL from GitHub>"
echo "       tar xzf actions-runner.tar.gz"
echo "       ./config.sh --url https://github.com/<owner>/<repo> --token <TOKEN>"
echo ""
echo "  3. Add the 'zig' label during config (--labels zig) or in the UI after registration."
echo ""
echo "  4. Install as a service (macOS/Linux):"
echo "       sudo ./svc.sh install && sudo ./svc.sh start"
echo ""
echo "  5. Verify the runner appears as online at:"
echo "       https://github.com/<owner>/<repo>/settings/actions/runners"
echo ""
echo "See .github/workflows/ci.yml (ghostty job) for the full CI configuration."
