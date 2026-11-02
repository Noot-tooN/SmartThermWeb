# OCSTAT temperature-change capture — 2026-11-01

## Scope and handling

Offline analysis of the owner-provided `PCAPdroid_01_Nov_17_11_00.pcap`, kept outside the repository. The owner reports increasing the setpoint by 1°C, waiting for the physical device to reflect it, then decreasing it by 1°C and again waiting for the device. No network requests or thermostat commands were issued during this analysis. Only sanitized evidence is retained here.

Classic PCAP, raw IPv4 link type, 119 packets over 90.161 seconds, beginning at 16:11:06.143 UTC (17:11:06.143 Europe/Belgrade). TCP payloads were reassembled by sequence number with overlapping bytes suppressed; no gaps occurred in the captured payload streams. Six complete HTTP request/response exchanges and bidirectional TCP port 8800 traffic are present. No login, token refresh, rejected write, or post-write device-list request appears in this capture.

## Observed changes

Times below are relative to capture start. Decimal temperatures use the existing tenths-of-a-degree interpretation, supported by the owner's reported ±1°C changes.

| Event | Time | Evidence |
| --- | ---: | --- |
| Initial device list | +2.157 s | Target `215` (21.5°C), room `215`, online true, `work_mode: "1"` |
| Increase request | +11.097 s | Target `225` (22.5°C), `work_mode=1`, message ID A |
| HTTP success | +11.136 s | 39 ms after request; success JSON below |
| Correlated `L` TCP frame | +11.540 s | 443 ms after request; main target changes to `2d`; trailing message ID matches A |
| Subsequent `N` TCP frame | +12.260 s | 1.163 s after request; candidate reported target changes from `2B` to `2D` |
| Decrease request | +38.162 s | Target `215` (21.5°C), `work_mode=1`, distinct message ID B |
| HTTP success | +38.195 s | 33 ms after request; same success JSON |
| Correlated `L` TCP frame | +38.582 s | 420 ms after request; main target changes to `2b`; trailing message ID matches B |
| Subsequent `N` TCP frame | +39.308 s | 1.146 s after request; candidate reported target changes from `2D` to `2B` |

## Write request and success response

Both requests are POST `/ocstat/setThermostatWorkModeNew` to `www.ocstat-thermostat.com`, HTTP port 80, with `application/x-www-form-urlencoded` bodies. All seven fields below are present. Identity/session fields are equal across both writes; only target and message ID differ.

```text
userid_phoneid=CLIENT_A
&device_id=DEVICE_A
&user_id=USER_A
&current_temprature=225
&messageId=MESSAGE_A
&work_mode=1
&token=TOKEN_A
```

The second write substitutes `215` and `MESSAGE_B`. Observed message IDs are distinct four-character lowercase hexadecimal strings. Two samples do not establish randomness, uniqueness scope, collision handling, retry reuse, or an idempotency guarantee. The HTTP response does not echo the message ID.

Both responses: HTTP 200, `text/json;charset=UTF-8`, with this JSON structure and values:

```json
{"statu":true,"error_code":0,"message":"modify work mode success","tcpCode":0}
```

This is evidence of cloud acceptance, not proof by itself of physical application. Neither HTTPS support for this write endpoint nor acceptance without the Android client identifier/TCP registration was tested. Existing HTTPS login/list support does not prove either point.

## Status and correlation evidence

This capture contains server application payloads on TCP 8800, unlike the earlier login capture. The app registers using an identifier and token, requests device information, and exchanges `5501`/`5502` heartbeat messages about every 20 seconds. Both `queryClientTcpOnline` responses report `client tcp success.`. This connection is plaintext and is not an acceptable fallback for our HTTPS-only implementation.

Observed frame families are `##3cL` (60 bytes), `##21N` (33 bytes), and `##62P` (98 bytes). The hexadecimal length prefix matches the frame lengths in these samples. Full framing, checksums, reconnect behavior, and field semantics are not established.

