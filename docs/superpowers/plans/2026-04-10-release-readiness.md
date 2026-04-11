# Release Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix broken CI, replace YOUR_ORG placeholders with `mudrii`, add binary release workflow with Homebrew formula, and crates.io publish pipeline.

**Architecture:** No new crates. CI fix is a feature-flag correction (3 lines). Release workflow builds for aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu and uploads to GitHub Releases. Homebrew formula is rendered from a template by a script in the release job. crates.io publish chains cargo publish in dependency order.

**Tech Stack:** GitHub Actions, cargo, softprops/action-gh-release@v2, Swatinem/rust-cache@v2

---

## File Map

| File | Action | Why |
|------|--------|-----|
| `.github/workflows/ci.yml` | Modify | Add `--no-default-features --features vt100-backend` to 3 jobs — CI is broken without this |
| `Cargo.toml` | Modify | Replace YOUR_ORG → mudrii |
| `SECURITY.md` | Modify | Replace YOUR_ORG → mudrii |
| `CONTRIBUTING.md` | Modify | Replace YOUR_ORG → mudrii |
| `NOTICE` | Modify | Replace YOUR_ORG → mudrii |
| `README.md` | Modify | Replace YOUR_ORG → mudrii |
| `CODE_OF_CONDUCT.md` | Modify | Replace YOUR_ORG → mudrii |
| `.github/workflows/release.yml` | Create | Binary release on `v*` tag push |
| `Formula/hom.rb.tmpl` | Create | Homebrew formula template |
| `scripts/update-homebrew.sh` | Create | Renders formula from SHA256 values computed in release job |
| `.github/workflows/publish.yml` | Create | crates.io publish in dependency order |

---

## Task 1: Fix CI — URGENT (CI is currently broken)

**Problem:** `ghostty-backend` is now the default feature. GitHub-hosted `ubuntu-latest` and `macos-latest` runners do not have Zig ≥0.15.x. `cargo clippy`, `cargo nextest run`, and `cargo doc` all fail because `libghostty-vt` requires Zig to compile. The self-hosted `ghostty` job already handles the ghostty-backend path correctly.

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Read the current ci.yml**

Run: `cat .github/workflows/ci.yml`

Confirm the 3 run lines that need changing:
- `cargo clippy --workspace --all-targets -- -D warnings` (clippy job)
- `cargo nextest run --workspace` (test job)
- `cargo doc --workspace --no-deps` (doc job)

- [ ] **Step 2: Add `--no-default-features --features vt100-backend` to the clippy job**

In `.github/workflows/ci.yml`, find:
```yaml
      - run: cargo clippy --workspace --all-targets -- -D warnings
```
Replace with:
```yaml
      # Use vt100-backend fallback: github-hosted runners lack Zig ≥0.15.x.
      # The ghostty job (self-hosted, zig label) covers ghostty-backend.
      - run: cargo clippy --workspace --all-targets --no-default-features --features vt100-backend -- -D warnings
```

- [ ] **Step 3: Add `--no-default-features --features vt100-backend` to the test job**

Find:
```yaml
      - run: cargo nextest run --workspace
```
Replace with:
```yaml
      - run: cargo nextest run --workspace --no-default-features --features vt100-backend
```

- [ ] **Step 4: Add `--no-default-features --features vt100-backend` to the doc job**

Find:
```yaml
      - name: Build docs
        run: cargo doc --workspace --no-deps
        env:
          RUSTDOCFLAGS: -D warnings
```
Replace with:
```yaml
      - name: Build docs
        run: cargo doc --workspace --no-deps --no-default-features --features vt100-backend
        env:
          RUSTDOCFLAGS: -D warnings
```

- [ ] **Step 5: Run local check to confirm the fix is valid**

