# Feature backlog

Updated 2026-11-02 from seven Android screenshots and the locally inspected
SmartTherm 128 APK. This is a backlog, not authorization to send live commands.

## Schedule (current focus)

See the [schedule transport investigation](schedule-protocol-investigation-2026-11-02.md)
for HTTPS probe results, captured disabled slots, and the live-save plan.

- [x] Provider-independent Rust schedule model and validated draft preview.
- [x] 24-hour timeline draft editor: select/paint ranges, drag boundaries, precise
  time controls, adjacent-temperature merging, immediate six-change limit, Undo.
- [x] Hover time marker, pending range handles, contextual temperature controls,
  and midnight normalization (retaining real changes across day groups).
- [x] All day/group timelines visible together, per-row hour markers and presets,
  and sticky shared Undo/Discard actions.
- [x] Load the active server schedule on demand through the owner-authorized
  plaintext TCP service. HTTP and HTTPS `getProgrammeByMode` both return 404.
  Login/status/temperature transport remains HTTPS; schedule Save stays disabled.
- [ ] Replace plaintext schedule reads if a supported encrypted route is found.
- [ ] Verify schedule write response and mode/day/disabled-slot semantics, then
  enable saving with an explicit controlled trial, conflict protection, and no
  automatic retries after uncertain outcomes.
- [ ] Resume Auto / cancel temporary override; verify override expiry rules.
- [ ] Read and edit daily, weekday/weekend, and individual-day programmes.
- [ ] Copy a day to selected days, with explicit replacement confirmation.
- [ ] Support target adjustments across all verified programme modes (currently
  real writes require 5+2 mode). Changing programme mode is a device action.
- [ ] Show next scheduled change only after establishing thermostat time zone.

The Android editor supports at most six active periods, prevents deleting the
last one, uses 10-minute time increments, and supports overnight carry-over.
Drafts model changes at a start time; each lasts until the next change. Rust
expands these into complete seven-day views, including Friday/Saturday and
Sunday/Monday boundaries. Do not imply that a scheduled or accepted value is a
physical device acknowledgement.

- [x] Styled pattern choices and previewable, undoable schedule presets scoped
  to the selected day/group; custom hours remain accessible without dragging.

## Everyday controls

- [ ] Auto / Manual / Off (verify frost protection and actual Off semantics).
- [ ] Timed Boost, including cancellation and interaction with current mode.
- [ ] Holiday temperature/duration, including cancellation.
- [ ] Verify whether the heating/flame indicator is sufficiently fresh to show;
  it must not become a button or imply confirmed current relay state.
- [ ] Real device application acknowledgement, only with defensible evidence.

## Device management

- [ ] Multi-device discovery, pagination and selection; currently ambiguous
  accounts are rejected at login instead of choosing a thermostat arbitrarily.
- [ ] Rename a thermostat; optional device metadata in settings.
- [ ] Pair/add and unbind/remove devices. Leave setup in Android initially.
- [ ] Evaluate firmware/reset only if needed; keep maintenance off the main page.

## Account settings (lower priority)

- [x] Existing OCSTAT account sign-in, account email display, browser logout.
- [ ] Account nickname change (distinct from device rename).
- [ ] Password change.
- [ ] Account deletion: retain Android flow until explicitly requested.

Do not reproduce the avatar/category landing page, branding, Feedback or About
screens just for parity. “Heating and Cooling” is category navigation, not proof
this thermostat supports cooling.

## Evidence and release boundary

APK: `cn.com.ocstat.homes`, version 128/code 136. Relevant classes:
`ScheduleAddFragment`, `EditAddScheduleActivity`, `HeatAddFragmentActivity`,
`ControlAddFragment`, `ProgrammeBean`, `SettingActivity`, `DeviceSettingActivity`.
The APK's write shape is `day` plus `programme` pairs of minutes and integer
tenths, with unused slots offset by 2880. Day codes 1–7 are individual weekdays,
8/9 are weekdays/weekends, and 10 is every day. The translation is isolated in
Rust and fixture-tested; it is not yet verified by a schedule write experiment.
Raw captures, account identifiers and credentials stay outside Git.
