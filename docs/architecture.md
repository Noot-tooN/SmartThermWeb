# Architecture and lifecycle

A single Rust crate has four responsibilities: domain types, providers, the controller, and HTTP/static serving. Svelte owns browser-local optimistic state. No database or background infrastructure is needed.

## State ownership

The controller stores the latest provider observation separately from the desired target. The snapshot calls it `confirmed`, but OCSTAT observations are cloud status, not physical confirmation. It publishes a monotonically revisioned snapshot on every meaningful transition and successful poll. A process session ID scopes revisions across restarts. Last successful sync is a Unix timestamp for display only; ordering never depends on it.

Each command gets a monotonically increasing ID. One worker serializes all provider reads and writes. While it is busy, requests replace a single pending slot. A stale acknowledgement may update the visible in-flight command, but cannot update the latest desired command. The pending latest command starts when the active write returns or reaches its existing request timeout; it does not wait for obsolete target observations. New pending work wakes the observation sleep immediately. A read already in progress finishes before the next write starts, preserving serialized provider access. The new command still performs its normal preflight checks. This is per-device serialization for the single-device MVP.

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

Within a server session, old/duplicate snapshot revisions are ignored. SSE can introduce a replacement snapshot session; explicit login/logout and session refresh can also establish the current browser session. An authentication generation discards responses and streams started before a session change; retired stream sessions cannot roll the browser back. Optimistic edits survive unrelated state updates and failed submissions. Retry is explicit. Browser transport errors are conservative: the target stays visible and the user can refresh/retry; no automatic write retry occurs.

Command feedback defaults to two persistent icons. The server icon has a presentation-only queue; the thermostat icon remains neutral and unverified. `StatusIconSequence` waits for each 220 ms opacity transition to finish (with a 300 ms fallback if the browser does not emit a completion event); persistent SVG layers cross-fade and scale slightly without remounts. Rapid forward updates queue, while a new local edit, error, lost evidence, or session change discards obsolete queued completion. The intent key includes session, authentication generation, local edit revision (including same-value retries), and target. Actual temperatures, accessible status, explanations and retry availability never wait for the queue. Reduced-motion preference disables queuing and motion.

The backend retains explicit acceptance in `Command.server_accepted` across later phases and browser refreshes. The dashboard also records whether the current command's cloud acknowledgement was actually observed before a later snapshot replaces it. This preserves evidence across batched rendering and avoids inventing cloud acceptance for a late device confirmation after a lost reply. The visual queue does not fabricate device confirmation or alter backend state. Initial loads show the current status without replaying a historical command.

The legacy single icon and four-checkpoint components and their tests remain available. Set the `progressStyle` default in `Thermostat.svelte` to `'icon'` or `'checkpoints'` to restore them. The default `'dual'` presentation separates explicit server acceptance from verified device application and never promotes cloud target equality to physical synchronization. Icon details overlay the card and retry occupies reserved space, so transitions do not reflow the layout.

The EventSource reconnects automatically. Every connection begins with a current full snapshot; broadcast lag also produces a full snapshot. Subscribing before taking the initial snapshot closes the subscription race. The event ID combines server session and snapshot revision. No replay log or Last-Event-ID history is necessary because each event is self-contained.

## Deployment and safety boundaries

Assets are copied into OUT_DIR at compile time and embedded in debug/release builds. Cargo-only development builds use a generated landing page when frontend assets do not exist; the production make target requires them. Unknown API routes return JSON 404 rather than HTML.

OCSTAT can log in and read the device list and status over a fixed HTTPS origin with certificate verification and no redirects. Every process starts signed out. The worker uses browser-submitted credentials for one HTTPS exchange, then retains the token, selected device, and signed-in email in memory. One serialized worker per browser session polls every 30 seconds. Authentication rejection stops polling and returns to browser login. No automatic login or password retry exists. The fixed HTTPS transport also supports the verified target endpoint; no TCP 8800 transport exists.

OCSTAT capability is evaluated per browser session after a successful online list/status read. The snapshot enables controls only in the tested raw mode context; each write performs its own preflight and preserves that work mode. The target API authenticates the session and checks Host/Origin before enqueueing. Failed reads disable controls and preserve prior observations and desired intent. New successful reads can restore capability. The anonymous controller cannot write.

OCSTAT observation revisions order local reads, not device changes. The provider tracks the latest attempted command and counts consecutive reads where both target fields match it; failures/mismatches reset the count. After two matches it returns `matched_command`, never `applied_command`. The controller transitions only that command to `TargetMatched`. This separate terminal phase describes current cloud-reported agreement, without claiming a physical command acknowledgement. Offline, stale, duplicate, or superseded evidence cannot complete a new command.

