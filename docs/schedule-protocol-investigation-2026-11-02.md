# Schedule transport investigation — 2026-11-02

## Scope and result

Inspected SmartTherm 128/code 136 locally, decoded the existing November 1
captures offline, and made bounded HTTPS read requests. No thermostat write,
mode change, plaintext connection, fresh login, or automatic retry was sent.
Credentials were used in memory and are absent from this report.

The draft model can describe the observed schedule, but live saving remains
blocked on a usable secure schedule readback and a verified write exchange.
The current `map_draft` output contains active rows only: it is deliberately
not a complete OCSTAT write payload.

## HTTPS read checks

Certificate verification was enabled; redirects and plaintext fallback were
disabled. The first four calls reused the previously authorized captured token;
the final three sent no credentials and checked only APK-listed route presence.

| Endpoint | Result | Schedule usefulness |
| --- | --- | --- |
| `getDeviceListNew` | HTTP 200, successful JSON | Device mode and status, no programme periods |
| `getDeviceStatus` | HTTP 200, non-JSON response | Existing control/status endpoint; no new schedule read established |
| `getDeviceSettingInfo` | HTTP 200, successful JSON | Mode and event count, no programme periods |
| `getProgrammeByMode` | HTTP 404 | Still unavailable at the APK URL with HTTPS substituted |
| `getAllDevicesData` | HTTP 404, unauthenticated | No usable route established |
| `getAllDevicesDataOnline` | HTTP 404, unauthenticated | No usable route established |
| `getDevicesListData` | HTTP 404, unauthenticated | No usable route established |

The last three constants have no callers in the recovered application code.
A 404 does not prove that no secure schedule API exists elsewhere. A bounded
public search for the exact API names and OCSTAT API documentation found no
usable protocol documentation; this is not proof of absence either.

## Android type-22 behavior

Source paths below are relative to the recovered `cn/com/ocstat/homes/` tree.

- `fragment/heat/add/ScheduleAddFragment.java:352`: schedule reads send
  `&&122<device_id><hex_day>` through `PushManager`.
- `activity/heat/HeatFragment_S725_Activity.java:391`: the HTTP
  `getProgrammeByMode` caller belongs to the separate S725 path, not the
  selected type-22 screen.
- `activity/heat/add/EditAddScheduleActivity.java:493`: saving a day sends
  `token`, `user_id`, `device_id`, `day`, `programme`, and `messageId` to
  `setProgrammeByDay`. `programme` is JSON pairs of start minutes and integer
  temperature tenths. The request carries the whole edited group, including
  inactive slots; it is not just the selected period.
- `bean/ProgrammeBean.java:65`: serialization retains every row supplied by
  the editor. Deleting or superseding a row adds 2880 to its start time rather
  than removing the slot (`ScheduleAddFragment.java:792`,
  `EditAddScheduleActivity.java:469,500`).
- `activity/heat/add/HeatAddFragmentActivity.java:561`: changing the pattern is
  a separate `setProgrammeModeNew` request with `mode`, `day`, and `messageId`
  in addition to authentication and device identity. First-day codes are 1, 8,
  and 10 for weekly, weekday/weekend, and daily patterns respectively.
- `activity/heat/add/EditAddScheduleActivity.java:79,82,123`: HTTP success does
  nothing in this activity's callback. A matching message ID from a `P` push
  frame completes its success path. The timeout is 6000 ms. This is client
  behavior, not proof that the physical thermostat applied the schedule.

A full-week Save must therefore be treated as potentially several independent
operations. No atomic whole-week transaction or idempotent retry was established.
The correct order of mode activation versus group writes remains unverified.

## Existing capture confirms the hidden slots

Offline replay of `PCAPdroid_01_Nov_17_11_00.pcap` found three identical `P`
frames, four `L` frames, and four `N` frames, with no TCP reassembly gaps.
The first login capture contained no matching schedule frames.

`DispatchPushMessage.java:344,674` decodes each `P` message as a day and next
day, slot count, programme mode, 12 temperatures, 12 start times, and correlation
information. `HeatAddFragmentActivity.java:1054` splits those into six slots
per day/group. Times decode as hexadecimal hours * 60 + hexadecimal minutes;
temperatures decode as hexadecimal half-degree counts * 5 tenths.

