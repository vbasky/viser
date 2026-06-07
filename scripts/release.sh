#!/usr/bin/env bash
#
# Release viser to crates.io.
#
# Binary builds + GitHub Release happen automatically in CI
# (.github/workflows/release.yml) when the tag is pushed.
#
# Usage: ./scripts/release.sh <version>
# Prerequisites: on master, clean tree

set -euo pipefail

VERSION="${1:?Usage: $0 <version>}"
CRATES=(
    viser-ffmpeg
    viser-quality
    viser-metrics
    viser-hull
    viser-ladder
    viser-shot
    viser-complexity
    viser-encoding
    viser-checkpoint
    viser-pertitle
    viser-pershot
    viser-persegment
    viser-contextaware
    viser-compare
    viser-chart
    viser
    viser-cli
)

# Pre-flight checks
if [[ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]]; then
    echo "ERROR: Must be on main branch"
    exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "ERROR: Working tree is not clean"
    exit 1
fi

if git rev-parse "v$VERSION" >/dev/null 2>&1; then
    echo "ERROR: Tag v$VERSION already exists"
    exit 1
fi

# Bump version in all crate Cargo.tomls
for crate in "${CRATES[@]}"; do
    perl -i -pe "s/^version = \"[^\"]+\"/version = \"$VERSION\"/" "crates/$crate/Cargo.toml"
done

# Update workspace root version references (intra-workspace deps)
for crate in "${CRATES[@]}"; do
    perl -i -pe "s/(${crate//-/\\-} = \{ .*?)version = \"[^\"]+\"/\${1}version = \"$VERSION\"/" "crates/"*/Cargo.toml
done

# Commit and tag
git add -A
git commit -m "Release v$VERSION"
git tag "v$VERSION"

# Push (triggers CI binary build + GitHub Release)
git push origin main
git push origin "v$VERSION"
echo "==> tag pushed — CI is building binaries and creating the GitHub Release"

# Publish to crates.io in dependency order
for crate in "${CRATES[@]}"; do
    cargo publish -p "$crate"
done

echo "Released viser v$VERSION"
