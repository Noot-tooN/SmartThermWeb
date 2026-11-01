# HTTP API

Same origin, JSON. All temperatures are integer tenths of °C. Errors use `{"error":{"code":"...","message":"..."}}`. The unauthenticated MVP is restricted to loopback.

| Method | Path | Behavior |
| --- | --- | --- |
| GET | `/api/health` | Process health and provider name, not device connectivity |
| GET | `/api/thermostat` | Current full snapshot, `Cache-Control: no-store` |
| PUT | `/api/thermostat/target` | Validate and queue desired target, HTTP 202 |
| GET | `/api/events` | SSE `state` events containing full snapshots; initial snapshot on each connection |
| GET / PUT | `/api/simulator` | Read/change simulator scenario and latencies |

## Temperature request

```json
{"target_temperature":230,"client_id":"random-browser-session","client_revision":3}
```

Targets must be 50–300 inclusive and divisible by 5. `client_id` and `client_revision` may both be omitted for scripts. If supplied, the ID must be 1–80 bytes and revision positive. A duplicate or older revision from that browser returns 409 and cannot alter the desired target. The client registry is bounded at 256 identities; a new identity beyond this limit receives 429. Request validation errors return 400, unknown API routes 404, and unsupported methods 405.

A 202 response contains a complete snapshot, including `session`, `revision`, `connectivity`, `last_sync`, `confirmed`, `desired`, `command`, and `in_flight`. The latest command may be queued while an older command is in flight. Before the first successful device read, confirmed state, desired target, and last_sync may be null. Without a command, command/in_flight are null. The confirmed state contains room and target temperatures, mode, provider revision, and optional applied-command evidence. `last_sync` is Unix seconds.

## SSE

Events have `event: state`, `id: <session>:<revision>`, and JSON data. Clients should replace their server snapshot atomically and ignore revisions no newer than the one already held for that session. Reconnect always sends the current full snapshot; there is no history replay even if Last-Event-ID is supplied. SSE keepalives keep the connection active. A disconnected stream does not itself prove the physical device is offline; the UI shows those connections separately.

## Simulator

```json
{"scenario":"reply_lost","ack_ms":900,"apply_ms":2400}
```

Scenarios: `normal`, `rejected`, `transport_failure`, `reply_lost`, `ack_timeout`, `device_timeout`, `offline`. Reply delay is 0–30000 ms; application delay 0–60000 ms. The current config is returned after a successful PUT. Rejected/transport_failure/device_timeout/offline never schedule a new device application. Reply_lost and ack_timeout do schedule application, even though the server reply is unavailable. Returning from offline allows pending scheduled applications to be observed.
