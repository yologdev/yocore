# Yocore mDNS Discovery — Integration Guide

This document describes how yocore announces itself on the local network via mDNS (Bonjour/Zeroconf), and how the yolog desktop app can discover instances automatically.

## Overview

When yocore is bound to a non-localhost address (e.g. `host = "0.0.0.0"`), it announces a `_yocore._tcp.local.` mDNS service on the LAN. Desktop clients can browse for this service type to find all running yocore instances without manual configuration.

mDNS is **disabled by default** because yocore defaults to `host = "127.0.0.1"`. Users who want LAN discovery must set `host = "0.0.0.0"` in their config.

## Service Details

| Field | Value |
|-------|-------|
| Service type | `_yocore._tcp.local.` |
| Protocol | mDNS (multicast UDP on `224.0.0.251:5353`) |
| Transport | TCP (the HTTP API) |

### TXT Record Properties

Each announced service includes these TXT record key-value pairs:

| Key | Type | Example | Description |
|-----|------|---------|-------------|
| `version` | string | `"0.1.0"` | Yocore version (semver) |
| `uuid` | string | `"84c11d21-d95a-48f1-ac17-b4c5d9e97c44"` | Persistent instance UUID (survives restarts) |
| `hostname` | string | `"Yuanhaos-Mac-mini.local"` | Machine hostname |
| `api_key_required` | string | `"true"` or `"false"` | Whether the API requires authentication |
| `projects` | string | `"3"` | Number of projects (as string, parse to int) |

### Instance Name Format

The mDNS instance name follows the pattern:

```
Yocore-{hostname}-{short_uuid}
```

Example: `Yocore-Yuanhaos-Mac-mini.local-84c11d21`

Users can override this via config:

```toml
[server]
instance_name = "My Workstation"
```

## Discovery Flow

```
┌──────────────────┐                          ┌──────────────────┐
│  yolog desktop   │                          │     yocore       │
│                  │                          │  (host=0.0.0.0)  │
└────────┬─────────┘                          └────────┬─────────┘
         │                                             │
         │  1. Browse "_yocore._tcp.local."            │
         │────────────────────────────────────────────>│
         │                                             │
         │  2. Service found:                          │
         │     Name: Yocore-macbook-a1b2c3d4           │
         │     IP:   192.168.1.42                      │
         │     Port: 19420                             │
         │     TXT:  uuid=..., version=0.1.0, ...      │
         │<────────────────────────────────────────────│
         │                                             │
         │  3. GET http://192.168.1.42:19420/health    │
         │────────────────────────────────────────────>│
         │                                             │
         │  4. {"status":"ok",                         │
         │      "version":"0.1.0",                     │
         │      "instance_uuid":"84c11d21-..."}        │
         │<────────────────────────────────────────────│
         │                                             │
         │  5. Correlate: TXT uuid == health uuid      │
         │     Add to instance list, start using API   │
         │                                             │
```

### Step-by-step

1. **Browse** for `_yocore._tcp.local.` using an mDNS library
2. **Receive** service announcements — each contains IP address, port, and TXT metadata
3. **Verify** connectivity by calling `GET /health` on the discovered address
4. **Correlate** the `uuid` from TXT records with the `instance_uuid` from `/health` to confirm identity
5. **Add** the instance to the connection list and start using the API at `http://{ip}:{port}/api/...`

## Health Endpoint

```
GET /health
```

No authentication required. Returns:

```json
{
  "status": "ok",
  "version": "0.1.0",
  "instance_uuid": "84c11d21-d95a-48f1-ac17-b4c5d9e97c44"
}
```

The `instance_uuid` is persistent across restarts and matches the `uuid` TXT record from mDNS. Use this to de-duplicate instances (e.g., if an instance restarts and re-announces).

## Recommended Libraries

### Electron / Node.js

```bash
npm install bonjour-service
```

