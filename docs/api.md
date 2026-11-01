# HTTP API

Same origin, JSON. All temperatures are integer tenths of °C. Errors use `{"error":{"code":"...","message":"..."}}`. The local build is restricted to loopback. OCSTAT device data requires a browser session; the explicit simulator mode is unauthenticated.

| Method | Path | Behavior |
| --- | --- | --- |
| POST | `/api/logout` | Invalidate this browser session and clear its cookie; HTTP 200, same-origin required |
| POST | `/api/login` | Authenticate OCSTAT credentials, read device, issue session cookie; HTTP 200 on success |
| GET | `/api/session` | Current browser session snapshot or empty signed-out snapshot; no cloud calls |
| GET | `/api/health` | Process health and provider name, not device connectivity |
| GET | `/api/thermostat` | Current full snapshot, `Cache-Control: no-store`; OCSTAT requires cookie (401 otherwise) |
| PUT | `/api/thermostat/target` | Simulator: validate and queue desired target, HTTP 202. OCSTAT: HTTP 403 `read_only` |
| GET | `/api/events` | SSE `state` events; OCSTAT requires cookie (401 otherwise); revocation/expiry emits `signed_out` then closes |
| GET / PUT | `/api/simulator` | Read/change simulator scenario and latencies; HTTP 404 in OCSTAT mode |

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

## OCSTAT read-only snapshots

Snapshots add `can_set_target` and `read_error`. OCSTAT always advertises `can_set_target: false`; its `desired`, `command`, and `in_flight` remain null. `confirmed` contains the selected cloud device observation with `online`, integer-tenths room/target temperatures, unknown mode, a locally ordered read revision, and `applied_command: null`. This field name does not imply physical device confirmation. OCSTAT observed targets are not limited by simulator write range/steps.

`connectivity` is `connecting`, `online`, `offline`, or `unavailable`. The latter means the backend could not obtain usable status, not that the thermostat is known offline. `read_error` is null on a successful read or a sanitized enum such as `authentication`, `transport`, `invalid_response`, `device_not_found`, or `rejected`. Upstream messages, credentials, tokens and account/device identifiers are never included. Failed reads retain the previous observation and `last_sync`; successful reads update `last_sync` to the cloud retrieval time, not physical measurement time.

The health response reports the active provider and `can_set_target`; it does not claim cloud connectivity. Snapshot GET/Refresh never makes an extra upstream call. The worker owns all reads and polls OCSTAT every 30 seconds.

## OCSTAT browser login

There are no local accounts or separate credentials. Every process starts signed out. POST `/api/login` with the existing OCSTAT `email` and `password`. The route requires matching loopback Host/Origin, accepts at most 16 KiB and 1024 bytes per credential, and never echoes submitted values. The controller worker performs HTTPS login and the first device read. HTTP 200 means both succeeded; the body contains `login: "ready"`. Invalid credentials return 401 `authentication`; cloud failure 502, no device 404, and multiple devices 409. Failure never issues a session cookie.

Success sets a random 256-bit `smarttherm_session` cookie with Path=/, HttpOnly, SameSite=Strict and Max-Age=43200. Secure is omitted for the enforced loopback HTTP listener. This opaque identifier is distinct from the OCSTAT token and the public snapshot `session` revision identifier. It never appears in JSON. Passwords are transient; only the OCSTAT token/account/device and session mapping remain in memory. Browser refresh keeps the session; 12-hour expiry or server restart requires login again. No file or environment credential loading exists.

Each session owns independent provider/controller/worker state. `/api/thermostat` and `/api/events` reject missing/invalid cookies with 401. `/api/session` is the public bootstrap route: valid cookies return that browser's snapshot; otherwise it returns only signed-out state. All state and account responses use `Cache-Control: no-store`.

One login may run at a time, with a global 10-second cooldown and 32-session limit (429). A browser already signed in must log out before replacing its account (409). Authentication rejection during polling clears readings and displays login again; other read failures preserve the last observation. OCSTAT writes remain forbidden regardless of session state.

## Logout

POST `/api/logout` requires the same Host/Origin checks and uses the browser cookie. It invalidates only that session, clears the cookie, aborts its polling worker, and returns HTTP 200 with an empty `login: "required"` snapshot. Existing streams for that session emit `signed_out` and close. A late read cannot recreate the removed session. Missing/stale cookies can also be cleared safely.

Tabs sharing the cookie are signed out together; other browser sessions remain independent. No manufacturer logout request is sent and no cloud token revocation is claimed. Credential files are never read or written, including on restart.
