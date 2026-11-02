# Installed Android decoder analysis — 2026-11-01

## Provenance and scope

The owner authorized copying and inspecting the installed SmartTherm APK over USB. ADB identified package `cn.com.ocstat.homes`, version name `128`, version code `136` (min SDK 21, target SDK 35). Package-manager inspection returned one APK, with no split packages. The 32,392,472-byte APK was copied to `/Users/nirdala/Downloads/SmartTherm-apk/SmartTherm-128.apk`; ZIP integrity validation passed. SHA-256: `4b4fc51776b8ec30e0e677567de8c666a28d3c38e3440329b404899572e7508d`.

Only the installed package was copied, not app-private data. No application was launched, no OCSTAT request was made, and no thermostat setting was changed. The owner was told they could unplug as soon as the copy and integrity check completed.

Static analysis used official JADX 1.5.6, downloaded from its GitHub release and checked against the release asset's SHA-256. The APK was not executed. Whole-app decompilation reported 102 errors, so these findings are limited to the inspected, recovered code paths. The key `L`/`N` field assignments were also checked using JADX's simplified instruction output, which avoids relying on its warned-about string-switch reconstruction. Six relevant classes and the simplified decoder are saved outside Git under `/Users/nirdala/Downloads/SmartTherm-apk/analysis/`.

## Device-specific path

The original capture's decoded device type is `22`. `HeatDevicesListActivity.onItemClickList` routes that type to `HeatAddFragmentActivity`, which uses `ControlAddFragment`. Findings below follow that path rather than assuming helpers for other thermostat models have identical meanings.

Source references, relative to `analysis/sources/`:

- `cn/com/ocstat/homes/activity/HeatDevicesListActivity.java:238`
- `cn/com/ocstat/homes/utils/ConstantsAPI.java:328`

## `N` is a gateway message containing device-status entries

`DispatchPushMessage.dispatchMessage` parses a ten-character gateway ID at `[5:15]` and, for `N`, entries of 18 characters. For the first entry:

| Offset | Android interpretation |
| --- | --- |
| `[15:16]` | Device index; selected device ID is gateway ID + literal `0` + this index |
| `[16:17]` | Online flag (`1` means online) |
| `[17:18]` | `th_work` |
| `[18:21]` | `inside_temparature`, hexadecimal integer tenths |
| `[21:23]` | `autoTemp`, hexadecimal half-degree count multiplied by 5 |
| `[23:25]` | Device type (`22` in this capture) |
| `[25:27]` | `disOnOff` |
| `[27:33]` | `reservedText` |

Subsequent entries shift the per-device offsets by 18 characters. There is no message-ID extraction in this `N` branch. The previous provisional interpretation of `[5:17]` as one device identifier was incorrect: it included the online flag and omitted the inserted `0` used by the app. Replaying this mapping offline against all four original `N` frames correctly reconstructs the selected device ID every time, without printing or saving identifiers.

References: `cn/com/ocstat/homes/push/DispatchPushMessage.java:275`; `analysis/DispatchPushMessage.simple.java:313`.

## The delayed `th_work` flag drives the heating indicator

The decoder places `N[17:18]` and `L[18:19]` into the same `th_work` field. In `ControlAddFragment.resetPageData`, `th_work == "0"` clears `binding.heatingRun`; any other value sets it. This establishes what Android uses the flag for: its heating-running indicator. It is not consulted by the temperature-command completion handler.

Offline replay of the original `N` frames:

| Capture time | `autoTemp` | Heating indicator | `disOnOff` |
| --- | ---: | --- | --- |
| +5.263 s | 21.5°C | Off | `03` |
| +12.260 s | 22.5°C | Off | `03` |
| +34.292 s | 22.5°C | On | `13` |
| +39.308 s | 21.5°C | On | `13` |

The delayed transition is therefore a change in the status Android presents as heating activity, not a per-command target acknowledgment. Static app code alone does not establish when the physical relay actually operated or what caused the display delay.

References: `DispatchPushMessage.java:289`, `:436`; `cn/com/ocstat/homes/fragment/heat/add/ControlAddFragment.java:483`.

## `disOnOff` is not used as completion evidence for type 22

The decoder assigns `N[25:27]` and `L[38:40]` to `disOnOff`. The type-22 activity copies it from `L` into its model, but its temperature success handler does not inspect it. The shared model contains product-specific interpretations (including hot-water-style helpers and US system/fan bitfields); importing those names into type 22 would be unjustified. The exact meaning of its high-bit change from `03` to `13` for this model remains unestablished.

