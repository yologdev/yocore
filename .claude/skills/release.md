---
name: release
description: Release yocore to GitHub Releases and npm. Use when the user says "release", "cut a release", "ship", "publish a new version", "bump version", or wants to create a new yocore release. Handles version bumping (Cargo.toml + npm/package.json), CHANGELOG updates, git tagging, CI build monitoring, npm publishing, and verification.
---

# Yocore Release

## Workflow

### 1. Determine Version

```bash
LAST_TAG=$(git tag -l "v*" --sort=-v:refname | head -1)
echo "Last release: $LAST_TAG"
git log $LAST_TAG..HEAD --oneline
```

Semver rules:
- Breaking changes → MAJOR (0.x → 1.0)
- New features → MINOR (0.1 → 0.2)
- Bug fixes → PATCH (0.1.0 → 0.1.1)

### 2. Update Versions

Update **both** files — they MUST match:

- `Cargo.toml` → `version = "X.Y.Z"`
- `npm/package.json` → `"version": "X.Y.Z"`

### 3. Update CHANGELOG.md

Move `[Unreleased]` items into a new version section:

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Added
- ...

### Fixed
- ...
```

### 4. Commit

```bash
git add Cargo.toml npm/package.json CHANGELOG.md
git commit -m "chore: prepare vX.Y.Z release

- Bump version to X.Y.Z
- Update CHANGELOG.md with release notes"
```

### 5. Tag and Push

```bash
git push origin main
git tag -a vX.Y.Z -m "vX.Y.Z - [Brief description]"
git push origin vX.Y.Z
```

Tag push triggers `.github/workflows/release.yml` — builds macOS (x64 + ARM64), Linux, Windows, then auto-publishes to npm.

### 6. Monitor Build

```bash
gh run list --workflow=release.yml --limit 1
gh run watch  # wait for completion
```

All 3 jobs must pass: `build` → `release` (GitHub Release) → `publish-npm`.

### 7. Verify

```bash
# GitHub Release artifacts (4 binaries)
gh release view vX.Y.Z

# npm version
npm info @yologdev/core version

# Local binary
cargo build --release && ./target/release/yocore --version
```

## Rollback

**Delete release:**
```bash
gh release delete vX.Y.Z
git push origin :refs/tags/vX.Y.Z && git tag -d vX.Y.Z
```

**Fix forward:** Create patch release (vX.Y.Z+1) with fixes.

**Unpublish npm** (within 72h, if auto-published): `npm unpublish @yologdev/core@X.Y.Z`

## Pre-flight Checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] On main branch, working tree clean
- [ ] Version matches in Cargo.toml and npm/package.json
- [ ] CHANGELOG.md updated
