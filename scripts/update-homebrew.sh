#!/usr/bin/env bash
# scripts/update-homebrew.sh
# Usage: update-homebrew.sh VERSION MACOS_ARM_SHA256 MACOS_X86_SHA256 LINUX_SHA256
#
# Renders Formula/hom.rb.tmpl → Formula/hom.rb with the given version and SHA256 values.
# Called by .github/workflows/release.yml after binaries are uploaded.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

VERSION="${1:?VERSION required}"
MACOS_ARM_SHA256="${2:?MACOS_ARM_SHA256 required}"
MACOS_X86_SHA256="${3:?MACOS_X86_SHA256 required}"
LINUX_SHA256="${4:?LINUX_SHA256 required}"

TMPL="${REPO_ROOT}/Formula/hom.rb.tmpl"
OUT="${REPO_ROOT}/Formula/hom.rb"

if [[ ! -f "${TMPL}" ]]; then
  echo "ERROR: template not found: ${TMPL}" >&2
  exit 1
fi

python3 - "${VERSION}" "${MACOS_ARM_SHA256}" "${MACOS_X86_SHA256}" "${LINUX_SHA256}" \
         "${TMPL}" "${OUT}" <<'EOF'
import sys, pathlib
ver, arm, x86, linux, tmpl, out = sys.argv[1:]
text = pathlib.Path(tmpl).read_text()
text = (text
    .replace("VERSION", ver)
    .replace("MACOS_ARM_SHA256", arm)
    .replace("MACOS_X86_SHA256", x86)
    .replace("LINUX_SHA256", linux))
pathlib.Path(out).write_text(text)
EOF

echo "Rendered ${OUT} for version ${VERSION}"
