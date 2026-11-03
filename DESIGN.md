# Home control interface

This is a household control panel, not a marketing site. Account and logout are the only header items. No taglines, brand decoration, new navigation, or promotional copy.

## Direction

Calm, precise, slightly warm. Quiet near-neutral surfaces; sage for interaction and acceptance, and sage-to-warm schedule colors representing temperature. The target temperature leads; the ambient temperature is secondary. Schedule timelines are the visual focus, with all days/groups visible.

Keep the system font, tabular temperature figures, mostly 400/500 weights, 4px spacing increments, 16px panel corners, and 44px or larger primary touch controls. Desktop content is capped at 960px to keep a single thermostat and its schedule easy to scan. Support light/dark preferences and small phones.

## Preserve

- Checkmarks mean server acceptance, never verified physical thermostat application. Keep two status icons and their disclosures.
- Keep continuous debounce glow and purposeful progress pulses, with reduced-motion support. These are functional feedback explicitly requested by the user.
- Keep stable authentication/loading geometry and schedule skeletons. No entrance animations that introduce page jumps.
- Schedule is always expanded. All groups/days remain visible. Keep draggable boundaries, hover/drag time labels, inline editors, active-mode marker, and sticky Undo/Discard/Save.
- Keep 06/12/18 helpers inside each timeline; no 00/24 outside labels or redundant instruction paragraph.
- Put secondary diagnostics behind the info controls and refresh beside the relevant reading.

## Tool choice and attribution

Selected [Unslop by Damir Sokolovsky](https://github.com/heydamir/unslop), installed in `.agents/skills/unslop`, for its focused existing-screen audit and sequential visual passes. The skill and its bundled references retain their CC BY 4.0 attribution; application code remains Apache 2.0. The installer copies instructions and optional helper scripts; this pass does not execute those upstream scripts or add a runtime dependency.

Compared with [Impeccable](https://github.com/pbakaus/impeccable), which supplies broader audit/critique commands and a detector runtime, and [Anthropic frontend-design](https://github.com/anthropics/skills/tree/main/skills/frontend-design), which gives broader creative direction. All can guide Svelte/CSS work. Unslop is the smallest workflow needed for this visual experiment; this is a fit judgment, not a claim of objectively superior output.

## Initial audit

The thermostat panel had excessive vertical space, room and target readings competed in size, the dark surfaces carried green throughout, and a global header margin affected the schedule heading. This pass tightens the panel, strengthens the target figure, makes status icons compact, contains color in meaningful places, and scopes header spacing to the page header. Existing interactions and network behavior stay intact.

Checkpoint before this experiment: `ad180ce`. Design branch: `codex/unslop-ui`.
