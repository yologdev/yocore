# SSE Events Reference

Yocore provides real-time event streaming via Server-Sent Events (SSE).

## Connecting

```
GET /api/events
```

Requires authentication if `api_key` is configured.

- **Heartbeat**: Every 30 seconds
- **Keep-alive**: Every 15 seconds

## Event Types

### Watcher Events

| Event | Description | Fields |
|-------|-------------|--------|
| `session:new` | New session file detected | `project_id`, `file_path`, `file_name` |
| `session:changed` | Session file grew | `session_id`, `file_path`, `previous_size`, `new_size` |
| `session:parsed` | Session parsing completed | `session_id`, `message_count` |
| `watcher:error` | File watcher error | `file_path`, `error` |

### AI Events

| Event | Description | Fields |
|-------|-------------|--------|
| `ai:title:start` | Title generation started | `session_id` |
| `ai:title:complete` | Title generation completed | `session_id`, `title` |
| `ai:title:error` | Title generation failed | `session_id`, `error` |
| `ai:memory:start` | Memory extraction started | `session_id` |
| `ai:memory:complete` | Memory extraction completed | `session_id`, `count` |
| `ai:memory:error` | Memory extraction failed | `session_id`, `error` |
| `ai:skill:start` | Skill extraction started | `session_id` |
| `ai:skill:complete` | Skill extraction completed | `session_id`, `count` |
| `ai:skill:error` | Skill extraction failed | `session_id`, `error` |
| `ai:markers:start` | Marker detection started | `session_id` |
| `ai:markers:complete` | Marker detection completed | `session_id`, `count` |
| `ai:markers:error` | Marker detection failed | `session_id`, `error` |

### Ranking Events

| Event | Description | Fields |
|-------|-------------|--------|
| `ai:ranking:start` | Memory ranking started | `project_id` |
| `ai:ranking:complete` | Memory ranking completed | `project_id`, `promoted`, `demoted`, `removed` |
| `ai:ranking:error` | Memory ranking failed | `project_id`, `error` |

### Scheduler Events

| Event | Description | Fields |
|-------|-------------|--------|
| `scheduler:start` | Background task started | `task_name`, `project_id` |
| `scheduler:complete` | Background task completed | `task_name`, `project_id`, `detail` |
| `scheduler:error` | Background task failed | `task_name`, `project_id`, `error` |

### System Events

| Event | Description | Fields |
|-------|-------------|--------|
| `heartbeat` | Connection keepalive | `timestamp` |

## Example: JavaScript EventSource

```javascript
const source = new EventSource('http://localhost:19420/api/events');

source.addEventListener('session:parsed', (event) => {
  const data = JSON.parse(event.data);
  console.log(`Session ${data.session_id} parsed: ${data.message_count} messages`);
});

source.addEventListener('ai:title:complete', (event) => {
  const data = JSON.parse(event.data);
  console.log(`Title generated for ${data.session_id}: ${data.title}`);
});

source.addEventListener('heartbeat', () => {
  // Connection alive
});

source.onerror = () => {
  console.log('SSE connection lost, will auto-reconnect');
};
```

## Example: curl

```bash
curl -N http://localhost:19420/api/events
```

Output:

```
event: heartbeat
data: {"type":"heartbeat","timestamp":"2026-02-10T16:00:00Z"}

event: session:parsed
data: {"type":"session_parsed","session_id":"abc123","message_count":42}
```