Candidate fields (zero-based ASCII byte offsets, end exclusive):

| Frame/field | Before increase | After increase | After decrease | Interpretation |
| --- | --- | --- | --- | --- |
| `L[19:21]` | `2b` | `2d` | `2b` | Main target: hex 43/45, consistent with half-degree units |
| `L[36:38]` | `2b` | **still `2b`** | **still `2d`** | Separate field lags the newly requested target; possible previous device target |
| `N[21:23]` | `2B` | `2D` | `2B` | Later reported target, consistent with half-degree units |
| `L[55:60]` | `00000` | `4` + message ID A | `4` + message ID B | Observed request correlation; length-digit interpretation remains provisional |

`L[5:17]` matches the selected device identifier, which is omitted from this document. The `N` identifier region does not directly equal that identifier; its mapping is not decoded. Do not assume all notifications belong to the selected device when multiple devices exist.

The initial HTTPS-untested HTTP `getDeviceStatus` response is also a 60-byte `##3cL` frame with the initial target. It differs from the initial TCP `L` frames in its trailing metadata. No HTTP status or list read follows either write, so this capture cannot establish how polling changes after device application, whether it returns a cache, or whether it exposes the later `N` observation reliably.

The later `N` reports are stronger evidence of device feedback than the early correlated `L` messages, especially alongside the owner's physical observations. However, this trace alone cannot prove their origin/freshness contract or a fully decoded physical acknowledgement. The `L` field that retains the previous target is a useful lead, not yet a verified confirmation decoder.

## Owner-provided mode clarification

After the capture analysis, the owner supplied an app screenshot showing Auto selected, “Temporary mode Active,” and a 21.5°C target. This supplies UI context for the experiment: automatic operation with a temporary override, not a selected Manual mode. The screenshot is subsequent evidence, not a timestamped observation of every captured packet.

The capture's `work_mode=1` must not be labeled “Manual” on this evidence. It may describe a base operating mode or participate in a larger mode/override representation; the exact encoding remains unresolved. The screen is consistent with a temperature adjustment temporarily overriding an automatic schedule. It does not establish when the override expires, whether returning to the previous temperature cancels it, or what each protocol mode value means. In particular, the returned 21.5°C target is shown with the temporary override still active.

Implementation should preserve the distinction between the base Auto/Manual/Off setting and any temporary override. For the captured use case, reproduce an adjustment in Auto without assuming it switches to Manual or cancels the override when the old target is restored. Keep override duration and cancellation semantics unverified until directly observed.

## Readiness and next steps

There is sufficient evidence to implement the exact write form and observed success decoder behind disabled live-write controls, with synthetic fixtures. Unknown, malformed, rejected, or lost responses must remain unverified; retries must not assume idempotency.

Before enabling the complete live flow:

1. Mode UI context is now supplied: Auto with a temporary override (see clarification above). Determine how base mode and override are encoded before mapping protocol values or supporting other modes. `work_mode=1` alone is not evidence of Manual mode. Do not silently switch modes or invent override-expiry behavior.
2. Use a separately authorized HTTPS-only, read-only status/list investigation to establish a usable reported-device target and freshness behavior. If a further manual change is needed, the owner performs it while reads are observed. Preserve the cloud-accepted/device-confirmed distinction.
3. Verify whether writes require `userid_phoneid` or active TCP registration. Browser login currently discovers a device without an Android client identifier; do not invent or persist the captured phone identity.
4. Verify the write endpoint over HTTPS during a separately approved bounded live experiment. Never fall back to HTTP/TCP plaintext. Continue to fail closed until that authorization and the necessary evidence exist.

No additional capture is immediately necessary to implement request parsing/fixtures. This capture does not establish global temperature bounds, mode mappings, multi-device notification routing, retries, or failure semantics. Existing simulator bounds must not be presented as verified OCSTAT limits.


## Authorized read-only HTTPS experiment — 2026-11-01

