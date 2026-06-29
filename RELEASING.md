# Releasing viser

Releases are tag-driven: pushing a `v*` tag triggers
[`.github/workflows/release.yml`](.github/workflows/release.yml), which builds
per-platform binaries, creates the GitHub Release, and publishes the crates to
crates.io.

## Update the changelog first

**Always add a `## [X.Y.Z] - YYYY-MM-DD` section to [`CHANGELOG.md`](CHANGELOG.md)
before tagging.** The release workflow extracts the GitHub Release body from the
section matching the tag. If the section is missing, the release body would be
empty — so both the local release script and the CI workflow now **fail loudly**
rather than publish an empty release.

The section must use the exact header format the extractor matches:

```markdown
## [0.7.1] - 2026-06-16

### Fixed
- ...
### Added
- ...
```

## Cut a release

```bash
# 1. Add the CHANGELOG.md section for the new version (see above).
# 2. From a clean `main`, run the release helper:
./scripts/release.sh 0.7.2
```

`scripts/release.sh` pre-flight-checks that you are on `main` with a clean tree,
that the tag does not already exist, **and that CHANGELOG.md has a populated
section for the version**, then bumps every crate version, syncs `viser = "0.X"`
/ `{ version = "0.X", ... }` lines in crate READMEs and `lib.rs` rustdoc to the
new compat line (e.g. `0.10.0` → `"0.10"`), commits, tags, pushes, and
publishes to crates.io.

## Verify the notes before tagging (optional manual check)

To preview exactly what the release body will contain for a version:

```bash
awk -v ver="0.7.1" \
  '$0 ~ "^## \\[" ver "\\] - " { found=1; next }
   found && /^## / { exit }
   found { print }' CHANGELOG.md
```

Empty output means the section is missing — add it before tagging.

## Backporting to a maintenance line

For patch releases on an older minor line (e.g. `0.6.x`), work on a
`release/0.6.x` branch based off the previous tag, add the `## [0.6.Z]` changelog
section there, bump versions, and push the branch and `v0.6.Z` tag. The same
workflow runs for any `v*` tag regardless of branch.
