# OCSTAT push and physical confirmation investigation

## What the capture establishes

Android uses a persistent plaintext TCP connection on **8800**, not 8080. It registers with an app client identifier and session token, sends device-specific selectors, and exchanges `5501`/`5502` heartbeat messages roughly every 20 seconds. Sanitized selector shapes are `&&111<DEVICE_ID>` (followed by an `L` status frame) and `&&122<DEVICE_ID>9` (followed by a `P` frame).

The adjustment capture contains four server `L` frames, four `N` frames, and three `P` frames. Some later `L`/`N` arrivals have no adjacent write or query, including an `N` frame at approximately +34.292 seconds. This establishes asynchronous push on that connection after setup. It does not establish whether each selector is a one-shot query, a subscription, or both. We see only messages delivered to this client during the capture, not all events the service supports. No topic catalogue, subscribe-to-all operation, or unsubscribe semantics has been established.

See [the write capture analysis](capture-write-analysis-2026-11-01.md) for the sanitized timeline and frame offsets, and [the login capture analysis](capture-analysis-2026-11-01.md) for connection setup.

## How SmartThermWeb receives updates

The Rust controller currently polls the OCSTAT HTTPS list/status endpoints: approximately every two seconds during reconciliation, and every 30 seconds while idle. Requests and reads are serialized. Rust publishes local snapshots over `/api/events` using server-sent events to the browser. Browser SSE is therefore our own stream of polled observations, not the manufacturer's TCP push connection. We do not connect to port 8800.

## Can an update be guaranteed?

Not with the evidence currently available. HTTP success proves server acceptance. The early `L` frame correlates to a request's message ID but does not prove physical application. The later `N` frame is promising telemetry, but has no established command correlation or device-origin timestamp/revision. Its identifier region differs from the selected device identifier; routing it in a multi-device account would require decoding that mapping first.

The owner's later app test observed matching cloud fields before the physical thermostat display changed, including a delay exceeding 20 seconds. Consequently, matching repeated HTTPS reads are not sufficient for a thermostat checkmark. Even a future verified device acknowledgement would need a clear contract: applied setpoint and refreshed LCD are not necessarily simultaneous.

The default UI now shows a checkmark only for explicit server acceptance. The thermostat icon stays neutral and unverified. `target_matched` remains an internal observation result; it is not physical confirmation.

## Useful next evidence

1. Synchronize a capture with timestamped physical display observations and make a change using the thermostat's own controls. This helps distinguish device-origin telemetry from an echo of an app write.
2. Compare Android behavior with the thermostat screen open, the device list open, and the app in the background. This can reveal which selectors enable or stop notifications.
3. If available, inspect the Android APK's frame dispatcher, selector calls, and identifier decoding to establish message families and device routing.
4. Verify whether an authenticated, certificate-validated encrypted push transport exists before considering production use. The captured TCP connection exposes its token in plaintext and is not an HTTPS fallback.

This investigation used local capture evidence and source inspection only. It sent no thermostat changes and made no new manufacturer connections.

## Credential-free encrypted transport probes — 2026-11-01

The owner authorized investigating an encrypted direct listener, with no plaintext credentials and no thermostat changes. Offline packet-header inspection identified the captured TCP 8800 destination. A live DNS lookup confirmed that `www.ocstat-thermostat.com` currently resolves to that same address. Only the observed push port and the known HTTPS port were probed; no port scan or speculative application commands were used.

| Probe | Result |
| --- | --- |
| Python/LibreSSL 2.8.3, port 443, SNI and hostname verification | TLS 1.2 negotiated in approximately 0.12 seconds; certificate chain and hostname verified |
| Python/LibreSSL 2.8.3, captured destination port 8800, same SNI and verification | TCP connected; TLS handshake timed out after 8 seconds |
| OpenSSL 4.1.0, port 443, SNI and hostname verification | TLS 1.2 negotiated in approximately 0.22 seconds; certificate verification OK |
| OpenSSL 4.1.0, captured destination port 8800, SNI and hostname verification | TLS handshake timed out after 8 seconds; no brief handshake diagnostic |
| OpenSSL 4.1.0, captured destination port 8800, no SNI but hostname/certificate verification still required | TLS handshake timed out after 8 seconds; no brief handshake diagnostic |

