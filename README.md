# SmartThermWeb

A small, self-hosted thermostat dashboard with immediate temperature controls and honest command feedback. Rust/Axum serves a Svelte 5 interface, API, and SSE updates. The default provider connects to your OCSTAT account over HTTPS for status and temperature changes. A deterministic simulator is available as an explicit development/test option.

The OCSTAT provider supports HTTPS login, separate requested/reported targets, and authenticated temperature changes in Auto and temporary-override mode. Existing Apache 2.0 license retained.

## Run on macOS

Install the Xcode command-line tools (`xcode-select --install`), current stable Rust via [rustup](https://rustup.rs/), and Node.js **22.12 or newer** (Node 22 LTS works). Node is needed only for development/builds. GNU Make supplied with macOS is sufficient.

```sh
git clone https://github.com/Noot-tooN/SmartThermWeb.git
cd SmartThermWeb
make setup
make dev
```

Open the page and sign in with your OCSTAT email and password. Every server start begins signed out; credentials are never loaded from files or environment variables.

For this local checkout, start in `/Users/nirdala/Projects/SmartThermWeb` instead of cloning. Open **http://127.0.0.1:5173**. Rust listens at `127.0.0.1:3000`; Vite proxies `/api` and provides HMR. Ctrl-C stops both processes. Rust edits require restarting `make dev`; frontend edits update immediately.

For development without a cloud account, launch `SMARTTHERM_PROVIDER=simulator make dev`. In that mode, press `+` three times: 21.5 → 22.0 → 22.5 → 23.0°C appears immediately. After 350 ms, one request is sent. With default timings, cloud acknowledgement arrives after 900 ms, and a device observation confirms application after approximately 2.4 seconds plus polling delay. **HTTP 202 means queued, not applied.**

The default feedback has two persistent icons: **Server** and **Thermostat**. Only an explicit successful server response produces a server checkmark. The thermostat stays neutral and unverified, even when cloud temperatures match; these readings can precede the physical display. Server acceptance is retained in command snapshots across browser refreshes. A steady debounce glow fades into the sending pulse, and short queued cross-fades avoid snapping. New edits discard obsolete completion animations, errors bypass the queue, and reduced-motion mode shows the latest state directly. Hover, focus, or tap either icon for details. The main view shows the selected device, room temperature, target controls, and account. The info button beside the mode contains the last-read timestamp, connection details, and saved server target. Refresh sits beside the room reading and reloads the latest backend snapshot; it does not trigger an extra manufacturer poll. Healthy connection messages stay in the info panel, while failures remain visible.

The previous single icon and four-checkpoint strip remain available. To revert this presentation experiment, change the `progressStyle` default in `web/src/lib/Thermostat.svelte` from `'dual'` to `'icon'` or `'checkpoints'` and rebuild. The legacy presentations retain their older convergence wording; the dual presentation is the accurate default for the currently unverified physical synchronization. This switch does not change requests, debouncing, authentication, or the account header.

Expand **Try a slow connection or a failure**, select a scenario, and click **Apply simulator settings** before changing the target. Try “Reply lost, device still applies” to see an uncertain command reconcile successfully. “Server accepts, device never applies” demonstrates an unverified outcome. Switch “Device offline” back to “Normal operation” to reconnect; retry the desired target if necessary.

## One executable in production

```sh
make build
./target/release/smartthermweb
```

Open **http://127.0.0.1:3000**. `make build` installs locked frontend dependencies, compiles Svelte, then embeds every generated asset using rust-embed and builds the release binary. Copy that binary anywhere on the same OS/architecture; it needs no checkout, Node, web/dist directory, database, or separate server. Build for the destination platform before moving to another OS/architecture. For a Linux package, systemd service and HomeServer/Caddy example, see [NixOS integration](docs/nixos.md).

`cargo build` / `cargo test` work from a clean checkout without `web/dist`: the build script generates a small development landing page in Cargo's OUT_DIR. `make build` sets `SMARTTHERM_REQUIRE_WEB=1`, so the production build fails if the compiled frontend is missing. Always use `make build` for the complete application. After changing the frontend, rebuild the binary to update embedded assets.

## Checks

```sh
make setup
make test
make check
make build
python3 scripts/smoke.py
```

- `cargo test --locked`: temperatures, lifecycle validation, coalescing, stale observations/acknowledgements, transport uncertainty, timeouts, late reconciliation, offline/reconnect, API validation, duplicate/out-of-order browser requests, SSE lag recovery/reconnect, authenticated OCSTAT writes, strict status decoding, and conservative target matching.
- `cd web && npm test`: fake-timer debounce tests, immediate optimistic updates, serialized submissions, stale/duplicate events, server restarts, uncertain errors/retry, and rendered accessible feedback.
- `make check`: Rust formatting, Clippy with warnings denied, Svelte/TypeScript diagnostics, and Prettier formatting.

The current npm audit reports two low-severity development dependency entries (deepmerge and its Svelte Vite plugin parent) for an upstream prototype-poisoning advisory with no published fix. They are build tooling, not included as a Node runtime in the executable. The critical/moderate advisories in the original test-tool versions were removed by upgrading to Vitest 5 and Vite 8.

Lifecycle tests use paused Tokio time and Vitest fake timers, without wall-clock sleeps. See [architecture](docs/architecture.md), [API](docs/api.md), and [protocol evidence](docs/ocstat-protocol.md).

## Configuration

Normal launches use OCSTAT and show the browser login form. No automatic sign-in or credential-file support exists. `.env.example` documents non-secret shell settings; `.env` files are **not automatically loaded**.

```sh
SMARTTHERM_BIND=127.0.0.1:3001 ./target/release/smartthermweb
SMARTTHERM_PROVIDER=simulator SMARTTHERM_SIM_CONFIG='{"scenario":"reply_lost","ack_ms":900,"apply_ms":2400}' ./target/release/smartthermweb
```

The Vite development proxy expects backend port 3000. Simulator controls can change behavior at runtime. Reply delay is limited to 30,000 ms; application delay to 60,000 ms. Both delays start at dispatch. Scenario settings affect future commands; offline status also affects reads immediately. There is no automatic resend after a failure. Last confirmed readings remain visible while offline.

The executable enforces a loopback bind. OCSTAT readings require a browser session; the development simulator is unauthenticated. For remote hosting, put an HTTPS reverse proxy on the same server and set `SMARTTHERM_PUBLIC_ORIGIN` to the exact browser origin. Hosted mode requires OCSTAT, uses Secure cookies and rejects mismatched origins; see [NixOS integration](docs/nixos.md). The Android capture used plaintext HTTP, but HTTPS login, listing, and status reads were separately verified. Login, device status and temperature writes require verified HTTPS and block redirects. Schedule reads use the owner-authorized plaintext TCP service on port 8800; they expose the OCSTAT session token on that connection. There is no HTTP fallback for the other operations.

## OCSTAT temperature control

Open the page, enter your OCSTAT email and password, and click **Sign in**. Every server start begins signed out. The old credential file and `SMARTTHERM_OCSTAT_CONFIG_FILE` setting are ignored; the application does not read, create, or update credential files.

The browser posts credentials only to the same-origin backend, clears the password input immediately, and never writes credentials to local/session storage. The controller worker exchanges them for an OCSTAT token over HTTPS with certificate validation and no redirects or HTTP fallback. The password is a transient input to that request. The backend retains the token, account ID, selected device, and signed-in email in memory. Authenticated snapshots include only the email and thermostat display name for the account header; passwords, OCSTAT tokens, and manufacturer account/device IDs are never returned or logged. No password-based automatic retry or sign-in occurs.

Successful OCSTAT login creates a random session identifier in an HttpOnly, SameSite=Strict browser cookie. There are no SmartThermWeb accounts or separate passwords. Each browser session gets its own controller, OCSTAT token, and polling worker; another browser must sign in independently. Frontend JavaScript cannot read the cookie or OCSTAT token. The cookie survives refresh; sessions expire after 12 hours and are lost on server restart. The cookie uses Secure when `SMARTTHERM_PUBLIC_ORIGIN` is configured; local loopback HTTP development omits it.

Click **Log out** in the header to clear the browser cookie, invalidate that session, stop its polling, clear readings, and return to login. Tabs sharing that browser cookie are signed out together; independent browser sessions are unaffected. Logout does not revoke the manufacturer token or sign out the Android app.

The single-device UI discovers exactly one returned thermostat and pins it for subsequent reads. Multiple-device selection is not supported yet; ambiguous discovery fails rather than choosing a device arbitrarily. Listing uses the verified first-page parameters (`pageNum=1`, `pageSize=12`, `online=1`, `device_type=1`) without an Android client identifier. Pagination/filter semantics remain unverified.

After explicit login, the worker polls every 30 seconds after each read completes. Each HTTPS request has a 10-second timeout; the controller allows 35 seconds for login plus listing and status. Explicit authentication rejection returns to the login form. Browser login attempts are limited globally to one per 10 seconds, with one in progress at a time and at most 32 sessions. Failed login requests are never retried in the background; ordinary read transport failures preserve the last reading and retry with the existing token.

After sign-in, temperature controls are enabled when the thermostat is online, `program_mode` is 0, 1, or 2 (the three schedule groupings), and list/status agree on Auto (`0`) or temporary override (`1`). Each write rechecks that context and sends `work_mode=1`: adjusting from Auto starts a temporary override, while further adjustments update it, following the inspected Android code. Override expiry is not yet verified. The original physical round trip tested an existing override; the Auto-to-override path is covered by fixture tests but has not had a separate physical trial. Unsupported modes and failed reads disable controls; use the Android app for heating operating-mode changes. `PUT /api/thermostat/target` requires the same-origin browser session. Temperature controls do not use TCP 8800; schedule loading uses a separate read-only connection with a fresh client identifier.

The app permits 5–35°C in 0.5°C steps, matching the type-22 control screen in the inspected Android APK. This confirms the official client's bounds, not firmware enforcement or physical endpoint testing. Individual targets outside the live-tested 21.5–22.5°C round trip have not all been physically tested. Repeated edits coalesce, failures retain the desired target, and there is no automatic resend.

Each read combines the device list with a validated, device-matched status frame. The target display follows the user’s desired temperature while an adjustment is pending; saved server readings are available in the info panel. A failure in either request retains the previous complete observation. Target and room readings use exact integer tenths. Observed targets are not constrained to simulator range/step. Auto and temporary override are identified only when list/status mode fields agree; other contexts remain unavailable. These cloud-reported values have unverified device freshness; the backend never supplies an applied-command ID. Refresh reloads the backend snapshot without bypassing the cloud polling interval.

## Scope and limitations

- One simulated device, manual mode, 5–35°C range in 0.5°C increments. The range matches the inspected Android control; simulated mode behavior and timings are fixtures.
- Room temperature is a fixed 20.8°C fixture; this is a command/network simulator, not a thermal model.
- State, command IDs, and latest per-browser revision counters live in memory and reset on restart. A fresh server session ID lets the browser distinguish resets from stale updates. At most 256 browser client IDs are tracked per process; restart the local simulator if that limit is reached.
- In simulator mode, full snapshots keep the API simple. Only the latest user command and active command are retained, not an audit history. Polling is every 500 ms outside a write request. Writes wait up to 3 seconds for a server reply and then up to 6 seconds for application. Background reconciliation continues after that deadline.
- Simulator observations include reliable application correlation and monotonic revisions. OCSTAT may not offer either; do not fabricate these guarantees in a future adapter.
- Shared/remote access control, token persistence, Nix, and deployment infrastructure are deferred.

Before enabling real writes, obtain evidence linking reported state to physical device application and verify message IDs and mode/schedule effects. Writes require a separately approved experiment and implementation. Never commit raw captures or credentials.

## OCSTAT command feedback

A successful write response means cloud acceptance only. During a command, the worker reads status/list approximately every two seconds, allowing 30 seconds for reported convergence. Outside commands, polling remains every 30 seconds. A newer queued target takes priority after the active write returns (or its request timeout expires), without waiting for the old target to match. Observation sleeps wake on new work; an already active read finishes before the next write begins. Writes remain serialized and retain preflight validation. Two consecutive online observations with both requested and reported values matching the current attempt produce the internal `target_matched` phase. A mismatch, failed read, unsupported mode, or new attempt resets this evidence.

This is distinct from a device-origin command acknowledgement: `applied_command` stays null, `matched_command` identifies only the local attempt whose target matched, and the terminal phase is `target_matched`. Cached reads, other clients, and already-matching values cannot prove that a particular command physically executed. For OCSTAT, the two-icon UI keeps the thermostat unverified regardless of matching cloud values. The simulator retains its stronger `confirmed` phase.

Each attempt consumes a distinct four-hex-digit message ID within the provider session, even after cancellation/lost replies; these IDs do not guarantee idempotency or uniqueness across other clients. Duplicate local command IDs are refused, and no command is automatically resent. See [the successful authorized HTTPS trial](docs/capture-write-analysis-2026-11-01.md).


### Command feedback

The server checkmark means **Accepted by server**, never physical application. OCSTAT's thermostat icon remains neutral even when cloud values match. No automatic retry is triggered by missing physical confirmation.

The temporary sync recorder was removed and capture investigation paused on 2026-11-02. Historical findings remain in `docs/`; captures and APK files stay outside Git.

Verified device feedback can be added through the existing provider contract: `applied_command` carries device-origin evidence correlated to a local command, while `matched_command` means only cloud-value agreement. The controller validates application evidence before emitting `confirmed`; the frontend's `deviceApplicationVerified` projection drives the separate thermostat indicator. The simulator exercises this path today. OCSTAT must leave `applied_command` empty until a reliable protocol is established.

## Schedule work

Opening **Schedule** loads the server-stored active schedule for the signed-in device.
Edits stay local until **Save schedule**. Saving supports temperatures, time boundaries, adding/removing periods, and all three repeating patterns. The simulator keeps its sample draft editor. Choose a repeating pattern: one row for every day, two rows for weekdays/weekends,
or seven rows for individual days. All rows remain visible, each with subtle hour markers inside its timeline.
The shared draft actions stick to the viewport bottom while scrolling within the editor.
Select a block or drag a range on any row,
and set its temperature. Drag boundaries (30-minute snapping), use the arrow-key
controls, or open the time label for exact 10-minute times. Adjacent equal temperatures merge; edits
that exceed six changes are rejected without altering the draft. Undo reverses
each completed edit across rows. Discard changes restores the loaded schedule and its pattern (or resets the current simulator draft);
Save checks the loaded baseline against a fresh read and verifies each changed group before continuing. A hover marker shows the snapped time, and pending selections
have draggable start/end handles before applying. Temperature controls appear in
a compact popover attached to the bar. Rust validation runs automatically after
edits; redundant midnight anchors do not consume a change.
Rust validates the draft and computes overnight periods. Drafts remain in page
memory while switching patterns; leaving, refreshing, or signing out clears them.
Previewing never changes the thermostat. Only the explicit Save action sends schedule writes.

Pattern selection uses a keyboard-accessible segmented control. **Choose a preset**
shows Winter day, Morning & evening, and Summer examples with their exact hours and
temperatures before selection. Applying one replaces only the selected day/group
and can be undone in one step. Summer is a constant lower heating target, not Off.
Preset templates are copied into the draft so later edits cannot change the template.
**Choose custom hours** opens the exact-time editor for keyboard/touch range entry.

The APK's `getProgrammeByMode` endpoint returned 404 over both HTTPS and HTTP.
Following explicit owner approval on 2026-11-02, schedule loading uses a bounded
plaintext TCP connection, fresh client identifier, and read selectors only. The
controller serializes it with other provider operations. Identity, mode, six-slot
structure, time/temperature ranges and complete groups are checked; HTTPS status
reads bracket the fetch to reject a changed mode or offline device. These checks
do not prove an atomic snapshot or physical application. Reload is manual and
disabled while edits exist. Mixed overnight temperatures within a grouped row
are rejected rather than flattened incorrectly. Existing-period temperature writes passed the controlled server round trip. See [the feature backlog](docs/feature-backlog.md) and
[preview API](docs/api.md#schedule-draft-preview).

Schedule stays expanded and loads after sign-in. A reduced-motion-aware skeleton
reserves the two-row layout during the initial fetch; reloads retain existing
content. The selector marks the mode last read from the server independently of
the selected draft. Changeover times share one line, with guide lines when labels
need spacing. Narrow periods expose exact times and temperatures in their editor.

### Controlled web schedule-save trial

The APK-derived investigation trial is opt-in and
limited to an existing weekend period in 5+2 mode: +0.5°C, then explicit
restoration. No time, mode, or disabled-slot changes are supported by the trial.

Set `SMARTTHERM_SCHEDULE_TRIAL_DIR` to an absolute private directory (0700) when
starting the OCSTAT server. Visit `/schedule-trial` after signing in through the
normal app. Prepare performs reads only and shows the exact proposed change.
Apply requires confirmation of the temporary +0.5°C change, which may affect
active heating; Restore is separate.
All calls use the existing browser session and controller worker. Writes use
HTTPS `setProgrammeByDay`, with no HTTP fallback; read-back uses the approved TCP
schedule reader. The HTTP reply alone never marks a trial successful.

The directory holds a 0600 `schedule-trial.json` recovery backup with device/account
identity and the original six slots, but no password or token. Keep it private,
outside Git, and retain it until restoration is verified. Restart with the same
directory and sign in to the same account to resume Check/Restore. Only one trial
can be active per server; no automatic retries or rollbacks run. An uncertain
Apply/Restore must be checked rather than resent. An external schedule edit can
block restoration to avoid overwriting it. Preflight comparisons reduce but cannot
eliminate upstream races. Server read-back does not confirm physical execution.

### Verified schedule saves

The owner completed the weekend 07:00 trial, 21.5 → 22.0 → 21.5°C, and a
subsequent read confirmed restoration of all six original weekend slots with
the existing weekday/weekend mode. This establishes server storage, not physical
thermostat application. Weekday writes use the same inspected six-slot endpoint
and mapping (day 8); the live trial used weekends (day 9).

Normal Save maps the validated draft into six manufacturer slots per group.
Removing a period disables its old slot; adding a period reuses a disabled slot.
Unused slots retain their data. Pattern/day codes stay inside Rust.
It compares a fresh schedule with the loaded baseline, sends changed groups
serially over HTTPS, and requires exact slot readback. A conflict or uncertain
outcome stops remaining groups. There is no automatic retry or rollback.

The editor retains its draft after failures. **Check server** reads only; a full
match clears the draft as **Accepted by server**. Otherwise, **Use server schedule** explicitly discards the draft and adopts a fresh
server read, clearing the unresolved local save. It sends no thermostat commands
and does not cancel an update that might still arrive. Failed recovery reads
leave the draft and pending lock intact. Partial acceptance names
the groups already verified. Normal pending-write tracking survives canceled
requests and browser logout within the running server, but is memory-only; server
restarts never resume writes. After a restart, inspect the freshly loaded schedule
before editing again. Unlike the restricted trial, normal saves do not have a
persistent recovery backup. External-client races cannot be eliminated because
OCSTAT offers no verified conditional-write or atomic whole-week operation.

### Full schedule editing

Save now supports every day, weekdays/weekends, and individual days, including
new/deleted periods and 10-minute boundary edits (dragging still snaps to 30 min).
The APK mapping uses six slots, disables deleted start times with +2880, reuses
inactive slots for new starts, and sorts the resulting array before submission.
These paths have local transport/encoder tests; the earlier real server trial
verified an existing weekend temperature edit, not a full grouping round trip.

Grouping changes use HTTPS `setProgrammeModeNew`, with a distinct message ID,
then require a complete read of the new grouping before sending period updates.
The previously stored schedule for that grouping can become active immediately;
the Save area explains this when grouping differs from the observed server mode.
This is not an atomic switch of the entire week. Failed/unverified steps stop
remaining operations and never trigger an automatic retry or rollback.
The active badge follows the latest observed server grouping even after partial
failure. Newly selected groupings start from the visible draft; a merged group
uses its first day (Monday for every day/weekdays, Saturday for weekends), shown
before Save. Previously edited alternate drafts remain available while editing.

Target controls remain available after grouping changes in Auto/temporary override. The APK temperature action depends on heating work mode, not schedule grouping; unknown groupings or disagreeing work/status modes still fail closed.
