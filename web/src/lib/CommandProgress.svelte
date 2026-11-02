<script lang="ts">
  import type { Dashboard } from './dashboard.svelte';
  import { commandProgress } from './progress';
  import { temperature } from './types';
  let { dashboard }: { dashboard: Dashboard } = $props();
  const id = $props.id();
  let checkpoints = $derived([
    {
      name: 'Target selected',
      detail:
        'Your desired temperature is saved locally and queued for sending.',
    },
    {
      name: 'Updating',
      detail:
        'Preparing and sending your latest target. The server has not acknowledged it yet.',
    },
    {
      name: 'Server acknowledged',
      detail:
        'The server accepted the target. Device application still needs confirmation.',
    },
    {
      name:
        dashboard.state?.provider === 'ocstat'
          ? 'Target matches device'
          : 'Synced with device',
      detail:
        dashboard.state?.provider === 'ocstat'
          ? 'The requested and device-reported targets match. Cloud readings may lag behind the thermostat.'
          : 'The device has confirmed your target temperature.',
    },
  ]);
  let progress = $derived(commandProgress(dashboard));
  let hovered = $state<number | null>(null);
  let focused = $state<number | null>(null);
  let pinned = $state<number | null>(null);
  let shown = $derived(pinned ?? focused ?? hovered);
  let tooltip = $derived(
    shown === null
      ? null
      : shown === progress.checkpoint
        ? { name: progress.status, detail: progress.detail }
        : checkpoints[shown],
  );
  function dismiss(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      pinned = null;
      focused = null;
      hovered = null;
    }
  }
</script>

<div
  class="command-progress"
  class:has-issue={progress.issue}
  class:synced={progress.checkpoint === 3}
  class:working={!dashboard.debouncing &&
    !progress.issue &&
    (progress.checkpoint === 1 || progress.checkpoint === 2)}
