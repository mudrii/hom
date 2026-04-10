#!/usr/bin/env bash
# scripts/update-homebrew.sh
# Usage: update-homebrew.sh VERSION MACOS_ARM_SHA256 MACOS_X86_SHA256 LINUX_SHA256
#
# Renders Formula/hom.rb.tmpl → Formula/hom.rb with the given version and SHA256 values.
# Called by .github/workflows/release.yml after binaries are uploaded.

set -euo pipefail

VERSION="${1:?VERSION required}"
MACOS_ARM_SHA256="${2:?MACOS_ARM_SHA256 required}"
MACOS_X86_SHA256="${3:?MACOS_X86_SHA256 required}"
LINUX_SHA256="${4:?LINUX_SHA256 required}"

TMPL="Formula/hom.rb.tmpl"
OUT="Formula/hom.rb"

if [[ ! -f "${TMPL}" ]]; then
  echo "ERROR: template not found: ${TMPL}" >&2
  exit 1
fi

sed \
  -e "s/VERSION/${VERSION}/g" \
  -e "s/MACOS_ARM_SHA256/${MACOS_ARM_SHA256}/g" \
  -e "s/MACOS_X86_SHA256/${MACOS_X86_SHA256}/g" \
  -e "s/LINUX_SHA256/${LINUX_SHA256}/g" \
  "${TMPL}" > "${OUT}"

echo "Rendered ${OUT} for version ${VERSION}"
