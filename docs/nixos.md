# NixOS / HomeServer integration

The flake builds a Linux executable with the Svelte frontend embedded. It exports
`packages.<system>.default` and `nixosModules.default` for x86_64/aarch64 Linux.
Cargo and npm dependencies come from the committed lockfiles; `flake.lock` pins
Nixpkgs. No Node runtime, database, password file or OCSTAT credentials belong in
the Nix configuration/store.

## Build before activating

On a Linux machine with Nix:

```sh
nix build .#default
```

For a local checkout containing newly created, untracked Nix files, use
`nix build path:.` until those files have been committed. The package runs Rust
and frontend tests and Svelte checks as part of its build. The source files are
explicitly selected so captures, secrets, `.env` files and local build outputs
are not included in the package source.

## Add to HomeServer

After the integration changes are committed and available at your chosen Git
revision, add a pinned input to HomeServer's `flake.nix`:

```nix
inputs.smartthermweb.url = "github:Noot-tooN/SmartThermWeb/<commit>";
```

Keep SmartThermWeb's Nixpkgs input independent of HomeServer's older system
Nixpkgs; its current frontend and Rust dependencies need modern build tools.
Pass `inputs` through HomeServer's `outputs` and `specialArgs` (preserving the
existing arguments):

```nix
outputs = inputs@{ nixpkgs, nixpkgs-unstable, home-manager, ... }: {
  # Within your existing nixosSystem:
  # specialArgs = { inherit inputs pkgsUnstable; };
};
```

Copy `nix/homeserver-example.nix` into the HomeServer modules directory and add
its path to the existing `nixosConfigurations.desktop.modules` list. It imports
the service module, selects this flake's package, and adds a Caddy virtual host:

```nix
services.smartthermweb = {
  enable = true;
  package = inputs.smartthermweb.packages.x86_64-linux.default;
  publicOrigin = "https://heating.homeserver.com";
  port = 3000;
};
```

`heating.homeserver.com` is the proposed browser address. The existing HomeServer
DNS wildcard already covers `homeserver.com`; verify that the client uses that
DNS resolver. The example uses `tls internal`, like the existing Caddy sites.
Clients must trust the existing Caddy CA. Do not disable certificate validation.
No application port is opened in the firewall. Keep the existing LAN/Tailscale
access policy; this module does not publish the service on the public internet.

The Rust listener stays on `127.0.0.1`. Caddy must preserve the browser's **Host**
and **Origin** headers (its normal HTTP reverse-proxy behavior). Set
`SMARTTHERM_PUBLIC_ORIGIN` to the exact HTTPS origin users will visit; the NixOS
module sets it from `publicOrigin`. A different hostname/port, HTTP origin or
cross-site edit is rejected. Forwarded headers cannot override this policy.
The example flushes SSE immediately and allows long schedule-save responses.

Build the HomeServer configuration before switching:

```sh
sudo nixos-rebuild build --flake .#desktop
# Activate only when ready:
sudo nixos-rebuild switch --flake .#desktop
systemctl status smartthermweb
journalctl -u smartthermweb -n 50
curl --fail http://127.0.0.1:3000/api/health
```

Then open `https://heating.homeserver.com`, sign in with the existing OCSTAT
account, and check that readings and live feedback load. Login/logout can be
tested without changing the thermostat. Any real target/schedule write is an
explicit user action.

## Runtime behavior

- Hosted session cookies are Secure, HttpOnly and SameSite=Strict. The backend
  holds the OCSTAT token in memory. Passwords are used only for browser login.
- Browser sessions expire after 12 hours; restart/reboot requires signing in
  again. There is no unattended re-login or persisted credentials.
- The thermostat's stored schedule continues independently when this dashboard
  is stopped. The service never replays edits on restart.
- Unverified schedule-save tracking is memory-only. After a restart during an
  edit, reload and inspect the server schedule before editing again.
- The service runs as a restricted dynamic user without a writable home/state
  directory. The temporary schedule-trial feature is not enabled.
- Login, status and write requests use HTTPS. Schedule read-back still uses the
  previously authorized plaintext manufacturer TCP service on port 8800; hosting
  the browser over HTTPS does not encrypt that upstream connection. Outbound DNS,
  HTTPS and TCP 8800 must work. This packaging does not introduce another fallback.
- SIGTERM initiates graceful shutdown. systemd allows 400 seconds for active
  requests before termination; this is not a guarantee of device application.

For local Mac development, leave `SMARTTHERM_PUBLIC_ORIGIN` unset. Existing
loopback HTTP and Vite behavior remains available. Hosted simulator mode is
rejected because the simulator does not authenticate requests.

Build approach follows the official Nixpkgs
[Rust packaging](https://nixos.org/manual/nixpkgs/unstable/#rust) and
[importNpmLock](https://nixos.org/manual/nixpkgs/unstable/#javascript-buildNpmPackage-importNpmLock)
documentation. Proxy options follow the [Caddy reverse-proxy reference](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy).

## Preparation verified on 2026-11-03

Built the pinned flake on HomeServer (`x86_64-linux`) in an isolated temporary
source directory. Frontend build/checks and 130 tests passed; Rust's 106 tests
passed, and local Clippy/format checks passed. The module and Caddy example
were evaluated in a minimal test NixOS configuration with no failed assertions.
The resulting Linux binary passed a temporary loopback smoke test for embedded
assets, health, authentication requirements, origin rejection and Secure logout
cookies. No OCSTAT login or thermostat command was used. This verifies packaging
and local request handling; the real HTTPS endpoint and restricted systemd
service still need checking after the owner chooses to activate the integration.
