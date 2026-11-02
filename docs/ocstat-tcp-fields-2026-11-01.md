# Offline TCP field analysis — 2026-11-01

## Scope and completeness

Re-examined the original owner-provided `PCAPdroid_01_Nov_17_11_00.pcap`, without any network access, registration, login, or thermostat command. Reassembled all captured TCP application streams by sequence number, checking for gaps and contradictory overlaps. The capture contains 119 complete packets, 14 payload-bearing directional streams, six HTTP requests/responses, and these server push frames: four `L`, four `N`, three `P`, plus five `5502` heartbeat replies. No gaps, truncated packets, contradictory overlaps, or unknown push framing were found. These statements apply to this capture's scope, not to every possible protocol message.

All offsets below are zero-based ASCII byte offsets; ranges exclude the ending offset. Account, device, app-client and session identities are omitted. The two write message IDs are referred to only as A and B. Raw frames and credentials are not reproduced.

## New lead: a later notification changes flags, not target

The increase request occurs at +11.097 seconds and the return at +38.162 seconds. The complete `N` frames differ only at offsets **17, 22, and 25**; offset 22 is the varying low digit of the target field at `[21:23]`. Their identity region and every other byte remain unchanged across the four frames.

| `N` arrival after capture start | Context | `[17]` | Target `[21:23]` | `[25:27]` |
| --- | --- | --- | --- | --- |
| +5.263 s | Baseline | `0` | 21.5°C | `03` |
| +12.260 s | 1.163 s after increase request | `0` | 22.5°C | `03` |
| +34.292 s | 23.195 s after increase request; no adjacent write/query | `1` | 22.5°C | `13` |
| +39.308 s | 1.146 s after return request | `1` | 21.5°C | `13` |

This is a real additional state transition beyond the early target echo. The two flags change together, approximately 22 seconds after the first notification showing the increased target. They do **not** reset or change when the later return target arrives. They therefore are not demonstrated per-command completion flags.

The initial HTTP device-list response contains `th_work: "0"` and `disOnOff: "03"`, matching the initial `N[17]` and `N[25:27]`. The corresponding `L` positions are `[18]` and `[38:40]`: initial values `0`/`03`, later values `1`/`13`. The later `L` frame after the return carries the same flags as the preceding delayed `N`. These alignments suggest a shared status representation. A heating/output-state interpretation is plausible from the field names and context, but is **not verified**. It must not be promoted to physical target confirmation.

Other `N` data remain constant: `[18:21]` is `0d7` (hex 215, consistent with the observed room reading in tenths), `[23:25]` is `22` (meaning unknown), and `[27:33]` is `000000`. Neither captured write's four-character message ID appears in the non-identity `N` data. No changing counter, timestamp, or additional status byte appears in these four frames. This does not establish that the protocol has no such fields or other message families.

## `L`: request correlation exists, but is early

The four `L` frames arrive at +4.048, +9.033, +11.540, and +38.582 seconds. All match the selected device identity. Outside the identity region and four trailing correlation characters, only offsets **18, 20, 37, 38, and 55** vary:

- `[19:21]`: requested target, 21.5 → 22.5 → 21.5°C.
- `[36:38]`: candidate reported target, still 21.5°C in the increase response and still 22.5°C in the return response.
- `[18]` and `[38:40]`: the flags described above.
- `[55]`: `0` in the two queried TCP status frames and `4` in both write-correlated frames.
- `[56:60]`: matches message ID A or B in the corresponding write-correlated frame.

The correlated frames arrive 443 ms and 420 ms after their requests while retaining the previous candidate reported target. Correlation therefore establishes which request the frame concerns, not physical application.

The initial HTTP `getDeviceStatus` response differs from the initial TCP `L` status only at byte 55 (`1` for HTTP, `0` for TCP). That HTTP request supplies no message ID. Consequently byte 55's interpretation remains uncertain: its values cannot simply be declared an applied/not-applied flag, and the previous suggested message-ID-length interpretation is not established.

## `P` and heartbeats

All three 98-byte `P` frames are byte-for-byte identical, including the one at +53.931 seconds after both writes. They offer no changing acknowledgement evidence in this capture. All server heartbeat replies are the same four bytes, `5502`; they indicate connection responsiveness, not thermostat application.

## What this does and does not establish

The original capture contains a delayed status-flag transition worth decoding. It does not establish a real per-command physical acknowledgement. There are no timestamped physical display markers in the original capture to align that transition with actual display application. The recent direct-listener logs retained only candidate temperatures and identity-match booleans, not these flags, so they cannot retroactively establish what the flags did during the 43-second return delay.

The best next step before another round trip is inspecting the Android decoder or manufacturer field definitions for `N[17]`, `N[25:27]`, `L[18]`, `L[38:40]`, `th_work`, and `disOnOff`. If another capture is needed afterward, retain these non-identity status fields along with temperature and physical markers. Any decoder must also resolve the differing `N` identifier before using it for multi-device routing. Leave physical synchronization unverified until there is evidence for the actual acknowledgement semantics.

## Installed APK follow-up

The [Android decoder analysis](android-decoder-analysis-2026-11-01.md) resolves several prior unknowns: `th_work` drives the heating indicator; `N` carries a gateway ID plus indexed device entries (the ID mapping is now validated against this capture); `[21:23]` is `autoTemp`; and type-22 request success follows the early message-ID-correlated `L` reply. No separate physical-application acknowledgement was established. The exact `disOnOff` high-bit semantics for type 22 remain unverified.