The HTTPS certificate covers `www.ocstat-thermostat.com` and `ocstat-thermostat.com`, with a reported expiry of 2026-11-18 23:59:59 GMT. Both successful control handshakes negotiated `ECDHE-RSA-AES128-GCM-SHA256`.

**Conclusion:** no working direct TLS connection was established on the captured push port. This is consistent with the plaintext protocol seen in Android's capture, but handshake timeouts do not prove that every possible encrypted push endpoint or protocol upgrade is unavailable. Port 443 supports validated TLS for the web service; this alone does not establish that the custom push protocol is available there.

No credentials were loaded for the network probes. No account/device registration, login, subscription, heartbeat, HTTP request, or thermostat command was sent. Application code and running services were unchanged. No certificate-verification bypass or plaintext fallback was used.

The next useful source is the Android APK/configuration or manufacturer protocol information, to look for an alternate TLS hostname/port, a documented encrypted upgrade, or a WSS endpoint. An authenticated direct listener remains unimplemented until a secure transport is established. The earlier statement that the investigation made no new manufacturer connections applies to the preceding offline investigation; this section records the explicitly authorized transport-only connections.

## Explicitly authorized temporary plaintext listener — 2026-11-01

After the unsuccessful TLS probes, the owner explicitly authorized plaintext authentication for this investigation, acknowledging token exposure, with no thermostat changes and no integration into the normal app. A standalone Python process outside the repository ran for 90 seconds, from 17:56:28 to 17:57:58 UTC. It then closed its socket and exited successfully.

The process loaded the captured registration and two read selectors into memory, checked that the selectors referenced the same device, and verified that the captured destination still matched the known service hostname. It reused the captured Android client identity and session token, without issuing a fresh login. Reusing that identity may displace an Android connection; the owner was told to leave Android closed. Its only outbound application messages were the one captured registration, one instance of each read selector, and four `5501` heartbeats. It sent no target/mode/schedule modification, HTTP request, or automatic reconnect. No app source or running app process was changed.

| Event | Seconds after listener start | Result |
| --- | ---: | --- |
| TCP connected; registration sent | 0.038 | Connection established on 8800 |
| Both captured read selectors sent | 4.043 | Sent once |
| `P` frame | 4.147 | 98 bytes; contents not exported |
| `L` status frame | 4.176 | Selected-device identifier matched; requested and candidate reported fields both 21.5°C |
| First `N` notification | 7.675 | Candidate reported target 21.5°C |
| Heartbeat replies | 10.073, 30.156, 50.125, 70.090 | All four `5501` heartbeats answered with `5502` |
| Second `N` notification | 68.722 | Candidate reported target still 21.5°C; no additional device query preceded it |
| Listener closed | 90.043 | No framing errors or incomplete buffered frame |

This demonstrates a working direct client: Android is not required to receive the captured push protocol. The later `N` notification arrived approximately 64.7 seconds after the only device queries, establishing continued asynchronous delivery after setup. Two notifications do not establish a fixed notification interval, a complete subscription contract, or all supported message families.

Both `N` identifier regions matched the corresponding identifier seen in the earlier capture. Their mapping to the selected device remains unverified; matching the captured identifier must not be treated as a general multi-device routing decoder. The parsed temperature remains a candidate field, not a physical application acknowledgement. No controlled target change or physical observation marker was part of this test, so it cannot explain the earlier 40-second delay.

Only an allowlisted redacted timeline was saved, at `/tmp/smarttherm-push-observation-2026-11-01.json`. Registration bytes, account/device/client identifiers, tokens, and raw frames were not printed or saved in the output or repository. The temporary script contains no embedded secrets. This explicit investigative exception does not change the normal app's HTTPS-only transport policy.

The next timing experiment can run the same isolated listener alongside the web recorder while the owner makes changes and marks physical observations. It can establish whether later `N` notifications align more closely with those observations than the polled HTTPS fields. The listener itself must continue to send no thermostat modifications.

