# Installation

## npm (Recommended)

```bash
npm install -g @yologdev/core
```

Pre-built binaries are included for:

| Platform | Architecture |
|----------|-------------|
| macOS | Apple Silicon (ARM64) |
| macOS | Intel (x64) |
| Linux | x64 |
| Windows | x64 |

## Binary Download

Download the latest release from [GitHub Releases](https://github.com/yologdev/yocore/releases).

Extract and place the binary in your PATH:

```bash
# macOS / Linux
tar -xzf yocore-darwin-arm64.tar.gz
sudo mv yocore /usr/local/bin/

# Windows
# Extract yocore-windows-x64.zip and add to PATH
```

## Build from Source

Requires [Rust](https://rustup.rs/) 1.75+.

```bash
git clone https://github.com/yologdev/yocore.git
cd yocore
cargo build --release
```

The binary is at `target/release/yocore`.

## Verify Installation

```bash
yocore --version
```
