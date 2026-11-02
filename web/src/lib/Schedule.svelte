<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { boundaryLabels } from './timelineLabels';
  import {
    clock,
    draftFromPreview,
    groups,
    labels,
    newDraft,
    patternChoices,
    schedulePresets,
    type SchedulePreset,
    normalizeSchedule,
    paint,
    periods,
    moveBoundary,
    type Change,
    type Group,
    type Pattern,
    type Period,
    type Schedule,
  } from './schedule';
  let {
    onExpired = () => {},
    live = false,
  }: { onExpired?: () => void; live?: boolean } = $props();
  let pattern = $state<Pattern>('daily');
  let selected = $state<Group>('every_day');
  let drafts = $state<Record<Pattern, Schedule>>({
    daily: newDraft('daily'),
    weekday_weekend: newDraft('weekday_weekend'),
    weekly: newDraft('weekly'),
  });
  const copy = <T,>(value: T): T => JSON.parse(JSON.stringify(value));
  let baselines = $state<Record<Pattern, Schedule>>({
    daily: newDraft('daily'),
    weekday_weekend: newDraft('weekday_weekend'),
    weekly: newDraft('weekly'),
  });
  let loaded = $state(false);
  let loading = $state(false);
  let loadError = $state('');
  let loadedPattern = $state<Pattern>('daily');
  let loadRequest: AbortController | null = null;
  async function loadSchedule() {
    if (loading || (loaded && dirty)) return;
    loading = true;
    loadError = '';
    const controller = new AbortController();
    loadRequest = controller;
    try {
      const response = await fetch('/api/thermostat/schedule', {
        signal: AbortSignal.any([
          controller.signal,
          AbortSignal.timeout(45000),
        ]),
      });
      if (controller.signal.aborted) return;
      if (response.status === 401) {
        onExpired();
        return;
      }
      if (!response.ok)
        throw new Error('Could not load the schedule. Try again.');
      const preview = await response.json();
      if (controller.signal.aborted) return;
      const draft = draftFromPreview(preview);
      generation++;
      clearTimeout(timer);
      request?.abort();
      validationError = false;
      error = '';
      history = [];
      drafts = {
        daily: newDraft('daily'),
        weekday_weekend: newDraft('weekday_weekend'),
        weekly: newDraft('weekly'),
      };
      drafts[draft.pattern] = draft;
      baselines = copy(drafts);
      pattern = loadedPattern = draft.pattern;
      changeGroup(groups[pattern][0]);
      dismiss();
      loaded = true;
    } catch (caught) {
      if (!controller.signal.aborted)
        loadError =
          caught instanceof Error && caught.name === 'Error'
            ? caught.message
            : 'Could not load the schedule. Try again.';
    } finally {
      if (!controller.signal.aborted) loading = false;
    }
  }
  let selection = $state({
    start: 0,
    end: 1440,
    temperature: 185,
    range: false,
  });
  let presetsOpen = $state(false);
  let presetsAbove = $state(false);
  let popover = $state(false);
  let precise = $state(false);
  let hover = $state<number | null>(null);
  let error = $state('');
  let validation = $state('');
  let validationError = $state(false);
  let history = $state<{ pattern: Pattern; group: Group; changes: Change[] }[]>(
    [],
  );
  const timelines = new Map<Group, HTMLDivElement>();
  let timelineWidths = $state<Partial<Record<Group, number>>>({});
  let hoverGroup = $state<Group | null>(null);
  const dirty = $derived(
    live && loaded
      ? pattern !== loadedPattern ||
          JSON.stringify(drafts) !== JSON.stringify(baselines)
      : JSON.stringify(drafts[pattern]) !== JSON.stringify(baselines[pattern]),
  );
  function registerTimeline(node: HTMLDivElement, group: Group) {
    timelines.set(group, node);
    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(([entry]) => {
            timelineWidths[group] = entry.contentRect.width;
          });
    observer?.observe(node);
    return {
      destroy() {
        observer?.disconnect();
        timelines.delete(group);
      },
    };
  }
  function activate(group: Group) {
    if (selected === group) return;
    changeGroup(group);
    popover = false;
    presetsOpen = false;
    skipClick = false;
  }
  function discard() {
    if (live && loaded) {
      drafts = copy(baselines);
      pattern = loadedPattern;
      history = [];
    } else {
      drafts[pattern] = copy(baselines[pattern]);
      history = history.filter((entry) => entry.pattern !== pattern);
    }
    changeGroup(groups[pattern][0]);
    dismiss();
    presetsOpen = false;
    hover = null;
    checkDraft();
  }
  type Drag = {
    kind: 'range' | 'boundary' | 'selection-start' | 'selection-end';
    pointer: number;
    x: number;
    anchor: number;
    index: number;
    minute: number;
    moved: boolean;
  };
  let drag = $state<Drag | null>(null);
  let skipClick = false;
  let request: AbortController | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  const currentGroup = $derived(
    drafts[pattern].groups.find((g) => g.group === selected)!,
  );
  const blocks = $derived(periods(currentGroup.changes));
  const display = $derived.by(() => {
    if (drag?.kind !== 'boundary' || !drag.moved) return blocks;
    return periods(moveBoundary(blocks, drag.index, drag.minute));
  });
  const times = Array.from({ length: 145 }, (_, i) => i * 10);
  function choose(b: Period) {
    selection = { ...b, range: false };
    error = '';
    popover = true;
    precise = false;
  }
  function changeGroup(group: Group) {
    drag = null;
    selected = group;
    choose(
      periods(
        drafts[pattern].groups.find((g) => g.group === group)!.changes,
      )[0],
    );
  }
  function changePattern(value: Pattern) {
    pattern = value;
    presetsOpen = false;
    changeGroup(groups[pattern][0]);
    popover = false;
    checkDraft();
  }
  function checkDraft() {
    const version = ++generation;
    clearTimeout(timer);
    request?.abort();
    validation = 'Checking draft…';
    validationError = false;
    const body = JSON.stringify(normalizeSchedule(drafts[pattern]));
    timer = setTimeout(async () => {
      const controller = new AbortController();
      request = controller;
      try {
        const response = await fetch('/api/thermostat/schedule/preview', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body,
          signal: AbortSignal.any([
            controller.signal,
            AbortSignal.timeout(8000),
          ]),
        });
        if (version !== generation) return;
        if (response.status === 401) {
          onExpired();
          return;
        }
        const result = await response.json();
        if (version !== generation) return;
        if (!response.ok)
          throw new Error(
            result.error?.message ?? 'Could not check this draft.',
          );
        validation = 'Draft checked';
      } catch (caught) {
        if (version !== generation) return;
        validationError = true;
        validation =
          caught instanceof Error && caught.name === 'Error'
            ? caught.message
            : 'Could not check draft. Your edits are still here.';
      }
    }, 300);
  }
  function commit(changes: Change[], focus: number) {
    normalizeSchedule({
      pattern,
      groups: drafts[pattern].groups.map((g) =>
        g.group === selected ? { group: selected, changes } : g,
      ),
    });
    if (JSON.stringify(changes) === JSON.stringify(currentGroup.changes)) {
      choose(
        periods(changes).find((b) => b.start <= focus && b.end > focus) ??
          periods(changes)[0],
      );
      return;
    }
    history = [
      ...history.slice(-19),
      {
        pattern,
        group: selected,
        changes: currentGroup.changes.map((c) => ({ ...c })),
      },
    ];
    currentGroup.changes = changes;
    choose(
      periods(changes).find((b) => b.start <= focus && b.end > focus) ??
        periods(changes)[0],
    );
    checkDraft();
  }
  function apply(temperature = selection.temperature) {
    try {
      commit(
        paint(blocks, selection.start, selection.end, temperature),
        selection.start,
      );
    } catch (caught) {
      error = (caught as Error).message;
    }
  }
  function adjustTemperature(delta: number) {
    const temperature = Math.max(
      50,
      Math.min(350, selection.temperature + delta),
    );
    if (selection.range) selection.temperature = temperature;
    else apply(temperature);
  }
  function usePreset(preset: SchedulePreset) {
    try {
      commit(
        preset.changes.map((c) => ({ ...c })),
        preset.changes[1]?.start_minute ?? 0,
      );
      presetsOpen = false;
      popover = false;
      hover = null;
    } catch (caught) {
      error = (caught as Error).message;
    }
  }
  function range() {
    presetsOpen = false;
    selection = { start: 420, end: 1320, temperature: 215, range: true };
    popover = true;
    precise = true;
    hover = null;
    error = '';
  }
  function boundary(index: number, minute: number) {
    try {
      commit(moveBoundary(blocks, index, minute), minute);
    } catch (caught) {
      error = (caught as Error).message;
    }
  }
  function adjustTime(edge: 'start' | 'end', minute: number) {
    error = '';
    if (
      minute < 0 ||
      minute > 1440 ||
      (edge === 'start' ? minute >= selection.end : minute <= selection.start)
    )
      return;
    if (selection.range) {
      selection[edge] = minute;
      return;
    }
    const index = blocks.findIndex((b) => b.start === selection.start);
    const start = edge === 'start' ? minute : selection.start;
    const end = edge === 'end' ? minute : selection.end;
    try {
      const changes = moveBoundary(
        blocks,
        edge === 'start' ? index : index + 1,
        minute,
      );
      commit(changes, start);
      selection = {
        start,
        end,
        temperature: selection.temperature,
        range: false,
      };
    } catch (caught) {
      error = (caught as Error).message;
    }
  }
  function undo() {
    const last = history.at(-1);
    if (!last) return;
    history = history.slice(0, -1);
    pattern = last.pattern;
    selected = last.group;
    drafts[pattern].groups.find((g) => g.group === selected)!.changes =
      last.changes;
    choose(periods(last.changes)[0]);
    checkDraft();
  }
  function minuteAt(x: number, group = selected) {
    const rect = timelines.get(group)!.getBoundingClientRect();
    return Math.max(
      0,
      Math.min(1440, Math.round(((x - rect.left) / rect.width) * 48) * 30),
    );
  }
  function begin(event: PointerEvent, kind: Drag['kind'], index = 0) {
    if (event.button !== 0 || !event.isPrimary) return;
    event.stopPropagation();
    hover = null;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    skipClick = false;
    presetsOpen = false;
    if (kind === 'range') {
      const rect = timelines.get(selected)!.getBoundingClientRect();
      const minute = Math.max(
        0,
        Math.min(1439, ((event.clientX - rect.left) / rect.width) * 1440),
      );
      index = blocks.findIndex((b) => b.start <= minute && b.end > minute);
    }
    drag = {
      kind,
      pointer: event.pointerId,
      x: event.clientX,
      anchor: minuteAt(event.clientX),
      index,
      minute:
        kind === 'selection-start'
          ? selection.start
          : kind === 'selection-end'
            ? selection.end
            : blocks[index].start,
      moved: false,
    };
  }
  function move(event: PointerEvent, group = selected) {
    if (!drag) {
      hoverGroup = group;
      hover =
        event.pointerType === 'touch' ? null : minuteAt(event.clientX, group);
      return;
    }
    if (event.pointerId !== drag.pointer) return;
    if (Math.abs(event.clientX - drag.x) < 6 && !drag.moved) return;
    drag.moved = true;
    const minute = minuteAt(event.clientX);
    if (drag.kind === 'boundary')
      drag.minute = Math.max(
        blocks[drag.index - 1].start + 10,
        Math.min(blocks[drag.index].end - 10, minute),
      );
    else if (drag.kind === 'selection-start')
      drag.minute = Math.min(selection.end - 10, minute);
    else if (drag.kind === 'selection-end')
      drag.minute = Math.max(selection.start + 10, minute);
    else drag.minute = minute;
  }
  const drawnRange = $derived.by(() => {
    if (!drag?.moved) return selection.range ? selection : null;
    if (drag.kind === 'range')
      return {
        start: Math.min(drag.anchor, drag.minute),
        end: Math.max(drag.anchor, drag.minute),
      };
    if (drag.kind === 'selection-start')
      return { ...selection, start: drag.minute };
    if (drag.kind === 'selection-end')
      return { ...selection, end: drag.minute };
    return selection.range ? selection : null;
  });
  const anchor = $derived((selection.start + selection.end) / 2 / 14.4);
  function finish(event: PointerEvent) {
    if (!drag || event.pointerId !== drag.pointer) return;
    const finished = drag;
    drag = null;
    if (!finished.moved) {
      if (finished.kind === 'range') choose(blocks[finished.index]);
      return;
    }
    skipClick = true;
    if (finished.kind === 'boundary') boundary(finished.index, finished.minute);
    else if (
      finished.kind === 'selection-start' ||
      finished.kind === 'selection-end'
    ) {
      selection[finished.kind === 'selection-start' ? 'start' : 'end'] =
        finished.minute;
      popover = true;
    } else {
      const start = Math.min(finished.anchor, finished.minute);
      const end = Math.max(finished.anchor, finished.minute);
      if (start < end) {
        selection = { start, end, temperature: 215, range: true };
        popover = true;
        precise = false;
        error = '';
      }
    }
  }
  function cancel() {
    drag = null;
    skipClick = true;
  }
  function clickBlock(b: Period, event: MouseEvent) {
    if (skipClick && event.detail > 0) {
      skipClick = false;
      return;
    }
    choose(b);
  }
  function boundaryKey(event: KeyboardEvent, index: number) {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const minute = blocks[index].start + (event.key === 'ArrowLeft' ? -30 : 30);
    boundary(index, minute);
  }
  function selectionKey(event: KeyboardEvent, edge: 'start' | 'end') {
    if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
    event.preventDefault();
    adjustTime(edge, selection[edge] + (event.key === 'ArrowLeft' ? -30 : 30));
    popover = true;
  }
  function dismiss() {
    popover = false;
    precise = false;
    if (selection.range) {
      selection = { ...blocks[0], range: false };
    }
  }
  function outside(event: PointerEvent) {
    if (
      !(event.target instanceof Element) ||
      !event.target.closest('.preset-picker')
    )
      presetsOpen = false;
    if (
      !(event.target instanceof Element) ||
      event.target.closest('.timeline-area')
    )
      return;
    dismiss();
  }
  function escape(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    presetsOpen = false;
    drag = null;
    hover = null;
    dismiss();
  }
  onMount(() => {
    if (live) void loadSchedule();
  });
  onDestroy(() => {
    loadRequest?.abort();
    generation++;
    clearTimeout(timer);
    request?.abort();
  });
