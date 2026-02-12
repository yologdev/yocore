# Troubleshooting & FAQ

## Port Already in Use

```
ERROR Port 19420 is already in use — another yocore instance may be running.
```

Another yocore instance (or another service) is using the port. Check with:

```bash
curl http://127.0.0.1:19420/health
```

Use a different port: `yocore --port 8080`

## Config File Not Found

```
WARN Config file not found at ~/.yolog/config.toml, using defaults
```

Create one with `yocore --init`, or yocore will use built-in defaults.

## AI Features Not Working

1. Verify Claude Code CLI is installed: `claude --version`
2. Verify AI is enabled in config:
   ```toml
   [ai]
   enabled = true
   provider = "claude_code"
   ```
3. Check AI CLI status: `curl http://localhost:19420/api/ai/cli/status`

## mDNS Not Working

- **Localhost binding**: mDNS is auto-disabled when `host = "127.0.0.1"`. Set `host = "0.0.0.0"`.
- **Firewall**: Ensure UDP port 5353 (multicast) is not blocked.
- **Verify**: On macOS, run `dns-sd -B _yocore._tcp` to check if the service is visible.

## Database Issues

Yocore uses SQLite with WAL mode and dual connections, so database locks should be rare. If you encounter issues:

```bash
# Check database integrity
sqlite3 ~/.yolog/yocore.db "PRAGMA integrity_check;"
```

## Reset Everything

Delete the data directory to start fresh:

```bash
rm -rf ~/.yolog/
yocore --init
```

This removes the database, config, and all stored data.

## Logs

Yocore logs to stdout. Enable verbose logging for debugging:

```bash
yocore --verbose
```

Or set the log level via environment:

```bash
RUST_LOG=yocore=debug yocore
```

## Report Issues

File bugs at [github.com/yologdev/yocore/issues](https://github.com/yologdev/yocore/issues).
