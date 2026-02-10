# LAN Discovery (mDNS)

Yocore can announce itself on your local network via mDNS (Bonjour/Zeroconf), so the [Yolog desktop app](https://github.com/yologdev/yolog) and other clients can automatically discover running instances.

## Enable LAN Discovery

By default, yocore binds to `127.0.0.1` (localhost only) and mDNS is disabled. To enable LAN discovery:

```toml
[server]
host = "0.0.0.0"
api_key = "your-secret-key"  # Recommended when exposing to the network
```

That's it — yocore will announce itself as `_yocore._tcp` on the local network.

## Service Details

| Field | Value |
|-------|-------|
| Service type | `_yocore._tcp.local.` |
| Protocol | mDNS (multicast UDP on `224.0.0.251:5353`) |
| Transport | TCP (the HTTP API) |

### TXT Record Properties

| Key | Type | Example | Description |
|-----|------|---------|-------------|
| `version` | string | `"0.2.0"` | Yocore version |
| `uuid` | string | `"84c11d21-..."` | Persistent instance UUID (survives restarts) |
| `hostname` | string | `"macbook.local"` | Machine hostname |
| `name` | string | `"Office Desktop"` | Custom instance name (if configured) |
| `api_key_required` | string | `"true"` / `"false"` | Whether API requires authentication |
| `projects` | string | `"3"` | Number of tracked projects |

## Custom Instance Name

By default, instances are named `Yocore-{hostname}-{short_uuid}`. Set a friendly name:

```toml
[server]
host = "0.0.0.0"
instance_name = "Office Desktop"
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
         │     Name: Office Desktop                    │
         │     IP:   192.168.1.42                      │
         │     Port: 19420                             │
         │     TXT:  uuid=..., version=0.2.0           │
         │<────────────────────────────────────────────│
         │                                             │
         │  3. GET http://192.168.1.42:19420/health    │
         │────────────────────────────────────────────>│
         │                                             │
         │  4. {"status":"ok", "instance_uuid":"..."}  │
         │<────────────────────────────────────────────│
         │                                             │
         │  5. Correlate: TXT uuid == health uuid      │
         │     Add to instance list, start using API   │
```

## Disable mDNS

To expose yocore on the network without mDNS:

```toml
[server]
host = "0.0.0.0"
mdns_enabled = false
```

## Verify Discovery

On macOS:

```bash
dns-sd -B _yocore._tcp
```

## Client Integration

### Node.js / Electron

```javascript
import Bonjour from 'bonjour-service';

const bonjour = new Bonjour();
const browser = bonjour.find({ type: 'yocore' }, (service) => {
  console.log('Found:', {
    name: service.name,
    host: service.host,
    port: service.port,
    uuid: service.txt.uuid,
    version: service.txt.version,
  });
});
```

### Rust

```rust
use mdns_sd::{ServiceDaemon, ServiceEvent};

let mdns = ServiceDaemon::new().unwrap();
let receiver = mdns.browse("_yocore._tcp.local.").unwrap();

while let Ok(event) = receiver.recv() {
    if let ServiceEvent::ServiceResolved(info) = event {
        println!("Found: {} at {:?}:{}", info.get_fullname(),
            info.get_addresses(), info.get_port());
    }
}
```

## Edge Cases

- **Instance goes offline**: mDNS records expire after ~75 minutes if the process crashes. Poll `/health` periodically to detect stale instances.
- **Multiple instances**: Each instance has a unique UUID. Use `uuid` (not name or IP) as the unique key.
- **Network changes**: The UUID stays stable across network changes. Re-resolve the service after Wi-Fi switches.

## Platform Support

| Platform | Notes |
|----------|-------|
| macOS | Works out of the box (Bonjour built-in) |
| Linux | Works standalone (Avahi not required) |
| Windows | Works standalone (Bonjour not required) |

Yocore uses the `mdns-sd` Rust crate which implements mDNS in userspace via raw UDP multicast.
