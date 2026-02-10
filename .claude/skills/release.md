# Yocore Release Skill

Release yocore via GitHub Actions. Builds cross-platform binaries and publishes to GitHub Releases + npm.

## When to Release

**Release when:**
- New features are complete and tested
- Critical bug fixes
- Security updates

**Don't release for:**
- Documentation-only changes
- CI/tooling changes

## Release Workflow

### Step 1: Determine Version

```bash
# Get last version tag
LAST_TAG=$(git tag -l "v*" --sort=-v:refname | head -1)
echo "Last release: $LAST_TAG"

# Review commits since last release
git log $LAST_TAG..HEAD --oneline
```

**Version selection (semver):**
- Breaking changes → Bump MAJOR (0.x → 1.0)
- New features → Bump MINOR (0.1 → 0.2)
- Bug fixes only → Bump PATCH (0.1.0 → 0.1.1)

### Step 2: Update Version Files

Update version in **both** files — they MUST match:

**Cargo.toml:**
```toml
[package]
version = "X.Y.Z"
```

**npm/package.json:**
```json
{
  "version": "X.Y.Z"
}
```

### Step 3: Update CHANGELOG.md

Add a new version section at the top:

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Added
- New feature 1
- New feature 2

### Fixed
- Bug fix 1

### Changed
- Change 1
```

### Step 4: Commit

```bash
git add Cargo.toml npm/package.json CHANGELOG.md
git commit -m "chore: prepare vX.Y.Z release

- Bump version to X.Y.Z
- Update CHANGELOG.md with release notes"
```

### Step 5: Push and Create Tag

```bash
# Push commit
git push origin main

# Create annotated tag
git tag -a vX.Y.Z -m "vX.Y.Z - [Brief description]

Highlights:
- Feature 1
- Feature 2
- Fix 1

Platforms: macOS (ARM64, x64), Linux (x64), Windows (x64)"

# Push tag (triggers GitHub Actions build)
git push origin vX.Y.Z
```

### Step 6: Monitor Build

1. Go to: https://github.com/yologdev/yocore/actions
2. Find "Release" workflow triggered by tag
3. Watch all 4 platform builds:
   - `macos-latest` (x86_64 + aarch64)
   - `ubuntu-latest` (x86_64)
   - `windows-latest` (x86_64)

**Build time:** ~5-10 minutes

### Step 7: Publish npm

After GitHub Release is created:

```bash
cd npm
# Update version if not already matching
npm publish --access public
cd ..
```

### Step 8: Update Release Notes

```bash
gh release edit vX.Y.Z --notes "$(cat <<'EOF'
## [Release Title]

### Highlights
- Feature 1
- Feature 2

### Installation
```
npm install -g @yologdev/core
```

### Downloads
- **macOS (Apple Silicon):** `yocore-darwin-arm64.tar.gz`
- **macOS (Intel):** `yocore-darwin-x64.tar.gz`
- **Linux:** `yocore-linux-x64.tar.gz`
- **Windows:** `yocore-windows-x64.zip`
EOF
)"
```

### Step 9: Verify

1. **GitHub Release:** https://github.com/yologdev/yocore/releases
   - All 4 platform binaries present
   - Release notes are meaningful

2. **npm:**
   ```bash
   npm info @yologdev/core version
   # Should show X.Y.Z
   ```

3. **Local test:**
   ```bash
   cargo build --release
   ./target/release/yocore --version
   # Should show X.Y.Z
   ```

## Rollback Strategy

### Option 1: Delete Release
```bash
gh release delete vX.Y.Z
git push origin :refs/tags/vX.Y.Z
git tag -d vX.Y.Z
```

### Option 2: Fix Forward
```bash
# Make fixes on main
git commit -m "fix: critical bug in vX.Y.Z"

# Release patch version
# Follow steps 2-9 with X.Y.Z+1
```

### Option 3: Unpublish npm (within 72 hours)
```bash
npm unpublish @yologdev/core@X.Y.Z
```

## Files Modified in Release

```
Cargo.toml          # version field
npm/package.json    # version field
CHANGELOG.md        # release notes
```

## Distribution Channels

| Channel | How | Audience |
|---------|-----|----------|
| **GitHub Releases** | Automatic (tag push) | Direct binary downloads |
| **npm** | Manual `npm publish` | Node.js developers, CI pipelines |
| **Homebrew** | TODO: tap formula | macOS users |
| **Source** | `cargo install --git` | Rust developers |

## Checklist

**Before release:**
- [ ] All tests pass (`cargo test`)
- [ ] Clippy clean (`cargo clippy -- -D warnings`)
- [ ] Version updated in Cargo.toml
- [ ] Version updated in npm/package.json
- [ ] CHANGELOG.md updated
- [ ] On main branch, working tree clean

**During release:**
- [ ] Tag created and pushed
- [ ] GitHub Actions workflow triggered

**After release:**
- [ ] All 4 platform builds succeeded
- [ ] GitHub Release has all artifacts
- [ ] npm published with correct version
- [ ] Release notes updated
- [ ] Tested binary works
