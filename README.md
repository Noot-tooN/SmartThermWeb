# SmartThermWeb

A small, self-hosted thermostat dashboard with immediate temperature controls and honest command feedback. Rust/Axum serves a Svelte 5 interface, API, and SSE updates. The default provider is a deterministic simulator; **no cloud account or thermostat is required**.

The real uniTEC SQ10net RF / OCSTAT integration is an isolated, fail-closed scaffold. No real-device network calls or writes are implemented. Existing Apache 2.0 license retained.

## Run on macOS

Install the Xcode command-line tools (`xcode-select --install`), current stable Rust via [rustup](https://rustup.rs/), and Node.js **22.12 or newer** (Node 22 LTS works). Node is needed only for development/builds. GNU Make supplied with macOS is sufficient.

```sh
git clone https://github.com/Noot-tooN/SmartThermWeb.git
cd SmartThermWeb
make setup
make dev
```

For this local checkout, start in `/Users/nirdala/Projects/SmartThermWeb` instead of cloning. Open **http://127.0.0.1:5173**. Rust listens at `127.0.0.1:3000`; Vite proxies `/api` and provides HMR. Ctrl-C stops both processes. Rust edits require restarting `make dev`; frontend edits update immediately.

Press `+` three times: 21.5 → 22.0 → 22.5 → 23.0°C appears immediately. After 350 ms, one request is sent. With default timings, cloud acknowledgement arrives after 900 ms, and a device observation confirms application after approximately 2.4 seconds plus polling delay. **HTTP 202 means queued, not applied.**

The feedback strip has four icon checkpoints: target selection, updating, server acknowledgement, and device synchronization. The bar fills only after device confirmation. Selection completes immediately. Debouncing, queueing, and sending share one Updating checkpoint, so repeated clicks continue the same animation. A newer edit after acknowledgement or synchronization smoothly retreats only to Updating; older acknowledgements never advance the latest edit. Its height stays fixed. Hover, focus, or tap a checkpoint for details; uncertain outcomes mark that checkpoint and reveal an icon button to retry. Reduced-motion preferences are respected.

Expand **Try a slow connection or a failure**, select a scenario, and click **Apply simulator settings** before changing the target. Try “Reply lost, device still applies” to see an uncertain command reconcile successfully. “Server accepts, device never applies” demonstrates an unverified outcome. Switch “Device offline” back to “Normal operation” to reconnect; retry the desired target if necessary.

## One executable in production

```sh
make build
./target/release/smartthermweb
```

Open **http://127.0.0.1:3000**. `make build` installs locked frontend dependencies, compiles Svelte, then embeds every generated asset using rust-embed and builds the release binary. Copy that binary anywhere on the same OS/architecture; it needs no checkout, Node, web/dist directory, database, or separate server. Build for the destination platform before moving to another OS/architecture. NixOS packaging is intentionally deferred.

`cargo build` / `cargo test` work from a clean checkout without `web/dist`: the build script generates a small development landing page in Cargo's OUT_DIR. `make build` sets `SMARTTHERM_REQUIRE_WEB=1`, so the production build fails if the compiled frontend is missing. Always use `make build` for the complete application. After changing the frontend, rebuild the binary to update embedded assets.

## Checks

```sh
make setup
make test
make check
make build
python3 scripts/smoke.py
```

- `cargo test --locked`: temperatures, lifecycle validation, coalescing, stale observations/acknowledgements, transport uncertainty, timeouts, late reconciliation, offline/reconnect, API validation, duplicate/out-of-order browser requests, SSE lag recovery/reconnect, and disabled OCSTAT writes.
- `cd web && npm test`: fake-timer debounce tests, immediate optimistic updates, serialized submissions, stale/duplicate events, server restarts, uncertain errors/retry, and rendered accessible feedback.
- `make check`: Rust formatting, Clippy with warnings denied, Svelte/TypeScript diagnostics, and Prettier formatting.

The current npm audit reports two low-severity development dependency entries (deepmerge and its Svelte Vite plugin parent) for an upstream prototype-poisoning advisory with no published fix. They are build tooling, not included as a Node runtime in the executable. The critical/moderate advisories in the original test-tool versions were removed by upgrading to Vitest 5 and Vite 8.

Lifecycle tests use paused Tokio time and Vitest fake timers, without wall-clock sleeps. See [architecture](docs/architecture.md), [API](docs/api.md), and [protocol evidence](docs/ocstat-protocol.md).

## Configuration

Defaults require no configuration. `.env.example` documents shell variables; `.env` files are **not automatically loaded**.

```sh
SMARTTHERM_BIND=127.0.0.1:3001 ./target/release/smartthermweb
SMARTTHERM_SIM_CONFIG='{"scenario":"reply_lost","ack_ms":900,"apply_ms":2400}' ./target/release/smartthermweb
```

The Vite development proxy expects backend port 3000. Simulator controls can change behavior at runtime. Reply delay is limited to 30,000 ms; application delay to 60,000 ms. Both delays start at dispatch. Scenario settings affect future commands; offline status also affects reads immediately. There is no automatic resend after a failure. Last confirmed readings remain visible while offline.

The executable enforces a loopback bind. It is unauthenticated and must not be exposed through a proxy, port forwarding, or a public tunnel. Before supporting LAN/Tailscale access or real-device control, add appropriate access control and transport protection. The manufacturer traffic observed so far uses plaintext HTTP; see the protocol notes before supplying any credentials.

## Scope and limitations

- One simulated device, manual mode, 5–30°C range in 0.5°C increments. These limits and mode behavior are simulator choices, not verified OCSTAT capabilities.
- Room temperature is a fixed 20.8°C fixture; this is a command/network simulator, not a thermal model.
- State, command IDs, and latest per-browser revision counters live in memory and reset on restart. A fresh server session ID lets the browser distinguish resets from stale updates. At most 256 browser client IDs are tracked per process; restart the local simulator if that limit is reached.
- Full snapshots keep the API simple. Only the latest user command and active command are retained, not an audit history. Polling is every 500 ms outside a write request. Writes wait up to 3 seconds for a server reply and then up to 6 seconds for application. Background reconciliation continues after that deadline.
- Simulator observations include reliable application correlation and monotonic revisions. OCSTAT may not offer either; do not fabricate these guarantees in a future adapter.
- Schedules, mode changes, authentication, persistence, Nix, and deployment infrastructure are deferred.

To connect real hardware, provide **sanitized** request/response examples and evidence linking reported state to physical device application, verify authentication/message IDs and mode/schedule effects, then implement read decoding before any separately approved live-write experiment. Never commit raw captures or credentials.
