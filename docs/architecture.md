# Architecture and lifecycle

A single Rust crate has four responsibilities: domain types, providers, the controller, and HTTP/static serving. Svelte owns browser-local optimistic state. No database or background infrastructure is needed.

## State ownership

The controller stores the latest confirmed device observation separately from the desired target. It publishes a monotonically revisioned snapshot on every meaningful transition and successful poll. A process session ID scopes revisions across restarts. Last successful sync is a Unix timestamp for display only; ordering never depends on it.

Each command gets a monotonically increasing ID. One worker serializes all provider reads and writes. While it is busy, requests replace a single pending slot. A stale acknowledgement may update the visible in-flight command, but cannot update the latest desired command. The pending latest command starts after the active command resolves or its observation window expires. This is per-device serialization for the single-device MVP.

```text
Queued → Sending → AcceptedByCloud → AwaitingDevice → Confirmed
             ↘ Rejected                    ↘ Unknown
             ↘ TransportFailed                 ↘ Confirmed (late evidence)
             ↘ TimedOut → Confirmed (reconciliation)
```

TransportFailed and TimedOut mean an unreliable or missing reply, not a proven failed thermostat operation. Rejected means a definite provider rejection. Unknown means cloud acceptance without sufficient device evidence before the observation deadline. Late observations can confirm uncertain commands. Rejected/queued commands cannot be confirmed, and sending does not transition directly to confirmed.

Cloud acceptance is published separately before awaiting-device. The browser may render only the latter if events arrive together, but its text still states server acknowledgement. The SSE stream exposes both transitions for clients that need them.

## Observation evidence

An observation older than the last provider revision is discarded. Confirmation requires both the desired value and an applied command ID matching the command under reconciliation. A coincidentally matching temperature is insufficient. Reconciliation can update actual confirmed state without replacing the desired value. A new provider must supply real evidence for these fields, or leave application correlation absent and keep the command unverified.

The simulator schedules side effects before awaiting its artificial cloud response. Canceling/timing out that future does not remove the side effect. Its simulated device discards older applications after newer ones, a deliberately documented simulator guarantee. Real OCSTAT ordering and mode/schedule interactions remain unknown.

## Browser behavior

Each button click updates a local desired value synchronously. A 350 ms debounce combines clicks. Browser submissions are serialized as well; a later edit remains visible through an earlier HTTP reply. An opaque browser ID and increasing edit revision let the server reject delayed/duplicate HTTP commands from that browser even if transport times out. Unversioned API clients use arrival order. Multiple browsers use server arrival order; there is no cross-browser edit locking.

Within a server session, old/duplicate snapshot revisions are ignored. Only the SSE connection can introduce a new session once one has been established; retired session responses cannot roll the browser back. Optimistic edits survive unrelated state updates and failed submissions. Retry is explicit. Browser transport errors are conservative: the target stays visible and the user can refresh/retry; no automatic write retry occurs.

Command feedback uses one persistent four-checkpoint strip. The presentation collapses AcceptedByCloud/AwaitingDevice into the server-acknowledged milestone without changing the backend lifecycle. New local edits take precedence over older snapshots. Local selection is immediately complete, so debounce, queueing, and sending all map to the Updating checkpoint. Further clicks during Updating keep the same fill destination and breathing animation; an edit after acknowledgement or confirmation retreats only to Updating. The first checkpoint is reserved for initial state acquisition. Full progress requires matching confirmed state, a connected event stream, and an online device. Delivery errors and disconnections cannot leave a misleading full bar. CSS transitions retarget the existing fill from its current visual position in both directions; no keyed remounts, simulated percentages, or extra timers drive progress. Checkpoint details overlay the layout and retry occupies reserved space, so state changes do not reflow the card.

The EventSource reconnects automatically. Every connection begins with a current full snapshot; broadcast lag also produces a full snapshot. Subscribing before taking the initial snapshot closes the subscription race. The event ID combines server session and snapshot revision. No replay log or Last-Event-ID history is necessary because each event is self-contained.

## Deployment and safety boundaries

Assets are copied into OUT_DIR at compile time and embedded in debug/release builds. Cargo-only development builds use a generated landing page when frontend assets do not exist; the production make target requires them. Unknown API routes return JSON 404 rather than HTML.

OCSTAT can log in and read the device list over a fixed HTTPS origin with certificate verification and no redirects. Its private server-side credential file is loaded at startup; tokens live only in memory. One serialized worker polls every 30 seconds. Authentication rejection stops further authenticated requests until restart; transient login attempts are limited to once a minute. No write endpoint or TCP 8800 transport exists.

The snapshot advertises `can_set_target: false` for OCSTAT. The API rejects target writes, the controller refuses to queue them, and the provider always rejects `send_target`. Read-only snapshots have no desired target or command; the UI follows the latest observed target. Cloud failures preserve the last reading and expose only a typed sanitized error. `unavailable` means a read failed, distinct from a successful cloud report of device `offline`.

For OCSTAT, observation revisions order local completed reads, not device changes, and `applied_command` is always absent. Observed target temperatures use unrestricted integer tenths rather than simulator write validation. Unknown mode values are not mapped speculatively. The UI labels cloud observations explicitly and does not show a full device-confirmation bar.

The binary rejects non-loopback bind addresses. Browser authentication and secure remote access remain deferred, so do not tunnel or proxy this unauthenticated service to other users. No secrets, captures, telemetry, or remote fonts are included in the build.
