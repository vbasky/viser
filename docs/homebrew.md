# Publishing to Homebrew

Viser aims to be installable via `brew install viser` directly from
[Homebrew/homebrew-core](https://github.com/Homebrew/homebrew-core).

## Current status

Not yet submitted. The formula is created from source, so it must build
on Homebrew's CI (which it does — `cargo build` with `fontconfig` via
Homebrew as a system dep for chart rendering).

## Submit homebrew-core PR

After each release, open a PR against `Homebrew/homebrew-core`:

```sh
# Clone the homebrew-core tap
git clone https://github.com/Homebrew/homebrew-core.git
cd homebrew-core

# Create a branch
git checkout -b viser-<version>

# Use brew to generate the formula stub
brew extract --version <version> viser homebrew/core

# Commit and push
git add Formula/v/viser.rb
git commit -m "viser <VERSION> (new formula)"
gh pr create --repo Homebrew/homebrew-core --fill
```

## CI formula artifact

The release workflow (`.github/workflows/release.yml`) prints the formula
with the correct SHA to its build log in the `formula` job. Find it under
the workflow run for the release tag.

## Requirements for homebrew-core acceptance

- Formula builds on macOS (ARM + Intel) and Linux — CI verifies this
- `brew audit --strict viser` passes
- `brew test viser` passes
- No vendored dependencies (cargo handles this)
- System deps are pure Homebrew formulas (fontconfig is already in core)
