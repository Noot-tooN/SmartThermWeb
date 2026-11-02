<script lang="ts">
  import type { Dashboard } from './dashboard.svelte';
  import { temperature } from './types';
  import CommandProgress from './CommandProgress.svelte';
  import SingleCommandStatus from './SingleCommandStatus.svelte';
  import ConnectionStatus from './ConnectionStatus.svelte';
  import ReadingDetails from './ReadingDetails.svelte';
  // Legacy presentations remain selectable for comparison.
  let {
    dashboard,
    progressStyle = 'dual',
  }: { dashboard: Dashboard; progressStyle?: 'dual' | 'icon' | 'checkpoints' } =
    $props();
  let confirmed = $derived(dashboard.state?.confirmed);
  let refreshing = $state(false);
  async function refreshReadings() {
    if (refreshing) return;
    refreshing = true;
    try {
      await dashboard.refresh();
    } finally {
      refreshing = false;
    }
  }
  let notice = $derived(
    dashboard.readError ||
      dashboard.error ||
      (dashboard.state?.connectivity === 'offline'
        ? 'Thermostat offline. Showing the last reading.'
        : dashboard.streamReconnecting
          ? 'Connection interrupted. Reconnecting…'
          : !dashboard.canSetTarget
            ? 'Controls unavailable in this mode. Check the mode in the Android app.'
            : ''),
  );
  let modeLabel = $derived(
    confirmed?.mode.kind === 'schedule'
      ? 'Auto'
      : confirmed?.mode.kind === 'temporary_override'
        ? 'Auto · Override'
        : confirmed?.mode.kind === 'unknown'
          ? 'Mode unavailable'
          : (confirmed?.mode.kind ?? 'Connecting'),
  );
</script>

<section class="thermostat" aria-labelledby="room-title">
  <div class="room-heading">
    <h1 id="room-title" title={confirmed?.device_name ?? 'Thermostat'}>
      {confirmed?.device_name ?? 'Thermostat'}
    </h1>
    <div class="device-tools">
      <span class="mode">{modeLabel}</span><ReadingDetails {dashboard} />
    </div>
  </div>
  <div class="temperature-layout">
    <div class="ambient">
      <div class="reading-label">
        <span class="label">Room temperature</span>
        <button
          class="reading-refresh"
          class:refreshing
          type="button"
          aria-label="Refresh readings"
          title="Reload the latest saved readings"
          aria-busy={refreshing}
          disabled={refreshing}
          onclick={refreshReadings}
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"><path d="M4 10a8 8 0 1 1 1 7M4 4v6h6" /></svg
          >
        </button>
      </div>
      <p>
        <strong>{temperature(confirmed?.room_temperature)}</strong><span
          >°C</span
        >
      </p>
    </div>
    <div class="setpoint">
      <span class="label">Target temperature</span>
      <div class="controls">
        <button
          class="step"
          aria-label="Decrease target temperature by 0.5 degrees"
          onclick={() => dashboard.change(-5)}
          disabled={!dashboard.canSetTarget ||
            dashboard.target === null ||
            dashboard.target <= 50}>−</button
        >
        <output aria-label="Desired target temperature" aria-live="polite"
          ><strong>{temperature(dashboard.target)}</strong><span>°C</span
          ></output
        >
        <button
          class="step"
          aria-label="Increase target temperature by 0.5 degrees"
          onclick={() => dashboard.change(5)}
          disabled={!dashboard.canSetTarget ||
            dashboard.target === null ||
            dashboard.target >= 350}>+</button
        >
      </div>
    </div>
  </div>
  <div class="control-notice" aria-live="polite">
    {#if notice}<p class="error" role="status">{notice}</p>
    {:else if confirmed?.mode.kind === 'schedule'}<p class="muted">
        Adjustments temporarily override your schedule.
      </p>
    {:else if confirmed?.mode.kind === 'temporary_override'}<p class="muted">
        Temporary override active.
      </p>{/if}
  </div>
  {#if progressStyle === 'dual'}
    <ConnectionStatus {dashboard} />
  {:else if dashboard.canSetTarget}
    {#if progressStyle === 'icon'}
      <SingleCommandStatus {dashboard} />
    {:else}
      <CommandProgress {dashboard} />
    {/if}
  {:else}
    <p class="muted">
      {#if confirmed?.requested_temperature != null}
        Cloud requested {temperature(confirmed.requested_temperature)}°C · Cloud
        status {temperature(confirmed.target_temperature)}°C.
      {/if}
      Cloud readings may lag behind the thermostat.
    </p>
    {#if dashboard.error}<p class="error" role="alert">
        {dashboard.error}
      </p>{/if}
  {/if}
</section>

<style>
  .room-heading h1 {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .device-tools {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-shrink: 0;
  }
  .reading-label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .reading-refresh {
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
  .reading-refresh:hover:enabled {
    background: var(--soft);
    color: var(--ink);
  }
  .reading-refresh svg {
    width: 17px;
    height: 17px;
  }
  .refreshing svg {
    animation: rotate 1s linear infinite;
  }
  @keyframes rotate {
    to {
      transform: rotate(360deg);
    }
  }
  .control-notice {
    min-height: 3em;
    font-size: 12px;
    line-height: 1.5;
    text-align: center;
  }
  .control-notice p {
    margin: 0;
  }
  @media (max-width: 640px) {
    .reading-label {
      justify-content: center;
    }
  }
</style>
