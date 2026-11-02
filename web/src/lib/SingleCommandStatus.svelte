<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { Dashboard } from './dashboard.svelte';
  import { commandProgress } from './progress';
  import { temperature } from './types';
  import {
    ICON_TRANSITION_MS,
    StatusIconSequence,
    type StatusIcon,
  } from './status-icon-sequence.svelte';
  let { dashboard }: { dashboard: Dashboard } = $props();
  const sequence = new StatusIconSequence();
  const icons: StatusIcon[] = [
    'waiting',
    'adjusting',
    'sending',
    'cloud',
    'synced',
    'error',
  ];
  const id = $props.id();
  let reducedMotion = $state(false);
  let hovered = $state(false);
  let focused = $state(false);
  let pinned = $state(false);
  let shown = $derived(hovered || focused || pinned);
  let feedback = $derived.by(() => {
    const progress = commandProgress(dashboard);
    // Connection failures must not wait behind a local debounce or queued write.
    if (dashboard.state?.connectivity === 'offline')
      return {
        ...progress,
        issue: true,
        status: 'Device offline',
        detail:
          'The last device reading is retained. Waiting for the device to reconnect.',
      };
    if (dashboard.error)
      return {
        ...progress,
        issue: true,
        status: 'Update unverified',
        detail: dashboard.error,
      };
    if (dashboard.state?.read_error)
      return {
        ...progress,
        issue: true,
        status: 'Device status unavailable',
        detail: dashboard.readError,
      };
    if (dashboard.state && !dashboard.streamConnected)
      return {
        ...progress,
        issue: true,
        status: 'Live updates disconnected',
        detail:
          'Reconnecting automatically. The latest device state is not yet verified.',
      };
    return progress;
  });
  let liveIcon = $derived<StatusIcon>(
    feedback.issue
      ? 'error'
      : dashboard.debouncing
        ? 'adjusting'
        : feedback.checkpoint === 3
          ? 'synced'
          : feedback.checkpoint === 2
            ? 'cloud'
            : feedback.checkpoint === 1
              ? 'sending'
              : 'waiting',
  );
  let intent = $derived(
    `${dashboard.authGeneration}:${dashboard.state?.session}:${dashboard.adjustmentRevision}:${dashboard.target}`,
  );
  $effect(() => {
    const next = liveIcon;
    const key = intent;
    const acknowledged = dashboard.cloudAcknowledged;
    const reduced = reducedMotion;
    untrack(() => sequence.update(next, key, acknowledged, reduced));
  });
  onMount(() => {
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)');
    reducedMotion = preference.matches;
    const update = () => {
      reducedMotion = preference.matches;
    };
    preference.addEventListener('change', update);
    return () => {
      preference.removeEventListener('change', update);
      sequence.dispose();
    };
  });
</script>

