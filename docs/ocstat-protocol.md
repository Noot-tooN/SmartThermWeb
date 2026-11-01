# OCSTAT / uniTEC SQ10net RF protocol evidence

## Evidence available for this iteration

The project brief and one private PCAPdroid capture have now been inspected. See [the sanitized capture analysis](capture-analysis-2026-11-01.md) for the timeline, schemas, and limitations. The capture spans 20.336 seconds and includes two successful plaintext HTTP logins, earlier list/status requests, and client TCP 8800 messages. The offline analysis was followed by an explicitly authorized HTTPS login and read-only device-list test, recorded below. No thermostat write requests have been made by this project.

| Observation | Evidence/status |
| --- | --- |
| Host `www.ocstat-thermostat.com`, HTTP port 80 | Confirmed in the capture; password field and tokens are sent without transport encryption |
| POST `/ocstat/login` | Form `email`, `password`; JSON `user.token`; both successful logins return the token already used by earlier reads; expiry field empty |
| POST `/ocstat/getDeviceListNew` | JSON device list observed; success has `statu: true`, `error_code: 1`; freshness unverified |
| POST `/ocstat/getDeviceStatus` | Custom text frame starting `##3cL`, despite a JSON content type; field mapping unverified |
| POST `/ocstat/queryClientTcpOnline` | App/client identifier supplied; all three calls report `client tcp fail.` while list reports thermostat online |
| TCP port 8800 | Client messages contain token and app/device identifiers; no server application payload captured |
| POST `/ocstat/setThermostatWorkModeNew` | Reported in project brief only; complete success/error semantics unknown |
| POST `/ocstat/setProgrammeByDay` | Reported in project brief only; out of MVP scope |
| Setpoint form fields `userid_phoneid`, `device_id`, `user_id`, `current_temprature`, `messageId`, `work_mode`, `token` | Reported in brief; no write captured in this round; preserve spellings exactly |
| Temperature units in tenths of °C | Capture values consistent with brief; physical display, range, and steps still need verification |

## Credential-free transport probes — 2026-11-01

Normal certificate verification was enabled; no insecure TLS option, credentials, cookies, form payloads, or thermostat commands were supplied.

| Request | Result |
| --- | --- |
| HEAD `https://www.ocstat-thermostat.com/` | HTTP 200, certificate verification succeeded, no redirect |
| HEAD `https://www.ocstat-thermostat.com/ocstat/getDeviceStatus` | HTTP 200, certificate verification succeeded, no redirect |
| HEAD `http://www.ocstat-thermostat.com/` | HTTP 200, no HTTPS redirect |
| GET HTTPS homepage | HTTP 200, `text/html;charset=UTF-8` |
| GET HTTPS `/ocstat/getDeviceStatus` without parameters | HTTP 200, `text/json;charset=UTF-8`, literal body `Illegal access` |

This establishes that the observed hostname supports certificate-validated HTTPS and that the status path returns a distinct access-denial response over HTTPS. These probes do **not** establish authenticated POST support, token behavior, equivalence with the app's HTTP responses, or TLS support on TCP 8800. The subsequent capture identifies the HTTP login endpoint, as documented above. A 200 status and JSON content type are not sufficient to classify a successful API response. No authentication implementation has been added.

## Authorized HTTPS login and read-only verification — 2026-11-01

At 10:35 UTC, the user explicitly authorized using the captured credentials to test HTTPS login and a device read, with no thermostat changes or HTTP fallback. A standalone offline-parser/probe script loaded credentials from the private capture into memory and sent exactly these requests to `https://www.ocstat-thermostat.com`:

| Request | Result |
| --- | --- |
| HEAD `/ocstat/login`, no credentials | HTTP 200 |
| POST `/ocstat/login`, captured email/password form | HTTP 200; JSON `statu: true`, `error_code: 0`; returned account matches capture |
| POST `/ocstat/getDeviceListNew`, using the login response token/user ID and captured list parameters | HTTP 200; JSON `statu: true`, `error_code: 1`; one device, identifier matches capture, `online: true` |

Default certificate-chain and hostname verification remained enabled. Redirects were blocked; all requests used HTTPS, with no HTTP fallback. No TCP 8800 connection or write endpoint was used. Raw credentials and response bodies were neither printed nor saved by the probe; only sanitized results were retained.

The list returned `current_temprature: "215"` and `inside_temparature: "211"`, consistent with 21.5 °C target and 21.1 °C room under the existing tenths convention. This establishes authenticated HTTPS login and device listing for this account at the test time. It does not establish list freshness, physical device confirmation, authenticated `getDeviceStatus` behavior, write support, or TLS on port 8800.