The owner explicitly authorized HTTPS polling and performed both target adjustments in the Android app. The observer sent only POST requests to `getDeviceStatus` and `getDeviceListNew`, using the existing captured session token in memory. TLS certificate/hostname verification was enabled, redirects were not followed, and no login, write endpoint, or TCP 8800 connection was used. Both endpoints were sampled sequentially every three seconds; only allowlisted numeric/status fields were retained. Device identity was checked against the selected device before accepting responses. All times below are Europe/Belgrade (UTC+01:00).

| Observation time | Status main target (`L[19:21]`) | Candidate reported target (`L[36:38]`) | Device-list target |
| --- | --- | --- | --- |
| 17:36:49, initial baseline | 21.5°C | 21.5°C | 21.5°C |
| 17:38:21, after owner increased target | 22.5°C | 22.5°C | 22.5°C |
| **17:38:48, after owner restored target** | **21.5°C** | **22.5°C** | **21.5°C** |
| 17:38:51, next status sample | 21.5°C | 21.5°C | 21.5°C |
| 17:39:21, final inspected sample | 21.5°C | 21.5°C | 21.5°C |

The owner independently confirmed the physical thermostat displayed 22.5°C after the increase and 21.5°C after the return. Confirmation messages were not precisely timestamped physical observations, so do not infer exact device application latency from them. Polling was stopped after both transitions.

Both read endpoints returned HTTP 200 over certificate-validated HTTPS. Listing retained `online: true`, `work_mode: "1"`, `program_mode: "1"`, `disOnOff: "13"`, and `th_work: "1"`; room temperature was 21.7°C in the observed samples. `autoTemp` followed the target (215 → 225 → 215), so its name alone must not be interpreted as an unchanged scheduled setpoint. Status mode/flag candidates remained `1`/`1`, with reported flags `13`.

The return transition is especially useful: HTTPS status exposed the previous reported value for one sample while its main target and the device list already exposed the newly requested target. The following status sample caught up. This directly supports separate requested and reported target fields accessible without plaintext TCP. It also demonstrates why device-list target equality alone is insufficient confirmation.

Together with the earlier TCP capture and the owner's physical observations, this strengthens the interpretation of `L[36:38]` as a device-reported target. It is not a general freshness guarantee: no device-origin timestamp/revision or command identifier was established for HTTPS polling, and offline/cached/repeated-target behavior remains untested. The approximately three-second gap between these samples is a polling interval, not a measured physical application duration.

The next implementation can use separate requested/reported observations and fail closed on malformed, mismatched-device, offline, or uncertain responses. HTTPS write support and any Android client-registration requirement remain unverified; this experiment does not authorize sending writes or establish mode/override expiry semantics.


## Implementation follow-up

A single additional HTTPS `getDeviceStatus` read omitted `userid_phoneid` and succeeded with a device-matched status frame showing 21.5°C in both target fields. The Rust read path can therefore use the browser session's existing account/token/device without copying the Android client identity. This is evidence for reads only, not writes, and the Android app's separate connection state was not controlled during this check.

The adapter now implements the combined read and the bounded write candidate, with production writes blocked independently at provider and transport levels. Synthetic tests cover field separation, strict parsing, preflight mode/online checks, lost responses, cancellation, new retry IDs, and refusal to infer application correlation. The first candidate is limited to the two observed setpoints. Live HTTPS write acceptance and identity requirements still need the owner's separately approved experiment.


## Explicitly authorized HTTPS write trial — 2026-11-01

The owner authorized exactly the round trip 21.5 → 22.5 → 21.5°C, with physical checks and normal temperature controls left disabled. A standalone bounded probe issued exactly two manufacturer write requests, one per leg, using certificate-validated HTTPS with no redirects, HTTP fallback, TCP connection, login, or automatic resend. The production Rust application remained read-only throughout; this verifies the upstream protocol, not execution through the Rust candidate path. Credentials and device identifiers were loaded into memory from the private capture and never written to this repository.

