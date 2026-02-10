# CLI Reference

## Usage

```
yocore [OPTIONS]
```

## Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--config <PATH>` | `-c` | `~/.yolog/config.toml` | Path to configuration file |
| `--mcp` | | | Run in MCP server mode (stdio JSON-RPC) |
| `--port <PORT>` | `-p` | | Override server port |
| `--host <HOST>` | | | Override server host |
| `--verbose` | `-v` | | Enable debug logging |
| `--init` | | | Create default config file and exit |
| `--version` | `-V` | | Print version |
| `--help` | `-h` | | Print help |

## Environment Variables

Environment variables override config file values. CLI flags take highest priority.

| Variable | Description | Example |
|----------|-------------|---------|
| `YOLOG_SERVER_HOST` | Override server host | `0.0.0.0` |
| `YOLOG_SERVER_PORT` | Override server port | `8080` |
| `YOLOG_SERVER_API_KEY` | Set API key for authentication | `my-secret-key` |
| `YOLOG_DATA_DIR` | Override data directory | `/data/yocore` |
| `YOLOG_CONFIG_READONLY` | Prevent config changes via API | `true` |
| `ANTHROPIC_API_KEY` | Used by Claude Code CLI (not yocore directly) | `sk-ant-...` |

## Precedence

Settings are resolved in this order (highest priority first):

1. CLI flags (`--port`, `--host`)
2. Environment variables (`YOLOG_SERVER_PORT`, etc.)
3. Config file (`~/.yolog/config.toml`)
4. Built-in defaults

## Examples

```bash
# Start with defaults
yocore

# Create default config
yocore --init

# Custom port and verbose logging
yocore --port 8080 --verbose

# Bind to all interfaces for LAN discovery
yocore --host 0.0.0.0

# MCP mode for Claude Code
yocore --mcp

# Use a custom config file
yocore --config /path/to/config.toml

# Override via environment
YOLOG_SERVER_PORT=9000 YOLOG_SERVER_API_KEY=secret yocore
```
