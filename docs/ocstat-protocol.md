# OCSTAT / uniTEC SQ10net RF protocol evidence

## Evidence available for this iteration

The supplied project brief reports a PCAPdroid capture of the SmartTherm by OCSTAT Android application. No capture or sanitized response fixtures were supplied in the attachment or repository. We have not contacted the manufacturer or the thermostat. The table below records the user's capture observations, not independently validated API behavior or an official specification.

| Observation from brief | Status |
| --- | --- |
| Host `www.ocstat-thermostat.com`, HTTP port 80 | Reported observation; plaintext credential exposure risk |
| POST `/ocstat/getDeviceListNew` | Request/response schema unavailable |
| POST `/ocstat/getDeviceStatus` | Response types, temperature keys, freshness and origin unavailable |
| POST `/ocstat/setThermostatWorkModeNew` | Temperature-change endpoint observed; complete success/error semantics unknown |
| POST `/ocstat/setProgrammeByDay` | Out of MVP scope |
| POST `/ocstat/queryClientTcpOnline` | Meaning and freshness of online status unknown |
| Persistent TCP port 8800 | Framing, authentication, subscriptions and acknowledgements unknown |
| Setpoint form fields `userid_phoneid`, `device_id`, `user_id`, `current_temprature`, `messageId`, `work_mode`, `token` | Field names reported; **preserve `current_temprature` exactly** |
| Temperature units in tenths of °C | Reported for observed values; full range and step rules unverified |

## Adapter boundary

`src/provider/ocstat.rs` supplies a bounded Reqwest client (redirects disabled), a form structure preserving observed field names, and a fail-closed Provider implementation. No URLs are contacted; read/write return UnverifiedProtocol. There is no speculative response parser and no credential configuration. The executable refuses `SMARTTHERM_PROVIDER=ocstat`.

This is intentionally not a functional cloud integration. HTTP 200 must not be mapped to AcceptedByCloud until the body schema and explicit acceptance criteria are verified. Even then, cloud acknowledgement must not be mapped to device confirmation. The application correlates physical application only when an observation can identify the applied command; a cloud cache that echoes a setpoint does not provide that evidence.

## Evidence needed before implementation

1. Sanitized samples of device listing/status/online requests and successful, rejected, expired-token and malformed responses, with field names/types preserved.
2. Authentication/session establishment and refresh rules. Do not bypass manufacturer authentication. Determine what userid_phoneid encodes without storing real identifiers in the repository.
3. `messageId` generation, uniqueness, reuse/retry behavior and acknowledgement correlation.
4. Exact work_mode values and the consequences of manually changing a target while scheduled/off. Verify on-device behavior; do not assume a write preserves a weekly schedule or mode.
5. Whether getDeviceStatus is live device state, last-known cache, or an echo of accepted cloud commands; timestamp/revision behavior and freshness guarantees.
6. Whether TCP 8800 supplies actual device acknowledgements; framing, heartbeats and reconnection behavior, if required. Do not add a speculative TCP client.
7. Valid target range/steps, rounding, device models, rate limits, and multi-client/out-of-order command behavior.

Start with fixture-based decoders and read-only reconciliation. Add a separate explicit opt-in for real writes only after the protocol is understood and the owner approves a specific live experiment. An environment variable alone must never silently activate writes in this MVP.

## Sensitive data

HTTP port 80 provides no encryption. Tokens, user identifiers, device identifiers, and thermostat settings can be visible to intermediaries and devices on the network. HTTPS support is unverified. Document and consciously address this before using real credentials; do not invent a secure endpoint or downgrade silently.

Do not commit raw PCAP/PCAPNG/HAR files, unsanitized payloads, tokens, passwords, or identifying values. Inspect future captures offline and retain only manually reviewed redacted fixtures. `.gitignore` excludes common capture/secret files, but ignore patterns are not a substitute for review. The fixture-like form test uses only literal `REDACTED` placeholders.
