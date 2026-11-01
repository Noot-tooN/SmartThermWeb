# Android capture guide: OCSTAT authentication and status

Goal: establish the real app's login, session reuse/refresh, and device-read behavior before implementing authentication. This round needs no temperature, mode, or schedule changes.

## Set up PCAPdroid

1. Record the SmartTherm app version/build, Android version, capture start time/timezone, and whether the phone uses Wi-Fi or mobile data. Keep the phone clock automatic.
2. Set PCAPdroid's **App Filter** to SmartTherm by OCSTAT before starting. Capture all traffic from that app rather than filtering to the known hostname: login may use another host. Keep HTTP, HTTPS, DNS, and TCP 8800, including any other ports the app uses.
3. Enable **Full payload** and a local PCAP/PCAPNG file dump. The connection list alone is insufficient: we need complete requests and replies. Start capture before launching/restarting the app so the beginning of the persistent TCP connection is included.
4. Start with TLS decryption off to observe the app's normal behavior. Previously observed HTTP should already be readable. HTTPS hostnames and connection metadata can still be useful without decrypted bodies.
5. If login uses TLS and bodies are unavailable, save that baseline and report it. PCAPdroid has an optional TLS-decryption workflow, but Android certificate trust/pinning may prevent it. Do not downgrade traffic, disable certificate validation, or modify the app as part of this first capture.

Official instructions: [capture/filter/export](https://emanuele-f.github.io/PCAPdroid/quick_start.html) and [payload inspection/TLS decryption](https://emanuele-f.github.io/PCAPdroid/tls_decryption.html).

## Separate captures

| File label | App actions | Evidence needed |
| --- | --- | --- |
| `01-login` | From logged out, start capture, launch, log in normally, open the device list, select the thermostat, wait 60–90 seconds. If currently logged in, capture the normal logout first and note its time. | Login host/path/method, field names and encoding, response and cookies/tokens, initial list/status/online calls, TCP 8800 authentication/subscription. |
| `02-session-reuse` | Without logging out or clearing app data, force-stop SmartTherm, start a new capture, reopen it, open the thermostat. Then background it for 10–15 minutes and return while capturing. | Whether the app reuses a token, refreshes it, or sends credentials again; whether reopening silently logs in. |
| `03-reconnect` | While capturing the device screen, disconnect the phone's network briefly (about 20 seconds), reconnect, and refresh the device page. Keep the thermostat/gateway powered and connected. | HTTP retry behavior, TCP reconnect/authentication, token reuse, and the distinction between phone disconnection and thermostat offline state. |
| `04-later-session` | Keep the account logged in. Start capture before reopening after several hours or overnight, then open/refresh the device page. If an expiry timestamp/TTL is discovered, capture across that boundary instead. | Natural expiry response, refresh or re-login request, replacement token/cookie, and retried status request. |
| `05-device-status` | Open the thermostat and use its read-only refresh action 2–3 times over a few minutes. At noted times, read room temperature, target, and mode on the physical thermostat and in the app. | Raw units, mode values, freshness/timestamps, online-state meaning, polling versus TCP push, correspondence to the physical display. |

No fixed wait guarantees token expiration. An unchanged token after an overnight capture means only that no refresh was observed during that interval. Force-stop, logout, and network reconnect are not substitutes for expiration. Do not change the clock or deliberately generate repeated failed logins.

Optional later captures: normal logout followed by login to see invalidation/rotation; if a second device is available, login there and then refresh on the first to investigate concurrent-session behavior. Note that the latter may sign the first app out. No account reset or app-data clearing is needed.

Read-only captures cannot prove that a future write was physically applied, or that a server response is never cached. That will need a separately approved write experiment later.

## What to retain in the evidence

Keep a small action timeline next to each capture, for example:

```text
11:30:00 capture started (Europe/Belgrade)
11:30:08 tapped Log in
11:30:10 device list appeared
11:30:15 opened thermostat
11:30:40 refreshed; physical room=20.8 C, target=21.5 C, mode=manual
11:31:15 capture stopped
```

For each relevant exchange retain method, scheme, hostname, port, path, query/form/JSON field names and types, response status, content type, response body structure, redirects, and cookie attributes. Preserve exact spelling, including `current_temprature`.

Pay attention to `Authorization`, `Cookie`, `Set-Cookie`, `token`, any refresh-token or expiry fields actually present, `userid_phoneid`, `user_id`, `device_id`, `messageId`, device revisions/timestamps, and error bodies. Do not assume tokens are JWTs or that refresh exists. Preserve initial TCP 8800 payloads and later heartbeats/push messages, even when HTTP seems sufficient.

For sanitized examples, use consistent aliases such as TOKEN_A/TOKEN_B, COOKIE_A, USER_A and DEVICE_A so changes and reuse remain visible. Preserve JSON types and record token equality/format/length separately without retaining usable secrets. A password hash or obfuscated password may itself be a reusable credential; redact it too.

## Handling captures

A login capture can contain the password, session tokens, cookies, personal identifiers and TCP authentication material. A decrypted PCAPNG can also embed TLS session keys; a separate `.keylog` is equally sensitive. Keep originals in a private local directory outside the SmartThermWeb checkout, and encrypt retained backups with your existing age setup. Do not post them to a public issue, online HAR analyzer, or Git repository. Keep decryption identities/recovery words separate.

For analysis, provide the local path so evidence can be inspected locally; only reviewed sanitized request/response fixtures should enter the source repository. Raw captures and secrets must never be committed.

## HTTPS verification after discovering login

The current credential-free probes already verified TLS on the observed host and a distinct access-denial response at the status path. Next, identify the actual login host/path from capture. Verify that host's certificate and HTTPS endpoint without credentials first, inspect redirects, then agree a credential-handling method before making authenticated read-only requests. Never follow a credential-bearing request onto HTTP. Check that status responses belong to the correct device and report useful current state. TCP 8800 transport must be assessed separately; HTTPS on port 443 says nothing about that connection.
