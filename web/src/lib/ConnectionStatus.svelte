<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { Dashboard } from './dashboard.svelte';
  import { connectionStatus } from './connection-status';
  import {
    ICON_TRANSITION_MS,
    StatusIconSequence,
    type StatusIcon,
  } from './status-icon-sequence.svelte';
  let { dashboard }: { dashboard: Dashboard } = $props();
  const id = $props.id();
  const sequence = new StatusIconSequence();
  const layers: StatusIcon[] = [
    'waiting',
    'adjusting',
    'sending',
    'cloud',
    'synced',
    'error',
  ];
  let reducedMotion = $state(false);
  let hovered = $state<number | null>(null);
  let focused = $state<number | null>(null);
  let pinned = $state<number | null>(null);
  let shown = $derived(pinned ?? hovered ?? focused);
  let feedback = $derived(connectionStatus(dashboard));
  let detail = $derived(shown === 0 ? feedback.server : feedback.thermostat);
  let intent = $derived(
    `${dashboard.authGeneration}:${dashboard.state?.session}:${dashboard.adjustmentRevision}:${dashboard.target}`,
  );
  $effect(() => {
    const next = feedback.server.icon;
    const key = intent;
    const reduced = reducedMotion;
    untrack(() => sequence.update(next, key, next === 'synced', reduced));
  });
  const dismiss = () => {
    hovered = null;
    focused = null;
    pinned = null;
  };
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

<div class="connection-status" style:--icon-duration="{ICON_TRANSITION_MS}ms">
  <div
    class="status-pair"
    role="group"
    aria-label="Server and thermostat status"
  >
    {#each ['Server', 'Thermostat'] as label, index}
      <div class="status-item">
        <button
          type="button"
          class="status-icon"
          class:neutral={index === 1 &&
            feedback.thermostat.state === 'unverified'}
          class:issue={index === 0 && feedback.server.icon === 'error'}
          data-state={index === 0
            ? sequence.display
            : feedback.thermostat.state}
          aria-label="{index === 0
            ? feedback.server.title
            : feedback.thermostat.title}. Show details"
          aria-expanded={shown === index}
          aria-describedby={shown === index ? `${id}-detail` : undefined}
          onpointerenter={() => (hovered = index)}
          onpointerleave={() => (hovered = null)}
          onfocus={() => (focused = index)}
          onblur={() => {
            focused = null;
            pinned = null;
          }}
          onclick={() => {
            if (pinned === index) dismiss();
            else pinned = index;
          }}
          onkeydown={(event) => {
            if (event.key === 'Escape') dismiss();
          }}
        >
          {#if index === 0}
            <span
              class="settling-glow"
              class:active={feedback.server.icon === 'adjusting'}
              aria-hidden="true"
            ></span>
            <span
              class="activity"
              class:active={feedback.server.icon === 'sending' &&
                (sequence.display === 'sending' ||
                  sequence.display === 'cloud')}
              aria-hidden="true"
            ></span>
            {#each layers as icon}
              <svg
                class="icon-layer"
                class:visible={sequence.display === icon}
                ontransitionend={(event) => {
                  if (event.propertyName === 'opacity')
                    sequence.finishTransition(icon);
                }}
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.7"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                {#if icon === 'synced'}<path d="m6 12 4 4 8-8" />
                {:else if icon === 'error'}<path
                    d="m12 4 9 16H3L12 4Z M12 9v4m0 3h.01"
                  />
                {:else if icon === 'sending'}<path
                    d="M12 18V5m-5 5 5-5 5 5M5 16v4h14v-4"
                  />
                {:else}<path
                    d="M7 18h11a4 4 0 0 0 .5-8 6.5 6.5 0 0 0-12.3-1.5A4.8 4.8 0 0 0 7 18Z"
                  />{/if}
              </svg>
            {/each}
          {:else}
            <svg
              class="thermostat-glyph"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.7"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path
                d="M10 14.5V5a2 2 0 0 1 4 0v9.5a4 4 0 1 1-4 0Z M12 9v8"
              /><circle cx="12" cy="18" r="1" />
            </svg>
            <span class="confirmation-mark" aria-hidden="true">
              <span class:visible={feedback.thermostat.state === 'unverified'}
                >?</span
              >
              <svg
                class:visible={feedback.thermostat.state === 'verified'}
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"><path d="m6 12 4 4 8-8" /></svg
              >
            </span>
          {/if}
        </button>
        <span class="icon-label">{label}</span>
      </div>
    {/each}
  </div>
  <div class="status-foot">
    <button
      class="retry"
      class:available={feedback.server.retry}
      type="button"
      disabled={!feedback.server.retry ||
        dashboard.submitting ||
        !dashboard.canSetTarget}
      tabindex={feedback.server.retry ? 0 : -1}
      aria-hidden={!feedback.server.retry}
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
    class:shown={shown !== null}
    id="{id}-detail"
    role="tooltip"
    aria-hidden={shown === null}
  >
    <strong>{detail.title}</strong><span>{detail.detail}</span>
  </div>
  <span class="sr-only" role="status" aria-live="polite"
    >{feedback.server.title}. {feedback.thermostat.title}. {feedback.reading}</span
  >
  {#if feedback.server.icon === 'error' || dashboard.readError}
    <span class="sr-only" role="alert"
      >{feedback.server.icon === 'error'
        ? feedback.server.detail
        : dashboard.readError}</span
    >
  {/if}
</div>

<style>
  .connection-status {
    position: relative;
    margin-top: 14px;
  }
  .status-pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 24px;
    max-width: 240px;
    margin: 0 auto;
  }
  .status-item {
    display: flex;
    align-items: center;
    flex-direction: column;
    gap: 9px;
  }
  .icon-label {
    color: var(--muted);
    font-size: 11px;
    line-height: 16px;
  }
  .status-icon {
    position: relative;
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: var(--soft);
    color: var(--accent);
    transition:
      color 150ms ease,
      border-color 150ms ease;
  }
  .status-icon.neutral {
    color: var(--muted);
    background: transparent;
  }
  .status-icon.issue {
    color: var(--warning);
    border-color: var(--warning);
  }
  .icon-layer {
    grid-area: 1/1;
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
  .thermostat-glyph {
    width: 24px;
    height: 24px;
  }
  .confirmation-mark {
    position: absolute;
    right: -2px;
    bottom: -2px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--surface);
    font-size: 11px;
    font-weight: 600;
  }
  .confirmation-mark > * {
    grid-area: 1/1;
    opacity: 0;
    transition: opacity 180ms ease;
  }
  .confirmation-mark > .visible {
    opacity: 1;
  }
  .confirmation-mark svg {
    width: 14px;
    height: 14px;
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
  .status-foot {
    position: absolute;
    right: 0;
    top: 6px;
  }
  @media (max-width: 400px) {
    .connection-status {
      padding-bottom: 44px;
    }
    .status-foot {
      top: auto;
      bottom: 0;
    }
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
    .activity,
    .status-tooltip,
    .confirmation-mark > * {
      transition: none;
    }
    .activity::after {
      animation: none;
    }
  }
</style>