>
  <div
    class="checkpoint-strip"
    role="group"
    aria-label="Temperature synchronization"
  >
    <div
      class="checkpoint-track"
      role="progressbar"
      aria-label="Target synchronization progress"
      aria-valuemin="0"
      aria-valuemax="3"
      aria-valuenow={progress.checkpoint}
      aria-valuetext={progress.status}
    >
      <div
        class="checkpoint-fill"
        style:transform="scaleX({progress.checkpoint / 3})"
      ></div>
    </div>
    {#each checkpoints as checkpoint, index}
      <button
        type="button"
        class="checkpoint"
        class:reached={index <= progress.checkpoint}
        class:current={index === progress.checkpoint}
        class:problem={index === progress.checkpoint && progress.issue}
        aria-label="{checkpoint.name} details"
        aria-describedby={shown === index ? `${id}-detail` : undefined}
        aria-expanded={pinned === index}
        onpointerenter={() => (hovered = index)}
        onpointerleave={() => (hovered = null)}
        onfocus={() => (focused = index)}
        onblur={() => {
          focused = null;
          pinned = null;
        }}
        onclick={() => (pinned = pinned === index ? null : index)}
        onkeydown={dismiss}
      >
        <span class="checkpoint-disc">
          {#if index === 0}
            <span
              class="debounce-glow"
              class:active={dashboard.debouncing}
              aria-hidden="true"
            ></span>
          {:else}
            <span class="checkpoint-activity" aria-hidden="true"></span>
          {/if}
          <svg
            class="milestone-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            {#if index === 0}<path d="M4 7h7m4 0h5M4 17h3m4 0h9" /><circle
                cx="13"
                cy="7"
                r="2"
              /><circle cx="9" cy="17" r="2" />
            {:else if index === 1}<path
                d="M12 18V5m-5 5 5-5 5 5M5 16v4h14v-4"
              />
            {:else if index === 2}<path
                d="M7 18h11a4 4 0 0 0 .5-8 6.5 6.5 0 0 0-12.3-1.5A4.8 4.8 0 0 0 7 18Z"
              />
            {:else}<path d="m6 12 4 4 8-8" />{/if}
          </svg>
          <svg
            class="issue-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            ><path d="m12 4 9 16H3L12 4Z" /><path d="M12 9v4m0 3h.01" /></svg
          >
        </span>
      </button>
    {/each}
    <div
      class="checkpoint-tooltip"
      class:shown={shown !== null}
      id="{id}-detail"
      role="tooltip"
      aria-hidden={shown === null}
    >
      <strong>{tooltip?.name ?? progress.status}</strong><span
        >{tooltip?.detail ?? progress.detail}</span
      >
    </div>
  </div>
  <div class="progress-footer">
    <span
      >Device <strong
        >{temperature(dashboard.state?.confirmed?.target_temperature)}°C</strong
      ></span
    >
    <button
      type="button"
      class="retry-command"
      class:available={progress.retry}
      disabled={!progress.retry || dashboard.submitting}
      tabindex={progress.retry ? 0 : -1}
      aria-hidden={!progress.retry}
      aria-label="Retry desired target"
      title="Retry desired target"
      onclick={() => dashboard.retry()}
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.7"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"><path d="M4 10a8 8 0 1 1 1 7M4 4v6h6" /></svg
      >
    </button>
  </div>
  <span class="sr-only" role="status" aria-live="polite"
    >{progress.status}. {progress.detail}</span
  >
</div>

<style>
  .command-progress {
    padding-top: 4px;
  }
  .checkpoint-strip {
    position: relative;
    display: flex;
    justify-content: space-between;
    align-items: center;
    height: 56px;
    isolation: isolate;
  }
  .checkpoint-track {
    position: absolute;
    inset: 25px 22px auto;
    height: 4px;
    background: var(--border);
    border-radius: 4px;
    z-index: -1;
    overflow: hidden;
  }
  .checkpoint-fill {
    height: 100%;
    width: 100%;
    background: var(--accent);
    transform-origin: left;
    border-radius: inherit;
    transition:
      transform 650ms cubic-bezier(0.22, 0.68, 0.25, 1),
      background-color 350ms ease;
  }
  .checkpoint {
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    padding: 0;
    background: var(--surface);
    color: var(--muted);
    touch-action: manipulation;
  }
  .checkpoint-disc {
    position: relative;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: var(--surface);
    transition:
      background-color 350ms ease,
      border-color 350ms ease,
      color 350ms ease,
      box-shadow 350ms ease;
  }
  svg {
    width: 19px;
    height: 19px;
  }
  .checkpoint svg {
    grid-area: 1 / 1;
    transition: opacity 250ms ease;
  }
  /* Persistent layers cross-fade; repeated edits never restart an animation. */
  .debounce-glow,
  .checkpoint-activity {
    position: absolute;
    inset: -5px;
    border-radius: inherit;
    pointer-events: none;
    opacity: 0;
    transition: opacity 180ms ease;
  }
  .debounce-glow {
    box-shadow: 0 0 10px 3px var(--accent);
  }
  .debounce-glow.active {
    opacity: 0.35;
  }
  .working .current .checkpoint-activity {
    opacity: 1;
  }
  .reached {
    color: var(--accent);
  }
  .reached .checkpoint-disc {
    background: var(--soft);
    border-color: var(--accent);
  }
  .current .checkpoint-disc {
    box-shadow: 0 0 0 4px var(--soft);
  }
  /* Pause the hidden pulse in place so a new edit can fade it out smoothly. */
  .checkpoint-activity::after {
    content: '';
    position: absolute;
    inset: 0;
    border: 1px solid var(--accent);
    border-radius: inherit;
    animation: checkpoint-breathe 1800ms ease-in-out infinite;
    animation-play-state: paused;
  }
  .working .current .checkpoint-activity::after {
    animation-play-state: running;
  }
  @keyframes checkpoint-breathe {
    0%,
    100% {
      opacity: 0.15;
    }
    50% {
      opacity: 0.65;
    }
  }
  .synced .checkpoint-disc {
    box-shadow: none;
  }
  .issue-icon {
    opacity: 0;
  }
  .problem {
    color: var(--warning);
  }
  .problem .checkpoint-disc {
    background: var(--warning-bg);
    border-color: var(--warning);
    box-shadow: 0 0 0 4px var(--warning-bg);
  }
  .problem .milestone-icon {
    opacity: 0;
  }
  .problem .issue-icon {
    opacity: 1;
  }
  .has-issue .checkpoint-fill {
    background: var(--warning);
  }
  .checkpoint-tooltip {
    position: absolute;
    z-index: 2;
    left: 0;
    right: 0;
    bottom: calc(100% + 8px);
    padding: 12px 14px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--ink);
    opacity: 0;
    visibility: hidden;
    pointer-events: none;
    transform: translateY(4px);
    transition:
      opacity 150ms ease,
      transform 150ms ease,
      visibility 150ms;
  }
  .checkpoint-tooltip.shown {
    opacity: 1;
    visibility: visible;
    transform: translateY(0);
  }
  .checkpoint-tooltip strong {
    display: block;
    font-size: 12px;
    font-weight: 600;
  }
  .checkpoint-tooltip span {
    display: block;
    font-size: 12px;
    line-height: 1.5;
    color: var(--muted);
    margin-top: 4px;
  }
  .progress-footer {
    height: 44px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: var(--muted);
    font-size: 11px;
  }
  .progress-footer strong {
    font-weight: 600;
    color: var(--ink);
    margin-left: 5px;
    font-variant-numeric: tabular-nums;
  }
  .retry-command {
    width: 44px;
    height: 44px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--warning);
    display: grid;
    place-items: center;
    opacity: 0;
    visibility: hidden;
    transition:
      opacity 250ms ease,
      visibility 250ms;
  }
  .retry-command.available {
    opacity: 1;
    visibility: visible;
  }
  .retry-command.available:disabled {
    opacity: 0.45;
  }
  .retry-command:hover:enabled {
    background: var(--warning-bg);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }
</style>