## Direct listener alongside owner-operated web round trip — 2026-11-01

The owner authorized running the isolated listener alongside the web recorder, performed the 21.5 → 22.5 → 21.5°C round trip, and clicked the physical-observation marker after each display update. The listener itself sent only registration, the two read selectors once, and heartbeats. It connected at 17:59:25 UTC and was explicitly stopped at 18:02:50 UTC after the owner reported completion. It exited normally with no incomplete frame or recorded error; all ten heartbeats received replies.

The input web recording is the owner-provided `smarttherm-sync-2026-11-01T18-02-07-799Z.json`. The redacted TCP recording is `/tmp/smarttherm-push-roundtrip-20261101T175925Z.json`; its incremental companion has the `.jsonl` extension. Neither file is added to Git.

All intervals below start at browser request dispatch, after debounce. Each leg's two clicks were coalesced into one request. There was no competing pending target, so this run does not exercise the new superseded-command queue path.

| Event | Increase to 22.5°C | Return to 21.5°C |
| --- | ---: | ---: |
| Browser request time (UTC) | 18:01:07.205 | 18:01:22.677 |
| Explicit server acceptance observed | +0.363 s | +0.213 s |
| First matching TCP `N` candidate temperature | +1.304 s | +0.885 s |
| First matching requested/reported HTTPS fields | +2.820 s | +2.603 s |
| Owner's physical-observation click | +12.701 s | +42.996 s |

The preceding TCP `L` frame in each leg carried the new requested target and the old candidate reported target: 22.5/21.5°C at 18:01:08.116, then 21.5/22.5°C at 18:01:23.162. The following `N` candidate target changed quickly, approximately 11.397 and 42.111 seconds before the respective physical-observation clicks.

An additional matching `N` arrived at 18:01:20.334, 0.428 seconds after the increase marker. However, the next matching `N` after the return arrived at 18:02:24.332, 18.659 seconds after that marker. Therefore neither the first matching `N` nor a simple second-notification rule supplies consistent physical-display confirmation in this experiment.

No HTTPS read failure, offline reading, listener error, or missing heartbeat response was recorded. Wall-clock versus monotonic timestamp offset varied by at most approximately 1 ms within each recording, with both collected on the same Mac. This supports timeline alignment but does not turn human reaction times into exact device application timestamps.

Conclusion: push exposes the candidate target sooner than HTTPS polling, but that field is also ahead of the owner's physical observation. Simply switching to push would not establish physical synchronization or explain the long delay. Transport delivery, firmware application, and LCD refresh remain indistinguishable with the currently decoded evidence.

Instrumentation limitation: the listener exported only known frame families, lengths, identity-match booleans and candidate temperatures. Other frame fields were not retained. This test cannot rule out a useful flag or acknowledgement elsewhere in the protocol. A subsequent investigation should examine redacted non-identity field changes or the Android decoder/manufacturer documentation, rather than assuming the decoded temperature alone proves application. Thermostat-side traffic or firmware/protocol evidence would be needed to separate cloud-to-device delivery from internal processing/display timing.

## Complete-field offline follow-up

The [offline field analysis](ocstat-tcp-fields-2026-11-01.md) revisits all TCP message bytes in the original capture. It identifies a delayed `N` transition at offsets 17 and 25, resembling the status fields `th_work` and `disOnOff`; those flags remain set through the subsequent return request. This is a useful decoder lead, not a verified per-command physical acknowledgement. No network requests or thermostat changes were made during this follow-up.

## Installed APK decoder findings

The owner-authorized APK extraction and [offline Android decoder inspection](android-decoder-analysis-2026-11-01.md) resolved the `N` device mapping and heating flag. It also corrected the interpretation of the candidate reported temperature: Android stores it as `autoTemp`, not a physical-command acknowledgement. The type-22 app's success handler uses the early correlated `L` message; no additional physical-applied check was found on that path. No app behavior, listener integration, or thermostat setting was changed by this analysis.