References: `DispatchPushMessage.java:297`, `:449`; `HeatAddFragmentActivity.java:1105`; `cn/com/ocstat/homes/bean/ThermostatBean.java:504`, `:1302`.

Do not indiscriminately retain all remaining bytes as harmless telemetry: the shared model can construct `reservedText` from gateway-standby and password-related fields (`ThermostatBean.java:485`). Future diagnostic exports should continue to allowlist known non-sensitive fields.

## Android's completion path uses the early `L` reply

1. `ControlAddFragment.modifyTemperature` creates a message ID, stores it in `modifyDeviceMsgIdMap`, and submits `setThermostatWorkModeNew`.
2. The `L` decoder extracts the message ID from `[56:60]`.
3. The type-22 activity receives the `L`, checks the device ID, updates its displayed state, and passes the message ID to `ControlAddFragment.hanldDeviceReply`.
4. A matching pending ID is removed and `hanldRequestResult(true, ...)` is called, dismissing the loading indicator and handling the successful operation. There is no extra physical-applied flag check in that path.

HTTP success alone also schedules a status refresh after 2.5 seconds if that message ID is still pending. The app's HTTP/TCP response timeout constant is 6 seconds. These client-side timers do not establish a physical-device deadline or explain the observed 43-second display delay.

References: `ControlAddFragment.java:990`, `:1035`, `:1239`; `HeatAddFragmentActivity.java:1101`, `:1129`; `DispatchPushMessage.java:454`; `ConstantsAPI.java:84`.

The original captured `L` frames echo the corresponding write IDs about 0.4 seconds after each write while retaining the previous `autoTemp` value. Thus the method's name “device reply” must not be taken as proof that the physical thermostat has applied the target. It is the same early evidence already shown inadequate by the physical-observation experiments.

Other frame families exist in the dispatcher, including `O` with a command and message ID. No `O` frame appears in the original capture, and it is not the completion path for the inspected type-22 temperature screen. This analysis does not establish that no acknowledgement exists anywhere in the wider protocol or firmware.

## Correction: `autoTemp` is not defined as a physical reported-target acknowledgment

Both `L[36:38]` and `N[21:23]` populate `autoTemp`. For type 22, incoming `N` values update that field in the screen's model only when work mode is `0`; the control screen uses it for automatic operation. Work mode `1` is explicitly displayed as temporary mode, consistent with the owner's screenshot. The separate `current_temprature` field is populated by `L[19:21]`.

These names and uses do not prove whether the cloud's `autoTemp` originates at a gateway, thermostat, or server cache. They do mean the previous “device-reported target” interpretation was too strong. Cloud field agreement should continue to be described only as an observation; it must not produce a physical synchronization checkmark.

References: `HeatAddFragmentActivity.java:931`; `ControlAddFragment.java:588`; `DispatchPushMessage.java:293`, `:437`, `:448`.

## Conclusion

The APK resolves the heating flag, device routing, automatic-temperature field, and the app's request-success path. It does not reveal a separate verified physical-application acknowledgement for this device's temperature changes. The normal web app remains unchanged and keeps thermostat synchronization unverified. Explaining delivery versus firmware/display timing still requires device/gateway-side evidence or manufacturer protocol information; repeating target-only cloud captures cannot establish that distinction.

## Gateway identification follow-up

The two existing capture device lists contain `device_type=22`, `device_class=1`, `gatewayType=1`, and `device_version=010000`. Neither list contains `device_mac` or `device_ipAddress`. These are protocol classifications, not an established retail gateway model.

The APK's `DeviceSettingActivity.getDeviceDate` calls `/ocstat/getDeviceSettingInfo` with token, user ID, and device ID; its result handler replaces the device model and displays MAC, IP, serial/device identifier, and firmware version. References: `DeviceSettingActivity.java:182`, `:190`, `:445`; `ThermostatBean.java:400`, `:433`.

A single read-only HTTPS request to that endpoint succeeded with certificate verification enabled and redirects disabled, using the existing captured session token. No fresh login, plaintext fallback, or thermostat change was sent. The response matched the selected device and returned `device_type=22`, `superDeviceType=02`, `gatewayType=1`, and current/available version `010000`. It supplied a nonzero, correctly formatted MAC address and a globally routable IPv4 address. The public address cannot be used as the device's LAN address. The MAC had no match in the Mac's existing ARP cache; no active discovery or scan was performed. This absence does not establish offline status. Only the network identifiers needed for local matching were stored outside Git in an owner-readable temporary file; raw responses and credentials were not saved.

