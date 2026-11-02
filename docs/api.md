# HTTP API

Same origin, JSON. All temperatures are integer tenths of °C. Errors use `{"error":{"code":"...","message":"..."}}`. The local build is restricted to loopback. OCSTAT device data requires a browser session; the explicit simulator mode is unauthenticated.

| Method | Path | Behavior |
| --- | --- | --- |
| POST | `/api/logout` | Invalidate this browser session and clear its cookie; HTTP 200, same-origin required |
| POST | `/api/login` | Authenticate OCSTAT credentials, read device, issue session cookie; HTTP 200 on success |
| GET | `/api/session` | Current browser session snapshot or empty signed-out snapshot; no cloud calls |
| GET | `/api/health` | Process health and provider name, not device connectivity |
| GET | `/api/thermostat` | Current full snapshot, `Cache-Control: no-store`; OCSTAT requires cookie (401 otherwise) |
| PUT | `/api/thermostat/target` | Validate and queue desired target, HTTP 202; OCSTAT requires same-origin authenticated session and currently writable device context |
| GET | `/api/events` | SSE `state` events; OCSTAT requires cookie (401 otherwise); revocation/expiry emits `signed_out` then closes |
| GET / PUT | `/api/simulator` | Read/change simulator scenario and latencies; HTTP 404 in OCSTAT mode |

## Temperature request

```json
{"target_temperature":230,"client_id":"random-browser-session","client_revision":3}
```

Targets must be 50–350 inclusive and divisible by 5. `client_id` and `client_revision` may both be omitted for scripts. If supplied, the ID must be 1–80 bytes and revision positive. A duplicate or older revision from that browser returns 409 and cannot alter the desired target. The client registry is bounded at 256 identities; a new identity beyond this limit receives 429. Request validation errors return 400, unknown API routes 404, and unsupported methods 405.

A 202 response contains a complete snapshot, including `session`, `revision`, `connectivity`, `last_sync`, `confirmed`, `desired`, `command`, and `in_flight`. The latest command may be queued while an older command is in flight. Before the first successful device read, confirmed state, desired target, and last_sync may be null. Without a command, command/in_flight are null. The confirmed state contains room and target temperatures, mode, provider revision, and optional applied-command evidence. `last_sync` is Unix seconds.

## SSE

Events have `event: state`, `id: <session>:<revision>`, and JSON data. Clients should replace their server snapshot atomically and ignore revisions no newer than the one already held for that session. Reconnect always sends the current full snapshot; there is no history replay even if Last-Event-ID is supplied. SSE keepalives keep the connection active. A disconnected stream does not itself prove the physical device is offline; the UI shows those connections separately.

## Simulator

```json
{"scenario":"reply_lost","ack_ms":900,"apply_ms":2400}
```

Scenarios: `normal`, `rejected`, `transport_failure`, `reply_lost`, `ack_timeout`, `device_timeout`, `offline`. Reply delay is 0–30000 ms; application delay 0–60000 ms. The current config is returned after a successful PUT. Rejected/transport_failure/device_timeout/offline never schedule a new device application. Reply_lost and ack_timeout do schedule application, even though the server reply is unavailable. Returning from offline allows pending scheduled applications to be observed.

## OCSTAT snapshots and writable context

`can_set_target` is true only for an authenticated OCSTAT session with a successful online read in the supported mode context. Signed-out, failed-read, offline, and unsupported-mode states advertise false. An accepted target request creates `desired`, `command`, and `in_flight`; local submissions never directly call the manufacturer. Unknown/stale browser revisions are rejected as before.

`confirmed` contains the selected cloud observation with room temperature, cloud-observed `target_temperature`, cloud `requested_temperature`, observed mode, and a locally ordered read revision. `target_writable` indicates support for the observed mode/online context. `applied_command` remains null for OCSTAT. `matched_command`, when present, identifies the local attempt whose requested and reported temperatures matched on two consecutive online reads, not a device-issued command acknowledgement. The controller ends that attempt in `target_matched`; the thermostat icon remains neutral and unverified. The simulator still uses `confirmed` with `applied_command`.

Neither local revision nor temperature equality proves device-origin freshness. The `confirmed` container name is retained for compatibility and must not be interpreted as that guarantee.

`connectivity` is `connecting`, `online`, `offline`, or `unavailable`. The latter means the backend could not obtain usable status, not that the thermostat is known offline. `read_error` is null on a successful read or a sanitized enum such as `authentication`, `transport`, `invalid_response`, `device_not_found`, or `rejected`. Authenticated snapshots include `account_email` and `confirmed.device_name` for display. The email is populated only after successful login; logout and authentication rejection clear it and the device observation. Device names are optional, trimmed, limited to 120 characters and stripped of control characters. Passwords, tokens, upstream messages, and manufacturer account/device IDs are never included. Failed reads retain the previous observation and `last_sync`; successful reads update `last_sync` to the cloud retrieval time, not physical measurement time.

The health response reports the active provider and `can_set_target`; it does not claim cloud connectivity. Snapshot GET/Refresh never makes an extra upstream call. The worker owns all reads and polls OCSTAT every 30 seconds while idle and approximately every two seconds during command reconciliation (up to 30 seconds).

## OCSTAT browser login

There are no local accounts or separate credentials. Every process starts signed out. POST `/api/login` with the existing OCSTAT `email` and `password`. The route requires matching loopback Host/Origin, accepts at most 16 KiB and 1024 bytes per credential, and never echoes submitted values in errors. After successful OCSTAT authentication, the signed-in email is returned as display metadata. The controller worker performs HTTPS login and the first device read. HTTP 200 means both succeeded; the body contains `login: "ready"`. Invalid credentials return 401 `authentication`; cloud failure 502, no device 404, and multiple devices 409. Failure never issues a session cookie.

