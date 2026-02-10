# First Run

## Create Default Config

```bash
yocore --init
```

This creates `~/.yolog/config.toml` with sensible defaults. The config watches `~/.claude/projects` for Claude Code sessions.

## Start Yocore

```bash
yocore
```

Yocore starts on `http://127.0.0.1:19420` by default. On first start it:

1. Creates the data directory (`~/.yolog/`)
2. Initializes the SQLite database
3. Starts watching configured directories for session files
4. Starts the HTTP API server

That's it — yocore watches your AI session files in the background. Any new or updated session files are automatically parsed and stored.

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

## View Your Sessions

Use the [Yolog](https://github.com/yologdev/support) app to browse and replay your AI coding sessions. It connects to yocore's HTTP API and provides a full session viewer with search, filtering, and timeline visualization.

For LAN access from other machines, see [LAN Discovery](../guides/lan-discovery.md).

## Common Options

```bash
yocore --port 8080          # Custom port
yocore --host 0.0.0.0       # Bind to all interfaces (enables LAN discovery)
yocore --verbose             # Debug logging
yocore --config /path/to/config.toml  # Custom config file
```

See [CLI Reference](../reference/cli.md) for all options.

## What's Next?

- **Session replay** — Install the [Yolog](https://github.com/yologdev/support) app
- **Long-term memory** — Add AI-powered memory extraction with [yoskill](long-term-memory.md) (optional)
- **LAN access** — Share sessions across machines with [mDNS discovery](../guides/lan-discovery.md)
