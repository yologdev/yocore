# First Run

## Create Default Config

```bash
yocore --init
```

This creates `~/.yolog/config.toml` with sensible defaults. The config watches `~/.claude/projects` for Claude Code sessions.

## Start the Server

```bash
yocore
```

Yocore starts on `http://127.0.0.1:19420` by default. On first start it:

1. Creates the data directory (`~/.yolog/`)
2. Initializes the SQLite database
3. Starts watching configured directories for session files
4. Starts the HTTP API server

## Verify

```bash
curl http://127.0.0.1:19420/health
```

```json
{
  "status": "ok",
  "version": "0.2.0",
  "instance_uuid": "84c11d21-d95a-48f1-ac17-b4c5d9e97c44"
}
```

## MCP Mode

For Claude Code integration, start in MCP mode instead:

```bash
yocore --mcp
```

This runs a JSON-RPC server over stdio (no HTTP). See [Claude Code Integration](claude-code-integration.md) for setup.

## Common Options

```bash
yocore --port 8080          # Custom port
yocore --host 0.0.0.0       # Bind to all interfaces (enables LAN discovery)
yocore --verbose             # Debug logging
yocore --config /path/to/config.toml  # Custom config file
```

See [CLI Reference](../reference/cli.md) for all options.