</script>

<svelte:window onpointerdown={outside} onkeydown={escape} />
<section
  class="schedule-panel"
  aria-labelledby="schedule-title"
  aria-busy={live && (loading || (!loaded && !loadError))}
>
  <header class="schedule-heading">
    <h2 id="schedule-title">
      Schedule <small
        title="Drafts are cleared when you leave or refresh. No thermostat settings are changed."
        >Draft only</small
      >
    </h2>
    {#if live}<button
        class="quiet reload-schedule"
        disabled={!loaded || loading || dirty}
        title={dirty
          ? 'Discard edits before reloading'
          : 'Reload schedule from server'}
        onclick={loadSchedule}
        ><span aria-hidden="true">↻</span> Reload schedule</button
      >{/if}
  </header>
  <div id="schedule-content">
    {#if !live}<p class="availability">
        Try a new schedule. Nothing here changes your thermostat.
      </p>{/if}
    {#if live && (loading || (!loaded && !loadError))}<p
        class="sr-only"
        role="status"
      >
        Loading schedule…
      </p>{/if}
    {#if live && !loaded}
      <div class="schedule-placeholder">
        <div class="skeleton" class:failed={!!loadError} aria-hidden="true">
          <div class="toolbar">
            <div class="pattern-picker skeleton-shape">
              {#each patternChoices as choice}<div class="pattern-choice">
                  <span class="pattern-option">
                    {choice.label}<small class="mode-marker">● Active</small>
                  </span>
                </div>{/each}
            </div>
          </div>
          <div class="schedule-editor">
            {#each [0, 1] as row}
              <div class="timeline-area">
                <div class="row-heading">
                  <span class="skeleton-shape skeleton-label"></span>
                  <span class="skeleton-shape skeleton-preset"></span>
                </div>
                <div class="timeline skeleton-shape"></div>
              </div>
            {/each}
            <div class="draft-actions">
              <div>
                <button class="quiet skeleton-shape" disabled>↶ Undo</button>
                <button class="quiet skeleton-shape" disabled
                  >Discard changes</button
                >
              </div>
              <div class="save-area">
                <button class="save skeleton-shape" disabled
                  >Save schedule</button
                >
                <small class="skeleton-shape">Saving isn’t available yet</small>
              </div>
            </div>
          </div>
        </div>
        {#if loadError}<div class="load-failure" role="alert">
            <p>{loadError}</p>
            <button class="quiet" onclick={loadSchedule}>Retry load</button>
          </div>{/if}
      </div>
    {/if}
    {#if !live || loaded}
      <div class="toolbar">
        <fieldset class="pattern-picker" disabled={loading}>
          <legend class="sr-only">Schedule pattern</legend>
          <span id="schedule-current-mode" class="sr-only"
            >Current schedule mode last read from the server</span
          >
          {#each patternChoices as choice}
            <label class="pattern-choice"
              ><input
                type="radio"
                name="schedule-pattern"
                value={choice.value}
                aria-label={choice.label}
                aria-describedby={live &&
                loaded &&
                choice.value === loadedPattern
                  ? 'schedule-current-mode'
                  : undefined}
                checked={pattern === choice.value}
                onchange={() => changePattern(choice.value)}
              /><span class="pattern-option"
                >{choice.label}<small
                  class="mode-marker"
                  class:is-current={live &&
                    loaded &&
                    choice.value === loadedPattern}
                  aria-hidden="true">● Active</small
                ></span
              ></label
            >
          {/each}
        </fieldset>
      </div>
      <div class="schedule-editor" inert={loading}>
        {#each drafts[pattern].groups as row (row.group)}
          {@const active = selected === row.group}
          {@const rowBlocks = periods(row.changes)}
          {@const rowDisplay = active ? display : rowBlocks}
          {@const changeovers = boundaryLabels(
            rowDisplay.slice(1).map((period) => period.start),
            timelineWidths[row.group] ?? 600,
          )}
          <section
            class="timeline-area"
            aria-label={`${labels[row.group]} schedule`}
          >
            <div class="row-heading">
              <h3>{labels[row.group]}</h3>
              <div class="preset-picker">
                <button
                  class="preset-toggle"
                  aria-expanded={active && presetsOpen}
                  onclick={(event) => {
                    presetsAbove =
                      event.currentTarget.getBoundingClientRect().top >
                      window.innerHeight / 2;
                    activate(row.group);
                    dismiss();
                    presetsOpen = !presetsOpen;
                    hover = null;
                  }}>Choose a preset <span aria-hidden="true">⌄</span></button
                >
                {#if active && presetsOpen}<div
                    class="preset-menu"
                    class:above={presetsAbove}
                  >
                    <div class="preset-heading">
                      <strong>Suggested schedules</strong><button
                        class="close"
                        aria-label="Close suggested schedules"
                        onclick={() => (presetsOpen = false)}>×</button
                      >
                    </div>
                    <p>
                      Replaces {labels[selected].toLowerCase()} only. You can edit
                      or undo it.
                    </p>
                    <div class="preset-options">
                      {#each schedulePresets as preset}
                        <button
                          class="preset-card"
                          aria-label={`Use ${preset.name} preset`}
                          onclick={() => usePreset(preset)}
                        >
                          <strong>{preset.name}</strong>
                          <span class="preset-strip" aria-hidden="true"
                            >{#each periods(preset.changes) as period}<span
                                style:flex={period.end - period.start}
                                style:background={period.temperature >= 210
                                  ? '#d9a064'
                                  : '#99b9aa'}
                              ></span>{/each}</span
                          >
                          <span class="preset-description"
                            >{preset.description}</span
                          ><small>{preset.detail}</small>
                        </button>
                      {/each}
                    </div>
                    <button class="custom-hours" onclick={range}
                      >Choose custom hours…</button
                    >
                  </div>
                {/if}
              </div>
            </div>
            <!-- Pointer gestures have keyboard equivalents on blocks and slider handles. -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="timeline"
              style:margin-bottom={changeovers.length ? '30px' : '0px'}
              use:registerTimeline={row.group}
              onpointerdown={(event) => {
                activate(row.group);
                begin(event, 'range');
              }}
              onpointermove={(event) => move(event, row.group)}
              onpointerup={finish}
              onpointercancel={cancel}
              onpointerleave={() => (hover = null)}
            >
              {#each rowDisplay as b}<button
                  class="block"
                  class:active={active &&
                    popover &&
                    !selection.range &&
                    selection.start === b.start}
                  class:last-block={b.end === 1440}
                  style:left={`${b.start / 14.4}%`}
                  style:width={`${(b.end - b.start) / 14.4}%`}
                  style:--warm={`${Math.min(75, Math.max(0, (b.temperature - 185) * 2))}%`}
                  title={`${clock(b.start)}–${clock(b.end)} · ${(b.temperature / 10).toFixed(1)}°C`}
                  aria-label={`${clock(b.start)} to ${clock(b.end)}, ${(b.temperature / 10).toFixed(1)} degrees`}
                  aria-pressed={active &&
                    popover &&
                    !selection.range &&
                    selection.start === b.start}
                  onclick={(event) => {
                    activate(row.group);
                    clickBlock(b, event);
                  }}
                  ><span class="block-value"
                    >{(b.temperature / 10).toFixed(1)}<small>°</small></span
                  ><span class="block-dot" aria-hidden="true">·</span></button
                >{/each}
              {#if changeovers.length}<svg
                  class="changeover-guides"
                  aria-hidden="true"
                  width="100%"
                  height="10"
                >
                  {#each changeovers as label}<line
                      x1={`${label.minute / 14.4}%`}
                      y1="0"
                      x2={`${label.position}%`}
                      y2="10"
                    />{/each}
                </svg>{/if}
              {#each changeovers as label}<span
                  class="changeover-time"
                  aria-hidden="true"
                  style:left={`${label.position}%`}>{clock(label.minute)}</span
                >{/each}
              <div class="timeline-scale" aria-hidden="true">
                {#each [6, 12, 18] as hour}<span
                    style:left={`${(hour / 24) * 100}%`}
                    >{hour.toString().padStart(2, '0')}</span
                  >{/each}
              </div>
              {#if active && drawnRange}
                <div
                  class="range-overlay"
                  style:left={`${drawnRange.start / 14.4}%`}
                  style:width={`${(drawnRange.end - drawnRange.start) / 14.4}%`}
                ></div>
              {/if}
              {#if active && drag?.kind === 'range'}
                {@const start = Math.min(
                  drag.anchor,
                  drag.moved ? drag.minute : drag.anchor,
                )}
                {@const end = Math.max(
                  drag.anchor,
                  drag.moved ? drag.minute : drag.anchor,
                )}
                <span
                  class="drag-range-time"
                  role="status"
                  aria-label="Drawing range"
                  style:left={`clamp(64px, ${(start + end) / 2 / 14.4}%, calc(100% - 64px))`}
                  >{clock(start)}–{clock(end)}</span
                >
              {/if}
              {#if active && selection.range && drawnRange && drag?.kind !== 'range'}
                {#each ['start', 'end'] as edge}
                  {@const e = edge as 'start' | 'end'}
                  <button
                    class="boundary selection-handle"
                    role="slider"
                    aria-label={`Selection ${edge}`}
                    aria-valuemin={e === 'start' ? 0 : selection.start + 10}
                    aria-valuemax={e === 'start' ? selection.end - 10 : 1440}
                    aria-valuenow={drawnRange[e]}
                    aria-valuetext={clock(drawnRange[e])}
                    style:left={`${drawnRange[e] / 14.4}%`}
                    onpointerdown={(event) =>
                      begin(
                        event,
                        e === 'start' ? 'selection-start' : 'selection-end',
                      )}
                    onpointermove={(event) => move(event, row.group)}
                    onpointerup={finish}
                    onpointercancel={cancel}
                    onkeydown={(event) => selectionKey(event, e)}
                    onclick={(event) => event.stopPropagation()}
                    ><span aria-hidden="true"></span><strong
                      class="boundary-time">{clock(drawnRange[e])}</strong
                    ></button
                  >
                {/each}
              {:else}
                {#each rowDisplay.slice(1) as b, i}<button
                    class="boundary"
                    role="slider"
                    aria-label={`Boundary ${i + 1}`}
                    aria-valuemin={rowBlocks[i].start + 10}
                    aria-valuemax={rowBlocks[i + 1].end - 10}
                    aria-valuenow={b.start}
                    aria-valuetext={clock(b.start)}
                    style:left={`${b.start / 14.4}%`}
                    onpointerdown={(event) => {
                      activate(row.group);
                      begin(event, 'boundary', i + 1);
                    }}
                    onpointermove={(event) => move(event, row.group)}
                    onpointerup={finish}
                    onpointercancel={cancel}
                    onkeydown={(event) => {
                      activate(row.group);
                      boundaryKey(event, i + 1);
                    }}
                    onclick={(event) => event.stopPropagation()}
                    ><span aria-hidden="true"
                    ></span>{#if active && drag?.kind === 'boundary' && drag.index === i + 1 && drag.moved}<strong
                        class="boundary-time">{clock(b.start)}</strong
                      >{/if}</button
                  >{/each}
              {/if}
              {#if hoverGroup === row.group && hover !== null && !drag}<div
                  class="hover-guide"
                  style:left={`${hover / 14.4}%`}
                  aria-hidden="true"
                ></div>
                <span
                  class="hover-time"
                  style:left={`clamp(24px, ${hover / 14.4}%, calc(100% - 24px))`}
                  aria-hidden="true">{clock(hover)}</span
                >{/if}
              {#if active && popover && !drag?.moved}
                <!-- Contextual controls stay attached to the bar; no permanent form below it. -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="period-popover"
                  role="group"
                  aria-label={selection.range
                    ? 'Edit selected range'
                    : 'Edit selected period'}
                  style:left={`clamp(146px, ${anchor}%, calc(100% - 146px))`}
                  onpointerdown={(event) => event.stopPropagation()}
                  onpointermove={(event) => event.stopPropagation()}
                >
                  <div class="popover-heading">
                    <button
                      class="time-summary"
                      aria-expanded={precise}
                      aria-controls="precise-times"
                      title="Edit exact times"
                      onclick={() => (precise = !precise)}
                      >{clock(selection.start)}–{clock(selection.end)}
                      <span aria-hidden="true">⌄</span></button
                    ><button
                      class="close"
                      aria-label={selection.range
                        ? 'Cancel range'
                        : 'Close period editor'}
                      onclick={dismiss}>×</button
                    >
                  </div>
                  <div class="temperature-controls">
                    <button
                      aria-label="Decrease period temperature"
                      disabled={selection.temperature <= 50}
                      onclick={() => adjustTemperature(-5)}>−</button
                    ><output aria-label="Period temperature"
                      >{(selection.temperature / 10).toFixed(1)}<small>°C</small
                      ></output
                    ><button
                      aria-label="Increase period temperature"
                      disabled={selection.temperature >= 350}
                      onclick={() => adjustTemperature(5)}>+</button
                    >
                  </div>
                  {#if precise}<div class="precise-times" id="precise-times">
                      {#each ['start', 'end'] as edge}{@const e = edge as
                          'start' | 'end'}{@const fixed =
                          !selection.range &&
                          (e === 'start'
                            ? selection.start === 0
                            : selection.end === 1440)}<label
                          >{e === 'start' ? 'From' : 'Until'}<select
                            aria-label={`Period ${edge}`}
                            value={selection[e]}
                            disabled={fixed}
                            onchange={(event) => {
                              adjustTime(e, Number(event.currentTarget.value));
                              precise = true;
                            }}
                            >{#each times as minute}<option
                                value={minute}
                                disabled={e === 'start'
                                  ? minute >= selection.end
                                  : minute <= selection.start}
                                >{clock(minute)}</option
                              >{/each}</select
                          ></label
                        >{/each}
                    </div>{/if}
                  {#if selection.range}<button
                      class="apply-range"
                      aria-label={`Set ${(selection.temperature / 10).toFixed(1)}°C for this range`}
                      onclick={() => apply()}>Apply to range</button
                    >{/if}
                </div>
              {/if}
            </div>
          </section>
        {/each}
        <div class="draft-actions">
          <div>
            <button class="quiet" disabled={!history.length} onclick={undo}
              >↶ Undo</button
            >
            <button class="quiet" disabled={!dirty} onclick={discard}
              >Discard changes</button
            >
          </div>
          <div class="save-area">
            <button class="save" disabled aria-describedby="schedule-save-note"
              >Save schedule</button
            >
            <small id="schedule-save-note">Saving isn’t available yet</small>
          </div>
        </div>
      </div>
      {#if loadError}<p class="error" role="alert">
          {loadError}<button
            class="quiet"
            disabled={loading || dirty}
            onclick={loadSchedule}>Retry load</button
          >
        </p>{/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if validationError}<p class="error" role="alert">
          {validation}<button class="quiet" onclick={checkDraft}
            >Retry check</button
          >
        </p>{/if}
    {/if}
  </div>
</section>

<style>
  .schedule-panel {
    position: relative;
    margin-top: 18px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 20px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 500;
  }
  .schedule-heading {
    padding: 22px 30px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px 16px;
  }
  .schedule-heading h2 {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .reload-schedule {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .changeover-guides {
    position: absolute;
    top: 100%;
    left: 0;
    pointer-events: none;
    stroke: var(--muted);
    stroke-opacity: 0.45;
    stroke-width: 1;
  }
  .changeover-time {
    position: absolute;
    top: calc(100% + 12px);
    transform: translateX(-50%);
    color: var(--muted);
    font-size: 10px;
    line-height: 14px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    pointer-events: none;
  }
  small {
    font-size: 11px;
    color: var(--muted);
    font-weight: 400;
  }
  #schedule-content {
    padding: 0 30px 18px;
  }
  .schedule-placeholder {
    position: relative;
  }
  .skeleton.failed {
    visibility: hidden;
  }
  .skeleton .pattern-option {
    color: transparent;
  }
  .skeleton .row-heading {
    height: 44px;
  }
  .skeleton .timeline {
    margin-bottom: 30px;
  }
  .skeleton-label {
    width: 70px;
    height: 12px;
  }
  .skeleton-preset {
    width: 120px;
    height: 12px;
  }
  .skeleton .quiet,
  .skeleton .save,
  .skeleton .save-area small {
    color: transparent;
  }
  .skeleton-shape {
    position: relative;
    overflow: hidden;
    border-radius: 8px;
    background: var(--soft);
  }
  .skeleton-shape::after {
    content: '';
    position: absolute;
    inset: 0;
    transform: translateX(-100%);
    background: linear-gradient(
      90deg,
      transparent,
      color-mix(in srgb, var(--accent) 12%, transparent),
      transparent
    );
    animation: schedule-shimmer 1.8s ease-in-out infinite;
  }
  .failed .skeleton-shape::after {
    animation: none;
  }
  .load-failure {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    font-size: 12px;
    color: var(--muted);
    text-align: center;
  }
  .load-failure p {
    margin: 0;
  }
  @keyframes schedule-shimmer {
    to {
      transform: translateX(100%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .skeleton-shape::after {
      animation: none;
      display: none;
    }
  }
  .availability {
    font-size: 12px;
    color: var(--muted);
    margin: 0 0 22px;
    line-height: 1.6;
  }
  button,
  select {
    font: inherit;
    color: var(--ink);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 8px;
    min-height: 44px;
    font-size: 12px;
  }
  select {
    padding: 10px;
    max-width: 100%;
    background: var(--surface);
  }
  .sr-only,
  .pattern-choice input {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
  .pattern-picker {
    display: flex;
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: 4px;
    gap: 4px;
    border: 1px solid var(--border);
    background: var(--soft);
    border-radius: 12px;
  }
  .pattern-choice {
    position: relative;
    flex: 1;
    display: flex;
    cursor: pointer;
    min-width: 0;
  }
  .pattern-option {
    flex-direction: column;
    gap: 3px;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 9px 10px;
    border-radius: 8px;
    font-size: 12px;
    line-height: 1.3;
    text-align: center;
    min-height: 44px;
    color: var(--muted);
    border: 1px solid transparent;
  }
  .mode-marker {
    visibility: hidden;
    font-size: 9px;
    line-height: 11px;
    color: var(--accent);
  }
  .mode-marker.is-current {
    visibility: visible;
  }
  .pattern-choice input:checked + span {
    background: var(--surface);
    color: var(--ink);
    border-color: var(--border);
    box-shadow: 0 1px 3px #0001;
  }
  .pattern-choice input:focus-visible + span {
    outline: 3px solid #b98030;
    outline-offset: 2px;
  }
  .preset-picker {
    position: relative;
  }
  .preset-toggle {
    border: 0;
    padding: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent);
    cursor: pointer;
    min-height: 44px;
    font-size: 12px;
    white-space: nowrap;
  }
  .preset-toggle[aria-expanded='true'] > span {
    transform: rotate(180deg);
  }
  .preset-menu {
    position: absolute;
    z-index: 10;
    right: 0;
    top: calc(100% + 4px);
    width: min(620px, calc(100vw - 88px));
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    box-shadow: 0 8px 35px #0003;
    color: var(--ink);
  }
  .preset-menu.above {
    top: auto;
    bottom: calc(100% + 4px);
  }
  .preset-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .preset-heading > strong {
    font-size: 14px;
    font-weight: 500;
  }
  .preset-menu p {
    color: var(--muted);
    font-size: 11px;
    line-height: 1.5;
    margin: 4px 0 14px;
  }
  .preset-options {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }
  .preset-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    text-align: left;
    background: var(--surface);
    min-width: 0;
  }
  .preset-card:hover {
    background: var(--soft);
    border-color: var(--accent);
  }
  .preset-card strong {
    font-size: 12px;
    font-weight: 500;
  }
  .preset-strip {
    display: flex;
    width: 100%;
    height: 7px;
    gap: 1px;
    border-radius: 4px;
    overflow: hidden;
  }
  .preset-description {
    font-size: 11px;
    line-height: 1.5;
  }
  .preset-card small {
    font-size: 10px;
    line-height: 1.5;
  }
  .custom-hours {
    border: 0;
    color: var(--accent);
    padding: 8px 0 0;
    margin-top: 8px;
  }
  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }
  .quiet {
    border: 0;
    color: var(--accent);
    padding: 8px;
    font-size: 12px;
    white-space: nowrap;
  }
  .timeline-area {
    margin: 0 0 12px;
  }
  .row-heading {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }
  .row-heading h3 {
    margin: 0;
    font-size: 12px;
    font-weight: 500;
  }
  .draft-actions {
    position: sticky;
    bottom: 0;
    z-index: 7;
    background: var(--surface);
    box-shadow: 0 -8px 12px -12px #0008;
    display: flex;
    justify-content: space-between;
    align-items: start;
    gap: 12px;
    padding: 12px 0 max(12px, env(safe-area-inset-bottom));
    margin-top: 16px;
    border-top: 1px solid var(--border);
  }
  .draft-actions > div:first-child {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .save-area {
    display: flex;
    flex-direction: column;
    align-items: end;
    gap: 6px;
  }
  .save {
    padding: 8px 16px;
    background: var(--soft);
  }
  .save-area small {
    font-size: 10px;
  }

  .schedule-editor {
    position: relative;
    display: flow-root;
    margin-top: 12px;
  }
  .timeline-scale {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 1;
    color: #203128b8;
    font-size: 9px;
    font-variant-numeric: tabular-nums;
  }
  .timeline-scale span {
    position: absolute;
    bottom: 4px;
    transform: translateX(-50%);
    line-height: 1;
  }
  .timeline-scale span::before {
    content: '';
    position: absolute;
    height: 4px;
    border-left: 1px solid #20312840;
    left: 50%;
    bottom: 12px;
  }
  .timeline {
    position: relative;
    height: 64px;
    touch-action: pan-y;
    user-select: none;
    border-radius: 10px;
    background: var(--soft);
  }
  .block {
    container-type: inline-size;
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 0;
    border: 0;
    border-right: 1px solid var(--surface);
    background: color-mix(
      in srgb,
      #99b9aa calc(100% - var(--warm)),
      #d9a064 var(--warm)
    );
    color: #203128;
    font-size: 20px;
    font-variant-numeric: tabular-nums;
    padding: 0 0 14px;
    overflow: hidden;
  }
  .block:first-child {
    border-radius: 10px 0 0 10px;
  }
  .block.last-block {
    border-top-right-radius: 10px;
    border-bottom-right-radius: 10px;
  }
  .block.active {
    box-shadow: inset 0 0 0 3px var(--accent);
  }
  .block small {
    font-size: 14px;
    color: inherit;
  }
  .block-dot {
    display: none;
  }
  .block-value {
    white-space: nowrap;
  }
  @container (max-width:76px) {
    .block-value,
    .block-value small {
      font-size: 12px;
    }
  }
  @container (max-width:52px) {
    .block-value,
    .block-value small {
      font-size: 10px;
    }
  }
  @container (max-width:40px) {
    .block-value {
      display: none;
    }
    .block-dot {
      display: inline;
    }
  }
  .boundary {
    position: absolute;
    top: 0;
    width: 28px;
    height: 100%;
    transform: translateX(-50%);
    cursor: ew-resize;
    border: 0;
    display: grid;
    place-items: center;
    touch-action: none;
    z-index: 3;
    padding: 0;
  }
  .boundary > span {
    background: var(--surface);
    width: 5px;
    height: 28px;
    border-radius: 4px;
    box-shadow: 0 0 0 1px #26372f44;
  }
  .selection-handle {
    z-index: 6;
  }
  .selection-handle > span {
    background: var(--accent);
    height: 40px;
    width: 7px;
    box-shadow: 0 0 0 2px var(--surface);
  }
  .boundary:focus-visible {
    outline-offset: 0;
  }
  .boundary-time,
  .hover-time,
  .drag-range-time {
    position: absolute;
    top: -27px;
    transform: translateX(-50%);
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    background: var(--ink);
    color: var(--surface);
    padding: 4px 6px;
    border-radius: 5px;
    pointer-events: none;
  }
  .boundary-time {
    left: 50%;
  }
  .range-overlay {
    position: absolute;
    inset-block: 0;
    border: 2px solid var(--accent);
    background: #29634822;
    pointer-events: none;
    z-index: 4;
  }
  .hover-guide {
    position: absolute;
    top: 0;
    height: 100%;
    border-left: 1px dashed #203128;
    pointer-events: none;
    z-index: 5;
  }
  .hover-time,
  .drag-range-time {
    z-index: 7;
  }
  .period-popover {
    position: absolute;
    bottom: calc(100% + 36px);
    transform: translateX(-50%);
    width: 280px;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 8px 35px #0003;
    z-index: 8;
    touch-action: auto;
  }
  .period-popover:after {
    content: '';
    position: absolute;
    bottom: -6px;
    left: calc(50% - 6px);
    width: 10px;
    height: 10px;
    background: var(--surface);
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    transform: rotate(45deg);
  }
  .popover-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .time-summary,
  .close {
    border: 0;
    min-height: 32px;
    padding: 4px 8px;
  }
  .time-summary {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .time-summary span {
    margin-left: 8px;
    color: var(--muted);
  }
  .close {
    font-size: 22px;
    width: 32px;
    color: var(--muted);
  }
  .temperature-controls {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    margin-top: 6px;
  }
  .temperature-controls button {
    width: 44px;
    font-size: 20px;
    background: var(--soft);
    color: var(--accent);
  }
  output {
    font-size: 24px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  output small {
    font-size: 12px;
    margin-left: 3px;
  }
  .apply-range {
    margin-top: 12px;
    width: 100%;
    background: var(--accent);
    color: var(--surface);
    border: 0;
  }
  .precise-times {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    margin-top: 12px;
  }
  .precise-times label {
    font-size: 10px;
    color: var(--muted);
  }
  .precise-times select {
    display: block;
    width: 100%;
    margin-top: 5px;
  }
  .error {
    font-size: 12px;
    line-height: 1.5;
  }
  @media (max-width: 540px) {
    .toolbar {
      flex-wrap: wrap;
      gap: 4px;
    }
    .pattern-picker {
      flex-basis: 100%;
    }
    .pattern-option {
      padding: 8px 5px;
      font-size: 11px;
    }
    .preset-menu {
      width: calc(100vw - 74px);
      padding: 14px;
      max-height: 60vh;
      overflow-y: auto;
    }
    .preset-options {
      grid-template-columns: 1fr;
    }
    .preset-card {
      gap: 6px;
      padding: 12px;
    }
    .preset-strip {
      height: 5px;
    }

    .schedule-heading {
      padding: 20px;
    }
    #schedule-content {
      padding: 0 20px 14px;
    }
    .timeline {
      height: 64px;
    }
    .block {
      font-size: 15px;
    }
    .period-popover {
      width: min(280px, 100%);
      left: 50% !important;
    }
    .boundary-time {
      font-size: 10px;
      padding: 4px;
    }
    .draft-actions {
      flex-wrap: wrap;
    }
    .save-area {
      margin-left: auto;
    }
  }
</style>