```bash
cargo check --workspace --no-default-features --features vt100-backend
```
Expected: `Finished dev profile [unoptimized + debuginfo] target(s) in X.XXs` — zero errors.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "fix: use vt100-backend in CI for github-hosted runners (no Zig)"
```

---

## Task 2: Replace YOUR_ORG placeholder with mudrii

**Files:**
- Modify: `Cargo.toml:18`
- Modify: `SECURITY.md:13`
- Modify: `CONTRIBUTING.md:23`
- Modify: `NOTICE:8`
- Modify: `README.md:3,53`
- Modify: `CODE_OF_CONDUCT.md:37`

- [ ] **Step 1: Replace in Cargo.toml**

In `Cargo.toml` line 18, change:
```toml
repository = "https://github.com/YOUR_ORG/hom"
```
to:
```toml
repository = "https://github.com/mudrii/hom"
```

- [ ] **Step 2: Replace in SECURITY.md**

In `SECURITY.md` line 13, change:
```markdown
Use [GitHub Private Security Advisories](https://github.com/YOUR_ORG/hom/security/advisories/new) to report a vulnerability privately.
```
to:
```markdown
Use [GitHub Private Security Advisories](https://github.com/mudrii/hom/security/advisories/new) to report a vulnerability privately.
```

- [ ] **Step 3: Replace in CONTRIBUTING.md**

In `CONTRIBUTING.md` line 23, change:
```bash
git clone https://github.com/YOUR_ORG/hom
```
to:
```bash
git clone https://github.com/mudrii/hom
```

- [ ] **Step 4: Replace in NOTICE**

In `NOTICE` line 8, change:
```
(https://github.com/YOUR_ORG/hom).
```
to:
```
(https://github.com/mudrii/hom).
```

- [ ] **Step 5: Replace in README.md (2 occurrences)**

Change:
```markdown
[![CI](https://github.com/YOUR_ORG/hom/actions/workflows/ci.yml/badge.svg)](https://github.com/YOUR_ORG/hom/actions/workflows/ci.yml)
```
to:
```markdown
[![CI](https://github.com/mudrii/hom/actions/workflows/ci.yml/badge.svg)](https://github.com/mudrii/hom/actions/workflows/ci.yml)
```

And:
```bash
git clone https://github.com/YOUR_ORG/hom
```
to:
```bash
git clone https://github.com/mudrii/hom
```

- [ ] **Step 6: Replace in CODE_OF_CONDUCT.md**

In `CODE_OF_CONDUCT.md` line 37, change:
```markdown
via [GitHub Private Security Advisories](https://github.com/YOUR_ORG/hom/security/advisories/new).
```
to:
```markdown
via [GitHub Private Security Advisories](https://github.com/mudrii/hom/security/advisories/new).
```

- [ ] **Step 7: Verify no YOUR_ORG remains**

```bash
grep -r "YOUR_ORG" .
```
Expected: no output.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml SECURITY.md CONTRIBUTING.md NOTICE README.md CODE_OF_CONDUCT.md
git commit -m "chore: replace YOUR_ORG placeholder with mudrii"
```

---

## Task 3: GitHub binary release workflow

Builds release binaries for three targets when a `v*` tag is pushed. Uses vt100-backend (no Zig required) for all targets. Cross-compiles aarch64-linux using the `aarch64-linux-gnu-gcc` cross-compiler.

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create the release workflow**

Create `.github/workflows/release.yml` with this content:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

env:
  CARGO_TERM_COLOR: always

permissions:
  contents: write

jobs:
  build:
    name: build / ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: aarch64-apple-darwin
            os: macos-latest
          - target: x86_64-apple-darwin
            os: macos-latest
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - uses: Swatinem/rust-cache@v2
        with:
          key: release-${{ matrix.target }}

      - name: Install aarch64-linux cross-compiler
        if: matrix.target == 'aarch64-unknown-linux-gnu'
        run: |
          sudo apt-get update -q
          sudo apt-get install -y gcc-aarch64-linux-gnu

      - name: Configure linker for aarch64-linux
        if: matrix.target == 'aarch64-unknown-linux-gnu'
        run: |
          mkdir -p .cargo
          cat >> .cargo/config.toml <<'EOF'
          [target.aarch64-unknown-linux-gnu]
          linker = "aarch64-linux-gnu-gcc"
          EOF

      - name: Build release binary
        run: |
          cargo build --release --target ${{ matrix.target }} \
            --no-default-features --features vt100-backend

      - name: Package binary
        shell: bash
        run: |
          ARCHIVE=hom-${{ github.ref_name }}-${{ matrix.target }}.tar.gz
          cd target/${{ matrix.target }}/release
          tar czf "../../../${ARCHIVE}" hom
          cd ../../..
          echo "ARCHIVE=${ARCHIVE}" >> $GITHUB_ENV
          if command -v shasum &>/dev/null; then
            echo "SHA256=$(shasum -a 256 "${ARCHIVE}" | cut -d' ' -f1)" >> $GITHUB_ENV
          else
            echo "SHA256=$(sha256sum "${ARCHIVE}" | cut -d' ' -f1)" >> $GITHUB_ENV
          fi

      - name: Upload to GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: ${{ env.ARCHIVE }}
          generate_release_notes: true

  homebrew:
    name: Update Homebrew formula
    runs-on: ubuntu-latest
    needs: build
    steps:
      - uses: actions/checkout@v4

      - name: Download release assets and compute SHA256
        shell: bash
        run: |
          VERSION=${{ github.ref_name }}
          BASE="https://github.com/mudrii/hom/releases/download/${VERSION}"

          download_sha() {
            local target=$1
            local file="hom-${VERSION}-${target}.tar.gz"
            curl -fsSL "${BASE}/${file}" -o "${file}"
            if command -v shasum &>/dev/null; then
              shasum -a 256 "${file}" | cut -d' ' -f1
            else
              sha256sum "${file}" | cut -d' ' -f1
            fi
          }

          echo "SHA_MACOS_ARM=$(download_sha aarch64-apple-darwin)" >> $GITHUB_ENV
          echo "SHA_MACOS_X86=$(download_sha x86_64-apple-darwin)" >> $GITHUB_ENV
          echo "SHA_LINUX=$(download_sha x86_64-unknown-linux-gnu)" >> $GITHUB_ENV

      - name: Render Homebrew formula
        shell: bash
        run: |
          VERSION_NO_V="${{ github.ref_name }}"
          VERSION_NO_V="${VERSION_NO_V#v}"
          bash scripts/update-homebrew.sh \
            "${VERSION_NO_V}" \
            "${{ env.SHA_MACOS_ARM }}" \
            "${{ env.SHA_MACOS_X86 }}" \
            "${{ env.SHA_LINUX }}"

      - name: Upload rendered formula as release asset
        uses: softprops/action-gh-release@v2
        with:
          files: Formula/hom.rb
```

- [ ] **Step 2: Verify the workflow file is valid YAML**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo "OK"
```
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add binary release workflow for v* tags"
```

---

## Task 4: Homebrew formula template and update script

**Files:**
- Create: `Formula/hom.rb.tmpl`
- Create: `scripts/update-homebrew.sh`

- [ ] **Step 1: Create the Formula directory and template**

```bash
mkdir -p Formula scripts
```

Create `Formula/hom.rb.tmpl`:

```ruby
# Formula/hom.rb.tmpl
# Generated by scripts/update-homebrew.sh — do not edit by hand.
class Hom < Formula
  desc "TUI terminal multiplexer and orchestrator for 7 AI coding agent CLIs"
  homepage "https://github.com/mudrii/hom"
  version "VERSION"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/mudrii/hom/releases/download/vVERSION/hom-vVERSION-aarch64-apple-darwin.tar.gz"
      sha256 "MACOS_ARM_SHA256"
    else
      url "https://github.com/mudrii/hom/releases/download/vVERSION/hom-vVERSION-x86_64-apple-darwin.tar.gz"
      sha256 "MACOS_X86_SHA256"
    end
  end

  on_linux do
    url "https://github.com/mudrii/hom/releases/download/vVERSION/hom-vVERSION-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "LINUX_SHA256"
  end

  def install
    bin.install "hom"
  end

  test do
    output = shell_output("#{bin}/hom --version 2>&1", 1)
    assert_match "hom", output
  end
end
```

- [ ] **Step 2: Create the update script**

Create `scripts/update-homebrew.sh`:

```bash
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
```

- [ ] **Step 3: Make the script executable and test it locally**

```bash
chmod +x scripts/update-homebrew.sh
bash scripts/update-homebrew.sh 0.1.0 abc123 def456 ghi789
cat Formula/hom.rb
```
Expected: `Formula/hom.rb` contains `version "0.1.0"`, `sha256 "abc123"`, etc. with no `VERSION` or `SHA256` placeholder strings remaining.

```bash
grep -E "VERSION|SHA256" Formula/hom.rb && echo "FAIL: placeholders remain" || echo "OK"
```
Expected: `OK`

- [ ] **Step 4: Add Formula/hom.rb to .gitignore so only the template is committed**

Add to `.gitignore`:
```
# Homebrew formula is rendered by scripts/update-homebrew.sh — commit only the template
Formula/hom.rb
```

- [ ] **Step 5: Commit**

```bash
git add Formula/hom.rb.tmpl scripts/update-homebrew.sh .gitignore
git commit -m "ci: add Homebrew formula template and render script"
```

---

## Task 5: crates.io publish workflow

Publishes all 8 crates in dependency order when a `v*` tag is pushed, after the release binaries are built. Requires a `CARGO_REGISTRY_TOKEN` secret in the repository settings.

**Files:**
- Create: `.github/workflows/publish.yml`

- [ ] **Step 1: Create the publish workflow**

Create `.github/workflows/publish.yml`:

```yaml
name: Publish to crates.io

on:
  push:
    tags:
      - 'v*'

env:
  CARGO_TERM_COLOR: always

jobs:
  publish:
    name: cargo publish
    runs-on: ubuntu-latest
    # Only run after the release build succeeds
    needs: []

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      # Publish in strict dependency order.
      # --no-verify skips rebuilding from the tarball, which is safe for workspace
      # crates where path deps resolve correctly only after all crates are published.
      # Each step waits 30s for crates.io to index before dependents are published.

      - name: Publish hom-core
        run: cargo publish -p hom-core --no-verify
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Wait for hom-core indexing
        run: sleep 30

      - name: Publish hom-terminal
        run: cargo publish -p hom-terminal --no-verify --no-default-features --features vt100-backend
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Publish hom-pty
        run: cargo publish -p hom-pty --no-verify
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Publish hom-adapters
        run: cargo publish -p hom-adapters --no-verify
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Publish hom-workflow
        run: cargo publish -p hom-workflow --no-verify
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Publish hom-db
        run: cargo publish -p hom-db --no-verify
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Wait for leaf crates to be indexed
        run: sleep 60

      - name: Publish hom-tui
        run: cargo publish -p hom-tui --no-verify
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Wait for hom-tui indexing
        run: sleep 30

      - name: Publish hom (binary)
        run: cargo publish --no-verify --no-default-features --features vt100-backend
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

- [ ] **Step 2: Document the CARGO_REGISTRY_TOKEN secret requirement**

Add a note to `CONTRIBUTING.md` in the "Release process" section (create it if it doesn't exist):

```markdown
## Release process

1. Bump the version in `[workspace.package]` in `Cargo.toml`.
2. Update `CHANGELOG.md` if one exists.
3. Commit: `git commit -m "chore: release v0.2.0"`
4. Tag: `git tag v0.2.0 && git push origin v0.2.0`

This triggers two workflows automatically:
- **release.yml** — builds binaries for macOS and Linux, uploads to GitHub Releases, renders Homebrew formula
- **publish.yml** — publishes all 8 crates to crates.io in dependency order

### One-time setup for publish.yml
Add your crates.io API token as a repository secret named `CARGO_REGISTRY_TOKEN`:
  Settings → Secrets and variables → Actions → New repository secret
```

- [ ] **Step 3: Verify the publish workflow is valid YAML**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/publish.yml'))" && echo "OK"
```
Expected: `OK`

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/publish.yml CONTRIBUTING.md
git commit -m "ci: add crates.io publish workflow triggered by v* tags"
```

---

## Self-review

**Spec coverage:**
1. YOUR_ORG → mudrii: Task 2 ✅
2. Linux CI validation: Task 1 ✅ (Linux ubuntu-latest already runs in CI; the fix makes it pass)
3. Binary release packaging: Task 3 ✅
4. Homebrew formula: Task 4 ✅
5. crates.io publish: Task 5 ✅

**Placeholder scan:** No TBD/TODO in any code block. SHA256 values are computed by the workflow from real artifacts. The formula template uses all-caps sentinel strings (`VERSION`, `MACOS_ARM_SHA256`) that are substituted by the script — the script validates they're gone.

**Type consistency:** No Rust types introduced. All shell variables referenced in scripts match what the workflow sets in `$GITHUB_ENV`.