```typescript
import Bonjour from 'bonjour-service';

const bonjour = new Bonjour();

const browser = bonjour.find({ type: 'yocore' }, (service) => {
  console.log('Found yocore instance:', {
    name: service.name,              // "Yocore-macbook-a1b2c3d4"
    host: service.host,              // "192.168.1.42"
    port: service.port,              // 19420
    uuid: service.txt.uuid,          // "84c11d21-..."
    version: service.txt.version,    // "0.1.0"
    hostname: service.txt.hostname,  // "macbook.local"
    apiKeyRequired: service.txt.api_key_required === 'true',
    projects: parseInt(service.txt.projects, 10),
  });
});

// Stop browsing when done
browser.stop();
bonjour.destroy();
```

### Swift (macOS/iOS native)

```swift
import Network

let browser = NWBrowser(for: .bonjour(type: "_yocore._tcp", domain: nil), using: .tcp)
browser.browseResultsChangedHandler = { results, changes in
    for result in results {
        if case let .service(name, type, domain, _) = result.endpoint {
            print("Found: \(name)")
            // Resolve to get IP and TXT records
        }
    }
}
browser.start(queue: .main)
```

### Tauri / Rust

```toml
# Cargo.toml
mdns-sd = "0.11"
```

```rust
use mdns_sd::{ServiceDaemon, ServiceEvent};

let mdns = ServiceDaemon::new().unwrap();
let receiver = mdns.browse("_yocore._tcp.local.").unwrap();

while let Ok(event) = receiver.recv() {
    match event {
        ServiceEvent::ServiceResolved(info) => {
            println!("Found: {}", info.get_fullname());
            println!("  IP: {:?}", info.get_addresses());
            println!("  Port: {}", info.get_port());
            println!("  UUID: {}", info.get_property_val_str("uuid").unwrap_or(""));
        }
        ServiceEvent::ServiceRemoved(_, fullname) => {
            println!("Removed: {}", fullname);
        }
        _ => {}
    }
}
```

## Edge Cases to Handle

### Instance Goes Offline

mDNS services are automatically removed when the yocore process shuts down gracefully (SIGTERM/SIGINT). However, if the process crashes or the machine loses power:

- The mDNS announcement will **expire after the TTL** (typically 75 minutes)
- **Recommendation**: Periodically verify discovered instances by polling `/health` (e.g., every 30 seconds). Remove instances that fail 3 consecutive health checks.

### Multiple Instances on Same Machine

Each yocore instance uses a different port and has a unique UUID. The mDNS instance name includes a short UUID to differentiate them. Use the `uuid` field (not the name or IP) as the unique key.

### API Key Required

If `api_key_required` is `"true"` in the TXT record, the user must provide an API key. All `/api/*` endpoints require the key as a `Bearer` token in the `Authorization` header:

```
Authorization: Bearer <api_key>
```

The `/health` endpoint never requires authentication.

### Network Changes

When the machine's network interface changes (e.g., switching Wi-Fi networks), the advertised IP address may change. The UUID remains stable. Re-resolve the service or use the health check to detect address changes.

## Yocore Config Reference

Users configure mDNS in `~/.yolog/config.toml`:

```toml
[server]
host = "0.0.0.0"          # Required for mDNS (default: "127.0.0.1")
port = 19420               # API port (default: 19420)
mdns_enabled = true        # Enable mDNS announcement (default: true)
instance_name = "My PC"    # Custom display name (optional)
api_key = "secret123"      # Require auth for API (optional)
```

| Setting | Default | Notes |
|---------|---------|-------|
| `host` | `"127.0.0.1"` | Must be `"0.0.0.0"` for mDNS to activate |
| `mdns_enabled` | `true` | Set `false` to disable mDNS even on `0.0.0.0` |
| `instance_name` | auto-generated | Override the mDNS display name |

## Platform Notes

The yocore mDNS implementation uses the `mdns-sd` Rust crate which implements the mDNS protocol in userspace (raw UDP multicast). It does **not** depend on OS-level mDNS services.

| Platform | Works | Notes |
|----------|-------|-------|
| macOS | Yes | Bonjour built-in; no extra setup |
| Linux | Yes | Works standalone; Avahi not required |
| Windows | Yes | Works standalone; Bonjour not required |

On the **client side** (yolog desktop), the same applies — use a library that handles mDNS directly (like `bonjour-service` for Node.js or `mdns-sd` for Rust) rather than depending on OS services.