<div class="single-status" style:--icon-duration="{ICON_TRANSITION_MS}ms">
  <span class="device-reading"
    >Device <strong
      >{temperature(dashboard.state?.confirmed?.target_temperature)}°C</strong
    ></span
  >
  <button
    type="button"
    class="status-icon"
    class:issue={feedback.issue}
    data-state={sequence.display}
    aria-label="{feedback.status}. Show details"
    aria-expanded={pinned}
    aria-describedby={shown ? `${id}-detail` : undefined}
    onpointerenter={() => (hovered = true)}
    onpointerleave={() => (hovered = false)}
    onfocus={() => (focused = true)}
    onblur={() => {
      focused = false;
      pinned = false;
    }}
    onclick={() => (pinned = !pinned)}
    onkeydown={(event) => {
      if (event.key === 'Escape') {
        hovered = false;
        focused = false;
        pinned = false;
      }
    }}
  >
    <span
      class="settling-glow"
      class:active={dashboard.debouncing && !feedback.issue}
      aria-hidden="true"
    ></span>
    <span
      class="activity"
      class:active={!dashboard.debouncing &&
        !feedback.issue &&
        (sequence.display === 'sending' || sequence.display === 'cloud')}
      aria-hidden="true"
    ></span>
    {#each icons as icon}
      <svg
        class="icon-layer"
        class:visible={sequence.display === icon}
        ontransitionend={(event) => {
          if (event.propertyName === 'opacity') sequence.finishTransition(icon);
        }}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.7"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        {#if icon === 'adjusting'}
          <path d="M4 7h7m4 0h5M4 17h3m4 0h9" /><circle
            cx="13"
            cy="7"
            r="2"
          /><circle cx="9" cy="17" r="2" />
        {:else if icon === 'sending'}<path
            d="M12 18V5m-5 5 5-5 5 5M5 16v4h14v-4"
          />
        {:else if icon === 'cloud'}<path
            d="M7 18h11a4 4 0 0 0 .5-8 6.5 6.5 0 0 0-12.3-1.5A4.8 4.8 0 0 0 7 18Z"
          />
        {:else if icon === 'synced'}<path d="m6 12 4 4 8-8" />
        {:else if icon === 'error'}<path d="m12 4 9 16H3L12 4Z" /><path
            d="M12 9v4m0 3h.01"
          />
        {:else}<circle cx="12" cy="12" r="8" /><path d="M12 7v5l3 2" />{/if}
      </svg>
    {/each}
  </button>
  <div class="retry-slot">
    <button
      type="button"
      class="retry"
      class:available={feedback.retry}
      disabled={!feedback.retry || dashboard.submitting}
      tabindex={feedback.retry ? 0 : -1}
      aria-hidden={!feedback.retry}
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
  <div
    class="status-tooltip"
    class:shown
    id="{id}-detail"
    role="tooltip"
    aria-hidden={!shown}
  >
    <strong>{feedback.status}</strong><span>{feedback.detail}</span>
  </div>
  {#if feedback.issue}
    <span class="sr-only" role="alert"
      >{feedback.status}. {feedback.detail}</span
    >
  {:else}
    <span class="sr-only" role="status" aria-live="polite"
      >{feedback.status}. {feedback.detail}</span
    >
  {/if}
</div>

<style>
  .single-status {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 56px minmax(0, 1fr);
    align-items: center;
    min-height: 76px;
    gap: 12px;
  }
  .device-reading {
    font-size: 11px;
    line-height: 1.5;
    color: var(--muted);
  }
  .device-reading strong {
    color: var(--ink);
    font-weight: 600;
    margin-left: 5px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .status-icon {
    position: relative;
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: var(--soft);
    color: var(--accent);
    padding: 0;
    transition:
      color 120ms ease,
      border-color 120ms ease;
  }
  .status-icon.issue {
    color: var(--warning);
    border-color: var(--warning);
  }
  .icon-layer {
    grid-area: 1 / 1;
    width: 24px;
    height: 24px;
    opacity: 0;
    transform: scale(0.86);
    transition:
      opacity var(--icon-duration) ease,
      transform var(--icon-duration) ease;
    pointer-events: none;
  }
  .icon-layer.visible {
    opacity: 1;
    transform: scale(1);
  }
  .settling-glow,
  .activity {
    position: absolute;
    inset: -5px;
    border-radius: inherit;
    pointer-events: none;
    opacity: 0;
    transition: opacity 180ms ease;
  }
  .settling-glow {
    box-shadow: 0 0 10px 3px var(--accent);
  }
  .settling-glow.active {
    opacity: 0.35;
  }
  .activity.active {
    opacity: 1;
  }
  .activity::after {
    content: '';
    position: absolute;
    inset: 0;
    border: 1px solid var(--accent);
    border-radius: inherit;
    animation: breathe 1800ms ease-in-out infinite;
    animation-play-state: paused;
  }
  .activity.active::after {
    animation-play-state: running;
  }
  @keyframes breathe {
    0%,
    100% {
      opacity: 0.15;
    }
    50% {
      opacity: 0.65;
    }
  }
  .retry-slot {
    display: flex;
    justify-content: flex-end;
  }
  .retry {
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    padding: 0;
    background: transparent;
    color: var(--warning);
    border: 0;
    border-radius: 50%;
    visibility: hidden;
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .retry.available {
    visibility: visible;
    opacity: 1;
  }
  .retry svg {
    width: 19px;
    height: 19px;
  }
  .status-tooltip {
    position: absolute;
    z-index: 2;
    bottom: calc(100% + 10px);
    left: 50%;
    width: min(320px, calc(100vw - 100px));
    transform: translateX(-50%);
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    color: var(--ink);
    visibility: hidden;
    opacity: 0;
    transition:
      opacity 150ms ease,
      visibility 150ms;
    pointer-events: none;
  }
  .status-tooltip.shown {
    visibility: visible;
    opacity: 1;
  }
  .status-tooltip strong,
  .status-tooltip span {
    display: block;
    font-size: 12px;
    line-height: 1.5;
  }
  .status-tooltip span {
    color: var(--muted);
    margin-top: 4px;
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
  @media (prefers-reduced-motion: reduce) {
    .icon-layer,
    .settling-glow,
    .activity {
      transition: none;
    }
    .activity::after {
      animation: none;
    }
  }
</style>