Both requests omitted `userid_phoneid`. They included only `device_id`, `user_id`, `token`, `current_temprature`, a fresh four-character hexadecimal `messageId`, and the preserved `work_mode=1`. Two preflight list/status reads per leg verified the expected starting requested/reported target, online status, and unchanged captured mode context. Each leg was recorded as attempted before networking to prevent accidental repeats after uncertainty.

| Local time (Europe/Belgrade) | Event |
| --- | --- |
| 17:53:32.011 | Single request for 22.5°C reserved and dispatched |
| 17:53:32.144 | HTTP 200; observed success JSON (`statu=true`, `error_code=0`, `tcpCode=0`, expected success message) |
| 17:53:32.637 | Status requested 22.5°C, reported still 21.5°C |
| 17:53:35.187 | Status requested/reported both 22.5°C |
| 17:53:37.689 | Second matching status sample, device list also 22.5°C |
| 17:54:10.215 | Owner's physical confirmation of 22.5°C recorded; owner explicitly requests return |
| 17:54:13.247 | Single request for 21.5°C reserved and dispatched |
| 17:54:13.402 | HTTP 200; same observed success JSON |
| 17:54:13.860 | Status requested 21.5°C, reported still 22.5°C |
| 17:54:16.401 | Status requested/reported both 21.5°C |
| 17:54:18.901 | Second matching status sample, device list also 21.5°C; probe exited |

Both HTTPS writes were accepted without an Android client identifier, and the separate reported field caught up on each leg. No TCP registration was performed by this probe. The Android app's concurrent connection was not controlled, so this does not prove that all app connection state is irrelevant. Returning to 21.5°C restores the numeric setpoint; it does not establish cancellation of the temporary override. Mode-related `work_mode` and `program_mode` stayed `1`.

At this record's creation, the final physical check at 21.5°C is pending. Normal application controls and provider/transport write gates remain disabled. No extra writes or configuration changes are authorized by this record.


## Trial completion and explicit control enablement

The owner subsequently confirmed that the physical thermostat returned to 21.5°C, completing the authorized round trip, and explicitly requested enabling app temperature changes. No further physical test writes were made during enablement.

The Rust path now uses the verified six-field form without the Android client identifier. Authenticated same-origin requests are enabled only after an online read in the tested mode context; every write also performs a fresh preflight and preserves that raw mode. The app retains its 5–30°C policy limits and 0.5°C steps, without claiming that the entire range has been physically verified.

The match lifecycle uses a distinct `target_matched` state after two consecutive matching requested/reported observations for the latest local attempt. It does not populate `applied_command` or claim device-origin freshness. Tests cover the real provider/controller path through a fake transport, authenticated session isolation, origin validation, resets on failures/mismatches, and stale-attempt suppression. The completed trial supersedes earlier disabled-write notes; earlier records remain as history.


## Owner-observed early completion after enablement

The owner reports that the app showed its checkmark before the physical thermostat display changed: approximately 1–2 seconds early on an increase, and more than 20 seconds early on the subsequent decrease. These are owner-observed durations, not a synchronized packet/display trace. No additional thermostat commands were issued to investigate this report.

The current completion rule is two matching requested/reported fields in HTTPS cloud responses. This observation demonstrates that the rule does not establish physical display synchronization. The interpretation of `L[36:38]` as a physical-device-confirmed target is therefore not established; use “cloud-reported target field” when describing the evidence. It may reflect intermediate/cached state or precede a delayed physical display update. This report does not distinguish transport, gateway/device processing, and display refresh delays.

The matching-field state must not be presented as an unqualified device-synced checkmark. Separating cloud acceptance from physical-device verification would better express the evidence: acceptance can be confirmed from the observed success response, while device verification must remain explicitly unavailable/unverified until a suitably fresh device-origin signal is established. More identical cloud reads or an arbitrary visual delay alone cannot establish that signal. No UI or control behavior was changed in this explanatory investigation.