The binding code has separate entries for receiver types `27` (`Receiver-2.4G`) and `52` (`Receiver-5G`), with `gateway2701` artwork, and type `54` (`2701ACW`). The inspected metadata does not establish a mapping from the selected type-22 device or `gatewayType=1` to one of those receiver models. `superDeviceType=02` likewise must not be treated as a hardware model identification. References: `bind/ConfigMode.java:48`, `:84`.

The useful next step is matching the returned MAC against the router's connected-client/DHCP list, then checking capture capability on the router or access point. A router-side IP capture would observe the network module's cloud connection, not necessarily a separate receiver-to-thermostat radio link. Capturing ordinary traffic can establish cadence and connection behavior without changing thermostat settings, but cannot guarantee a command-specific delay diagnosis without observing a command.

### Clarification of push message purposes

`P` is explicitly named `TCP_MSG_TYPE_SHEDULE_DOUBLE_DAY` and decoded into schedule day, next day, count, program mode, times, temperatures, and message ID (`ConstantsAPI.java:160`; `DispatchPushMessage.java:344`). The three unchanged `P` frames in the original adjustment capture are schedule data, not evidence of target application. `L` supplies fuller control/status information and correlation; `N` supplies compact gateway/device status including online, heating, room temperature, and `autoTemp`. These update the app's view of state; neither the decoder nor our timing observations establish that each field is fresh physical thermostat state. A correlated early `L` could originate at an intermediate device or server; its precise producer remains unverified.


## Temperature range follow-up — 2026-11-02

Offline inspection of the same version-128 APK establishes the **Android type-22 control-screen range as 5–35°C inclusive in 0.5°C steps**. No network requests or thermostat commands were sent, and the web app's existing 5–30°C policy limit was not changed.

Evidence chain:

- `ControlAddFragment.onCreateView` inflates `FragmentHeatControlBinding`, whose `inflate` uses `R.layout.fragment_heat_control`.
- `cn.com.ocstat.homes.view.SeekArc` initializes `mStartTemp=5.0f`, `step=0.5f`, and `mMax=60` in all constructors. `getCurrentTemp(progress)` returns `(progress * step + mStartTemp) * 10` as integer tenths. Thus progress 0 gives 50 (5°C), and progress 60 gives 350 (35°C).
- The widget can take a `max` XML/style attribute (`R.styleable.SeekArc_max` index 4), but `res/layout/fragment_heat_control.xml` specifies no maximum, and the decoded styles contain no `seekArcStyle` or `max` override. The manifest selects `HeatAppTheme` for `HeatAddFragmentActivity`. Only one resource variant of `fragment_heat_control.xml` exists in this APK.
- `ControlAddFragment`'s plus handler refuses progress above `getMax()`; its minus handler refuses progress below zero. Both convert through `getCurrentTemp(progress)` and call `updateTemp`. Its dial callback uses the same conversion. This fragment only uses the single-argument `setData` method, which does not alter the bounds; no runtime range override was found on the inspected type-22 control path.
- `SeekArc.mEndTemp=32` is an unused field in this class; it does not define the maximum accepted progress or temperature. Other model/schedule screens use other widget configurations and must not be substituted for this path.

Source references: `ControlAddFragment.java:124`, `:191`, `:371`, `:382`; `FragmentHeatControlBinding.java:102`; `SeekArc.java:102`, `:119`, `:168`, `:527`, `:545`, `:566`; `res/layout/fragment_heat_control.xml:126`. The fresh single-class decode and decoded resources are in `/tmp/smarttherm-range/`; the original APK remains under the private Downloads directory documented above.

This is static evidence for the official client's allowed values, not physical endpoint testing or a claim that firmware/server enforcement has been exhaustively verified. The previously owner-confirmed HTTPS round trip remains 21.5 → 22.5 → 21.5°C. The web limit is a narrower subset of the Android control range.


Following this inspection, the owner authorized matching the Android range. The web controls, browser clamp, Rust target validation, simulator, and API now permit 5–35°C inclusive in 0.5°C steps. Boundary tests cover 5°C, crossing the former 30°C cap, 34.5 → 35°C, and rejection above 35°C. No live thermostat command was sent for this range change.
