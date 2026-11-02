<script lang="ts">
  import type { Dashboard } from './dashboard.svelte';
  import { temperature } from './types';
  let { dashboard }: { dashboard: Dashboard } = $props();
  const id = $props.id();
  let open = $state(false);
  let container: HTMLDivElement;
  let snapshot = $derived(dashboard.state);
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === 'Escape') open = false;
  }}
  onclick={(event) => {
    if (event.target instanceof Node && !container?.contains(event.target))
      open = false;
  }}
/>
<div class="reading-details" bind:this={container}>
  <button
    class="info-button"
    type="button"
    aria-label="Reading details"
    aria-expanded={open}
    aria-controls="{id}-readings"
    onclick={() => (open = !open)}
    onblur={(event) => {
      if (!container.contains(event.relatedTarget as Node | null)) open = false;
    }}
  >
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.6"
      stroke-linecap="round"
      aria-hidden="true"
      ><circle cx="12" cy="12" r="9" /><path d="M12 11v6m0-10h.01" /></svg
    >
  </button>
  <div
    class="details-panel"
    id="{id}-readings"
    hidden={!open}
    role="region"
    aria-label="Reading details"
  >
    <dl>
      <div>
        <dt>Last reading</dt>
        <dd>
          {snapshot?.last_sync
            ? new Date(snapshot.last_sync * 1000).toLocaleString()
            : 'No reading yet'}
        </dd>
      </div>
      <div>
        <dt>Source</dt>
        <dd>
          {snapshot?.provider === 'ocstat' ? 'OCSTAT server' : 'Simulator'}
        </dd>
      </div>
      <div>
        <dt>Server target</dt>
        <dd>{temperature(snapshot?.confirmed?.target_temperature)}°C</dd>
      </div>
      <div>
        <dt>Device connection</dt>
        <dd>
          {snapshot?.connectivity === 'online'
            ? 'Reported online'
            : snapshot?.connectivity === 'offline'
              ? 'Reported offline'
              : 'Unavailable'}
        </dd>
      </div>
      <div>
        <dt>Live updates</dt>
        <dd>
          {dashboard.streamConnected
            ? 'Connected'
            : dashboard.streamReconnecting
              ? 'Reconnecting'
              : 'Connecting'}
        </dd>
      </div>
    </dl>
    <p>
      Refresh reloads the latest saved readings. New readings arrive
      automatically.
    </p>
    {#if snapshot?.provider === 'ocstat'}<p>
        Server acceptance does not confirm that the thermostat has applied the
        target.
      </p>{/if}
    <p>Range: 5–35°C in 0.5°C steps, matching the Android app.</p>
  </div>
</div>

<style>
  .reading-details {
    position: relative;
  }
  .info-button {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border: 0;
    border-radius: 50%;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }
  .info-button:hover,
  .info-button[aria-expanded='true'] {
    background: var(--soft);
    color: var(--ink);
  }
  svg {
    width: 18px;
    height: 18px;
  }
  .details-panel {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 3;
    width: min(320px, calc(100vw - 80px));
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    color: var(--ink);
    font-size: 12px;
    line-height: 1.5;
    box-shadow: 0 8px 24px #0002;
  }
  dl {
    margin: 0;
  }
  dl > div {
    display: grid;
    grid-template-columns: 1fr 1.2fr;
    gap: 12px;
    margin-bottom: 8px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  p {
    margin: 12px 0 0;
    color: var(--muted);
  }
</style>