The historical payload identifies weekend group 9 followed by weekday group 8,
mode 1, count 6. Both groups contain two active changes: 07:00 at 21.5°C and
23:30 at 18.5°C. Each also contains four disabled entries with start minutes at
or above 2880. Thus `count=6` is not the number of visible active periods.

This agrees with the screenshots, but it is a November 1 snapshot, not the
current live schedule or a valid pre-write backup. Neither capture contains a
schedule-save request. The capture alone cannot establish write response
semantics, successful disabled-slot replacement, or physical application.

## Integration plan

1. Establish a secure readback for all relevant groups and mode. Ask the vendor
   for the type-22 HTTPS schedule-read API or an encrypted equivalent of its
   TCP schedule service if APK/protocol evidence yields no further route.
   Do not silently reintroduce the plaintext listener into normal operation.
2. Implement a raw provider snapshot that preserves six slots per group,
   including disabled times/temperatures, alongside the clean browser model.
   Decode/encode fixtures must cover overnight carry-over, disabled entries,
   duplicate boundaries, and mode/day mapping. Do not invent slot padding.
3. Load that snapshot before editing. Save submits the whole intended draft
   once, but Rust calculates and serializes the changed-group operations.
   Re-read before writing and reject a changed baseline. Without an upstream
   conditional-write API this reduces, but cannot eliminate, cross-client races.
4. Keep writes in the existing controller worker, serialized with temperature
   writes. Track accepted, failed, and uncertain groups separately; retain the
   user's desired draft. Do not retry or roll back automatically after an
   uncertain result. Report server acceptance without claiming physical sync.
5. After explicit approval, capture one Android schedule edit and restoration,
   preserving the existing mode. Change one period's temperature by 0.5°C in
   a future, inactive period; record its original value/time, save, reopen,
   restore, save, and reopen. Record the whole request/reply/push sequence.
   This can verify the wire write and correlation, but does not solve secure
   readback or prove future physical execution by itself.
6. Only after readback and fixture checks, conduct a separately authorized
   HTTPS write/restoration trial with a fresh backup and defined recovery path.
   Keep the normal Save button disabled until that passes.

At the end of the HTTPS investigation, the UI still contained drafts only.
The owner-approved follow-up below supersedes that read-only integration status.

## Owner-approved plaintext follow-up

The owner subsequently authorized plaintext transport. An authenticated HTTP
`getProgrammeByMode` request also returned 404. A bounded TCP 8800 read using a
fresh client ID then returned complete weekday/weekend groups, each with 07:00
at 21.5°C and 23:30 at 18.5°C plus four disabled slots. HTTPS reads before and
after agreed on the selected device and mode. No thermostat write was sent.

The app now uses this path for explicit schedule loads. Login, status and target
changes remain HTTPS. Drafts load from the server rather than screenshot values;
Discard restores the loaded baseline. Save remains disabled pending write
verification. The earlier secure-readback requirement is superseded by the
owner's plaintext authorization; it is not an unresolved permission question.

A separate temporary Rust process passed the full browser-session API path:
HTTPS login, authenticated schedule GET, unchanged local desired target/no
command, then local logout. The result contained both active groups and
`can_save: false`. Device IDs may contain printable punctuation; fixtures
exercise this without including the owner's actual ID. The P-frame trailer
flag is validated as hexadecimal but is not assigned acknowledgement semantics.

## Save readiness at the read-only checkpoint

The TCP reader validates all six raw slots per group but returns only the clean
active schedule. A write implementation still needs to retain the raw slots for
a fresh baseline and faithful replacement. No schedule-save request exists in
the captures inspected so far; HTTPS schedule-write success has not been tested.

Start by verifying one Android edit and restoration within the existing mode.
The first web save should support changed groups in that same mode, check for
baseline conflicts, serialize writes, and report partial or uncertain outcomes
without automatic retries. Pattern switching remains a separate feature until
its activation order and readback have been verified.