Idle reads follow Android-side target changes when there is no pending local intent. Active commands retain the user's desired target until completion or explicit retry. The single worker serializes writes, coalesces pending requests, polls at approximately two-second intervals for up to 30 seconds during reconciliation, and returns to 30-second idle polling afterward.

The binary rejects non-loopback bind addresses. OCSTAT data requires a browser session; the explicit simulator remains unauthenticated. Remote hosting uses a same-machine HTTPS reverse proxy with an explicitly configured `SMARTTHERM_PUBLIC_ORIGIN`; the NixOS module provides an opt-in service. No secrets, captures, telemetry, or remote fonts are included in the build.

## Browser sessions backed by OCSTAT

There are no local accounts or separate passwords. The browser submits the existing OCSTAT email/password; a new per-session controller worker verifies them over HTTPS and reads the uniquely discovered device. Only success creates an opaque 256-bit session cookie. The browser keeps it as HttpOnly, SameSite=Strict; frontend JavaScript never sees it or the OCSTAT token. The server retains the upstream token/account/device and signed-in email in memory, with no password retention, file loading, or automatic sign-in. Authenticated snapshots expose only the display email and device name, never manufacturer identifiers or tokens.

A bounded in-memory session map selects each browser's independent controller for snapshots and SSE. Login is globally serialized/rate-limited, credentials and errors are sanitized, and every account action checks matching Host/Origin against either the local loopback policy or the exact configured HTTPS origin. Sessions last 12 hours; refresh preserves the browser cookie, but restart loses the map and requires explicit login. Hosted mode uses Secure cookies; the local development policy omits Secure for loopback HTTP.

Logout removes the mapping before returning, clears the cookie and readings, signals active streams to close, and aborts that session's worker. Late cloud replies cannot restore access. Expiry stops the worker and closes its streams. Failed/canceled login tasks drop their worker instead of leaving detached polling. Tabs sharing a cookie share logout; independent browser sessions are unaffected. No manufacturer revocation or logout endpoint is assumed. Temperature requests require the authenticated session and are revalidated against live device context before dispatch.

The debounce indicator has its own boolean, distinct from pending HTTP delivery. A persistent soft glow stays steady through repeated clicks; only the 350 ms submission timer resets. When the timer ends, the glow and Updating pulse cross-fade over 180 ms. The hidden pulse pauses in place, and all visual layers remain mounted so new edits can reverse the fade without snapping. Session changes, read-only state, and disposal cancel the glow; a completed debounce does not imply a sent or accepted command. Reduced motion disables both fades and breathing.

## OCSTAT transport and request behavior

Each read combines list metadata with a strict, device-matched status frame. The UI follows reported targets separately from requested targets. Malformed or mismatched status rejects the combined observation. Mode mapping and override expiry remain unverified; no mode, schedule, or override-cancellation operation is exposed.

The successful authorized HTTPS round trip verified the six-field form without `userid_phoneid`. Production uses that same form. Application bounds are 5–35°C in 0.5°C steps, matching the inspected Android control screen. Unknown or disagreeing mode contexts fail closed; Auto and temporary-override adjustments send work mode 1, following Android. A new message ID is consumed before each attempt, including attempts with canceled/lost responses. There are no automatic retries or idempotency assumptions. Authorization failures discard upstream session state; logout clears queued work and aborts the browser's worker.

## Device confirmation extension point

The capture investigation is paused; the temporary browser recorder and its collection hooks were removed on 2026-11-02. Historical investigation documents describe experiments, not active app features.

The server indicator says **Accepted by server** only for explicit acceptance. `Provider::read` may return `DeviceState.applied_command` only with device-origin application evidence correlated to that local command. `matched_command` is deliberately weaker and never enables a thermostat checkmark. Keep provider observation revisions monotonic and preserve the controller's fresh/online/target/correlation checks. A future asynchronous device listener should feed observations through the serialized controller/provider boundary, not update the frontend independently.

`deviceApplicationVerified` in `web/src/lib/connection-status.ts` is the UI boundary: a confirmed command must match the latest displayed target and applied-command ID, with an online reading and no read error. The component additionally suppresses confirmation during local edits, request errors, or stream disconnection. The simulator supplies real simulated application evidence to exercise this route; OCSTAT still returns no applied-command evidence. No new transport or speculative acknowledgement is introduced. When implementing a real signal, test delayed, superseded, offline and lost-reply cases; physical confirmation must remain independent of server acceptance.

A later verified application may promote `TargetMatched` to `Confirmed`, subject to the same freshness, online, target and command-ID checks. Matching cloud values alone never perform that transition.
