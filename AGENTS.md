# SmartThermWeb

Single Rust crate (Axum/Tokio) and Svelte 5 runes frontend in web/.

- Keep temperatures as integer tenths; simulator target bounds are 50–300, step 5.
- Cloud acceptance is not device confirmation. Preserve desired state across errors.
- Only the controller worker calls the provider. Keep one pending target, coalescing newer requests.
- Revisions reject stale events; server session IDs distinguish restarts.
- Keep OCSTAT fail-closed until sanitized protocol evidence and explicit live-write approval exist.
- Never log/commit credentials, identifiers, raw packet captures, or unsanitized payloads.
- No real thermostat commands, commits, or pushes without explicit user instruction.
- `make setup`, `make test`, `make check`, `make build`; `make dev` runs both development servers.
- Use deterministic Tokio/Vitest clocks for lifecycle tests. Test uncertain outcomes and late reconciliation.
- Keep dependencies small. No schedules, database, auth system, Nix, or extra infrastructure in this milestone.
- Preserve the Apache 2.0 license.
- CodeGraph is not initialized here. Ask before creating an index; use native tools until then.
