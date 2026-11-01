# OCSTAT capture analysis — 2026-11-01

## Scope and handling

One user-supplied PCAPdroid file was inspected offline from the private capture directory outside Git. Raw credentials, tokens, cookies, addresses, and account/device identifiers are omitted here. During the offline analysis, no captured request was replayed or manufacturer request made. A subsequent explicitly authorized HTTPS login and read-only device-list test succeeded; see [the live verification record](ocstat-protocol.md#authorized-https-login-and-read-only-verification--2026-11-01). No thermostat write request was made. Authentication implementation was a subsequent, separately requested step; see the protocol notes.

The recorded packets span **20.336 seconds**: 120 packets, including 118 TCP and 2 UDP. There are nine complete HTTP request/response pairs on port 80 and three connections carrying client application data on TCP 8800. No port-443 TCP traffic appears. The offline parser found no truncated packets, TCP reassembly gaps, or unparsed HTTP bytes in these exchanges. This describes the supplied capture, not traffic outside its recording/filter scope.

## Timeline

Offsets are seconds after the first captured packet. Near-simultaneous events are shown together; user actions cannot be reconstructed reliably without an action log.

| Offset | Captured activity |
| --- | --- |
| +0.637–0.639 | Two `queryClientTcpOnline` requests, one `getDeviceStatus`, and TCP 8800 registration/subscription-shaped messages |
| +1.581 | Another `queryClientTcpOnline` |
| +1.826 | `getDeviceListNew` |
| +8.688 | `getHeadPic` |
| +10.931 | First successful `login` |
| +12.585 | Another TCP 8800 registration-shaped message |
| +18.658 | Second successful `login` and another `getHeadPic` |
| +20.336 | Another TCP 8800 registration-shaped message |

The status/list requests occur **before** both captured logins. There is no post-login device-list/status exchange in this file. The capture therefore demonstrates token equality across these requests, but does not establish a complete fresh-login-to-device-read flow or what the app persists across force-stop/reboot.

## Login and session evidence

- Observed endpoint: `POST http://www.ocstat-thermostat.com/ocstat/login`.
- Encoding: `application/x-www-form-urlencoded`; fields: `email`, `password`.
- Both responses are JSON with `statu: true` and `error_code: 0`.
- The `user` object contains `id`, `username`, `token`, `token_invalid_time`, `loginTime`, `smartAccount`, `smartPwd`, `master`, `email`, `remoteAddr`, and `clientType`.
- Both logins return the **same token**, also used by the earlier authenticated reads and all three TCP registration-shaped messages. Token equality does not prove indefinite validity, renewal, or logout behavior.
- `token_invalid_time` is an empty string in both responses. No refresh-token field or refresh endpoint is observed.
- Login responses do not set a cookie. Captured API requests use no `Authorization` or `Cookie` header; authenticated reads carry `token` and identity fields in the form body.
- The response includes a nonempty `smartPwd` credential-like field, different from the submitted `password`. Do not persist or log the entire user response. Whether the submitted password is transformed by the app is not established by this capture alone.

The app transmits the password field and token over unencrypted HTTP. TCP registration also exposes the token. This is distinct from the earlier credential-free probe proving that the hostname accepts certificate-validated HTTPS: authenticated login/read compatibility was not established by the capture itself. The subsequent authorized test confirmed HTTPS login and device listing, as recorded in the protocol notes.

## Read endpoints

All paths below are under `/ocstat/`, with POST and form encoding.

| Endpoint | Request fields | Observed response |
| --- | --- | --- |
| `getDeviceListNew` | `userid_phoneid`, `user_id`, `pageSize`, `online`, `device_type`, `pageNum`, `token` | JSON, `statu: true`, `error_code: 1`, success message, `tcpCode: 0`, `devicesCount: 1`, one device |
| `getDeviceStatus` | `userid_phoneid`, `device_id`, `user_id`, `token` | HTTP 200 with `text/json;charset=UTF-8`, but the body is a **60-byte custom text frame beginning `##3cL`**, not JSON; contains the device identifier |
| `queryClientTcpOnline` | `tcp_id`, `userid_phoneid`, `user_id`, `token` | All three return `statu: false`, `error_code: -2`, `tcpCode: 0`, `message: "client tcp fail."` |
| `getHeadPic` | `email` | HTTP 200 HTML resembling a web login page, not an avatar/API JSON response; sets `JSESSIONID`, which is not sent in subsequent captured API requests |

Login and device listing use **different `error_code` values on successful responses**. Do not implement a global “zero means success” rule. HTTP status and advertised content type are likewise insufficient for decoding or success classification.

In all TCP-online queries, `tcp_id` equals `userid_phoneid` and differs from the listed thermostat's `device_id`. Meanwhile, the list reports the device as online. This supports interpreting this check as app/client TCP connectivity; it must not automatically mark the thermostat offline. The construction and lifetime of `userid_phoneid` remain unknown.

### Device-list observations

| Field | Observed value/type | Interpretation |
| --- | --- | --- |
| `online` | Boolean `true` | Server-reported device online; freshness unverified |
| `current_temprature` | String `"215"` | Consistent with target 21.5 °C under the brief's tenths convention; needs physical-display comparison |
| `inside_temparature` | String `"211"` | Consistent with room 21.1 °C under the same convention; needs physical-display comparison |
| `presetsTemp` | String `"70|100|200"` | Delimited values; meaning unverified |
| `program_mode`, `work_mode`, `th_work` | Each string `"1"` | Enum meanings unverified |
| `serviceTime`, `deviceTime` | Strings | Timezone, units, and freshness semantics unverified |

The device object also includes device/model/version fields, display/lock/gateway state, upgrade state, command types, and a `password` field. Treat the latter as sensitive. Do not infer transport security from the device's `encryption` flag.

## TCP 8800

Three connections send the same 59-byte frame with the sanitized structure:

```text
##3b1<APP_CLIENT_ID>01<TOKEN>
```

The first also sends a 17-byte frame:

```text
&&111<DEVICE_ID>
```

These resemble authentication/registration and device subscription messages, but their semantics are not proven. The `3b` and `3c` prefixes coincide with hexadecimal byte lengths 59 and 60; this is a framing hypothesis only. No server application payload is captured on these connections, so this file cannot establish successful registration, push updates, heartbeats, or device acknowledgement semantics. TCP acknowledgements alone would not prove application acceptance.

## Implications and next evidence

1. **HTTPS (follow-up completed):** an explicitly authorized HTTPS login and device-list request succeeded with certificate verification enabled and redirects blocked. Account/device identities matched the capture; no write request or HTTP fallback occurred. Other authenticated endpoints and TLS support on port 8800 remain unverified.
2. **Session reuse:** record 60–90 seconds after a normal login while opening the thermostat, then force-stop/reopen without logout or clearing data and open it again. One combined file is fine; add an action timeline. This should capture post-login reads and distinguish cached-token reuse from silent password login.
3. **Expiry:** capture a later reopening after several hours/overnight. An unchanged token still does not prove no expiry. Natural rejection/renewal evidence is needed before implementing refresh/re-login policy.
4. **Status:** compare app values with the physical display at noted times and retain longer TCP traffic. Decode the custom status frame only with evidence for field boundaries and meaning. Neither cached list values nor a cloud response yet proves that a future write was physically applied.
5. **Credential persistence:** encrypted password provisioning plus an in-memory token and re-login on restart remains a reasonable candidate. The capture establishes a login exchange, but not token longevity or safe automatic retry semantics. Persisting a token is optional and would itself require secret handling.

The subsequent OCSTAT implementation supports HTTPS login and read-only listing; writes remain disabled. No write experiment or commit is part of this analysis.
