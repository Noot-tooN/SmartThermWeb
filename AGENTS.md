# SmartThermWeb

Single Rust crate (Axum/Tokio) and Svelte 5 runes frontend in web/.

- Keep temperatures as integer tenths; target bounds are 50–350 (matching the inspected Android control screen), step 5.
- Cloud acceptance is not device confirmation. Preserve desired state across errors.
- Only the controller worker calls the provider. Keep one pending target, coalescing newer requests.
- Revisions reject stale events; server session IDs distinguish restarts.
- Keep OCSTAT fail-closed until sanitized protocol evidence and explicit live-write approval exist.
- Never log/commit credentials, identifiers, raw packet captures, or unsanitized payloads.
- No real thermostat commands, commits, or pushes without explicit user instruction.
- `make setup`, `make test`, `make check`, `make build`; `make dev` runs both development servers.
- Use deterministic Tokio/Vitest clocks for lifecycle tests. Test uncertain outcomes and late reconciliation.
- Keep dependencies small. Schedules are now in scope: keep the browser model independent of OCSTAT encoding; gate live schedule writes until verified and explicitly approved. No database, new auth system, Nix, or extra infrastructure.
- Preserve the Apache 2.0 license.
- CodeGraph is not initialized here. Ask before creating an index; use native tools until then.

## Frontend design

- For visual UI changes, use the project-local `.agents/skills/unslop/SKILL.md` and read `DESIGN.md` first.
- Product requirements in `DESIGN.md` override generic visual rules: preserve useful debounce/status motion, stable loading geometry, accessible controls, and the timeline editor.