Success sets a random 256-bit `smarttherm_session` cookie with Path=/, HttpOnly, SameSite=Strict and Max-Age=43200. Secure is omitted for the enforced loopback HTTP listener. This opaque identifier is distinct from the OCSTAT token and the public snapshot `session` revision identifier. It never appears in JSON. Passwords are transient; only the OCSTAT token/account/device and session mapping remain in memory. Browser refresh keeps the session; 12-hour expiry or server restart requires login again. No file or environment credential loading exists.

Each session owns independent provider/controller/worker state. `/api/thermostat` and `/api/events` reject missing/invalid cookies with 401. `/api/session` is the public bootstrap route: valid cookies return that browser's snapshot; otherwise it returns only signed-out state. All state and account responses use `Cache-Control: no-store`.

One login may run at a time, with a global 10-second cooldown and 32-session limit (429). A browser already signed in must log out before replacing its account (409). Authentication rejection during polling clears readings and displays login again; other read failures preserve the last observation. OCSTAT writes require a valid session, matching loopback Host/Origin, and current writable device state. Signing out immediately disables controls and cancels pending work; a request already sent to the manufacturer cannot be undone by logout.

## Logout

POST `/api/logout` requires the same Host/Origin checks and uses the browser cookie. It invalidates only that session, clears the cookie, aborts its polling worker, and returns HTTP 200 with an empty `login: "required"` snapshot. Existing streams for that session emit `signed_out` and close. A late read cannot recreate the removed session. Missing/stale cookies can also be cleared safely.

Tabs sharing the cookie are signed out together; other browser sessions remain independent. No manufacturer logout request is sent and no cloud token revocation is claimed. Credential files are never read or written, including on restart.

## Requested versus reported OCSTAT targets

`confirmed.target_temperature` now comes from the candidate device-reported field of the validated `getDeviceStatus` frame (`L[36:38]`, hexadecimal half-degrees converted to integer tenths). `confirmed.requested_temperature` comes from the separate requested field (`L[19:21]`). For example, a request to restore 21.5°C can produce `requested_temperature: 215` and `target_temperature: 225` until the reported value catches up. The simulator returns `requested_temperature: null`; its user intent remains in `desired`.

Status/list reads are sequential, not atomic. A malformed frame, device mismatch, or failed request rejects the combined observation rather than publishing a partial device-list target as confirmation. Neither a fresh local revision nor equality of temperatures establishes device-origin freshness or command correlation. `applied_command` remains null for OCSTAT. Two consecutive matching reads can produce `matched_command` and `target_matched` without claiming device-origin acknowledgement. The existing target route is enabled only for the authenticated session and writable context.

The process-wide `/api/health` response uses the anonymous controller, so its OCSTAT `can_set_target` stays false. Use the authenticated `/api/session` snapshot for the current browser's capability. Target writes are never dispatched through that anonymous controller.


`command.server_accepted` records an explicit successful provider response for that command and survives subsequent phases. It defaults to false when absent. A matching target after a lost reply never sets this flag. The default frontend uses this evidence for the server checkmark, including on refresh; no OCSTAT observation currently produces a thermostat checkmark.

## Application evidence

`command.server_accepted` backs **Accepted by server** and is independent of physical application. `confirmed.applied_command` is reserved for verified device-origin application correlated to a local command; `confirmed.matched_command` only describes matching cloud observations. Only the controller's `confirmed` phase with matching application evidence may enable the separate thermostat confirmation indicator. OCSTAT currently leaves `applied_command` null; the simulator exercises the stronger path. The historical `confirmed` snapshot property name alone does not certify a physical reading.


### Auto and temporary override

OCSTAT reports `mode.kind: schedule` for matching work/status mode `0`, or `temporary_override` for matching mode `1`, with `program_mode=1`. Other or disagreeing contexts remain `unknown` and cannot write. Labels describe cloud observations. An online target change in either supported mode sends `work_mode=1`, following Android's Auto-to-temporary-override behavior. The local target API is unchanged; the provider decides from a fresh preflight, never a client-supplied mode. Scheduled readings do not count as a matching temporary-override command. No device application evidence is created.

## Schedule draft preview

`POST /api/thermostat/schedule/preview` validates and expands a draft; it does
not call a provider, persist a draft, or change any thermostat state. Real mode
requires a signed-in browser session and same origin. Responses use `no-store`.

```json
{"pattern":"daily","groups":[{"group":"every_day","changes":[{"start_minute":420,"target_temperature":215},{"start_minute":1410,"target_temperature":185}]}]}
```

Patterns: `daily` (`every_day`); `weekday_weekend` (`weekdays`, `weekends`);
`weekly` (`monday` through `sunday`). Each group requires 1–6 unique changes after midnight normalization,
in 10-minute increments within a day. Temperatures use integer tenths, 50–350
in steps of 5. Rust sorts and validates groups/changes and returns `schedule`,
seven `days` with contiguous `periods` covering 00:00–24:00, and `can_save: false`.
Periods before the first change inherit the preceding actual day's last target.
A redundant 00:00 anchor is removed only if it matches the last temperature of
every preceding day that feeds that group. A seven-row editor draft is accepted
only when that reduces to six actual changes. Real midnight changes are retained.
No timezone/current-time assumptions are made. There is no schedule write route.

The manufacturer adapter separately maps to APK programme mode/day codes and
active `[minute, temperature]` arrays. This is not a complete send-ready payload:
disabled-slot padding/preservation and successful response semantics still need
verification. No manufacturer codes, tokens, or device identifiers reach the UI.