The login token matched the capture and `token_invalid_time` remained empty. Token expiry, rotation, revocation, and refresh semantics remain unverified. The app's transformation, if any, of the captured password field also remains unknown.

Next: collect longer session/status evidence using [the capture analysis](capture-analysis-2026-11-01.md) and [Android capture guide](android-capture-guide.md). Preserve HTTPS-only transport when implementing authentication; do not silently fall back to HTTP.

## Adapter boundary

`src/provider/ocstat.rs` now implements the explicitly authorized HTTPS login and read-only device-list integration. Its production transport has a fixed HTTPS origin, only two endpoint variants (`login`, `getDeviceListNew`), certificate verification, disabled redirects, 10-second request timeouts, and a 1 MiB response cap. Raw upstream responses and credential-like extra fields are not exposed or logged.

The backend reads a restricted server-side credential file and caches the login token only in memory. It polls every 30 seconds, selects the configured device ID, validates endpoint-specific response schemas/success codes, and preserves unknown mode semantics. Login transport retries are limited to once per 60 seconds; explicit authentication rejection stops until restart. No expiry or refresh flow is invented.

The executable accepts `SMARTTHERM_PROVIDER=ocstat` in **read-only** mode. API/controller submissions fail closed; `send_target` always returns `ReadOnly` without network access. The UI disables controls and hides command progress. No write endpoint or TCP 8800 integration exists. Local API access remains restricted to loopback.

Readings come from the verified list endpoint, not the undecoded `getDeviceStatus` frame. The `confirmed` snapshot field is retained for API compatibility but represents a cloud observation in OCSTAT mode, not physical device confirmation. Revisions order local reads only; `applied_command` is always null. `last_sync` records the successful cloud read time, not the age of the physical measurement.

## Remaining evidence before further protocol implementation

1. Sanitized samples of device listing/status/online requests and successful, rejected, expired-token and malformed responses, with field names/types preserved.
2. Authentication/session establishment and refresh rules. Do not bypass manufacturer authentication. Determine what userid_phoneid encodes without storing real identifiers in the repository.
3. `messageId` generation, uniqueness, reuse/retry behavior and acknowledgement correlation.
4. Exact work_mode values and the consequences of manually changing a target while scheduled/off. Verify on-device behavior; do not assume a write preserves a weekly schedule or mode.
5. Whether getDeviceStatus is live device state, last-known cache, or an echo of accepted cloud commands; timestamp/revision behavior and freshness guarantees.
6. Whether TCP 8800 supplies actual device acknowledgements; framing, heartbeats and reconnection behavior, if required. Do not add a speculative TCP client.
7. Valid target range/steps, rounding, device models, rate limits, and multi-client/out-of-order command behavior.

Start with fixture-based decoders and read-only reconciliation. Add a separate explicit opt-in for real writes only after the protocol is understood and the owner approves a specific live experiment. An environment variable alone must never silently activate writes in this MVP.

## Sensitive data

HTTP port 80 provides no encryption. Tokens, user identifiers, device identifiers, and thermostat settings can be visible to intermediaries and devices on the network. Certificate-validated HTTPS login and device listing are now verified by the authorized test above. Other endpoints and TCP 8800 transport remain unverified. Require HTTPS for credentials and tokens; do not downgrade silently.

Do not commit raw PCAP/PCAPNG/HAR files, unsanitized payloads, tokens, passwords, or identifying values. Inspect future captures offline and retain only manually reviewed redacted fixtures. `.gitignore` excludes common capture/secret files, but ignore patterns are not a substitute for review. The fixture-like form test uses only literal `REDACTED` placeholders.

## Rust implementation verification — 2026-11-01

The release executable was checked against the account using the private server-side credential file. Its initial worker login and device-list read succeeded over HTTPS. The API reported OCSTAT, `can_set_target: false`, online status, target 215 and room 213 (integer tenths), with null desired/command/application-correlation fields. No credential-file values appeared in the snapshot. No manufacturer write request was made.

Automated validation passed: 26 Rust tests and 27 frontend tests; Clippy, Rust/Prettier formatting and Svelte/TypeScript checks; the embedded release build; and the standalone simulator smoke test (SSE lifecycle, lost replies and reconnect). Tests cover token reuse, authentication rejection, login retry cooldown, malformed payloads, exact device selection, unrestricted observed temperatures, HTTP refusal, private-file permissions, preserved readings on cloud failure, API/controller/provider write rejection, and disabled browser controls. Browser automation was unavailable; the live frontend was served successfully, with UI behavior verified in component tests.
