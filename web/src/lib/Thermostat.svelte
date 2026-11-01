<script lang="ts">
  import type { Dashboard } from './dashboard.svelte';
  import { temperature } from './types';
  import CommandProgress from './CommandProgress.svelte';
  let { dashboard }: { dashboard: Dashboard } = $props();
  let confirmed = $derived(dashboard.state?.confirmed);
</script>

<section class="thermostat" aria-labelledby="room-title">
  <div class="room-heading">
    <div>
      <p class="eyebrow">YOUR HOME</p>
      <h1 id="room-title">A little more comfortable.</h1>
    </div>
    <span class="mode"
      >{confirmed?.mode.kind === 'unknown'
        ? 'Mode unavailable'
        : (confirmed?.mode.kind ?? 'Connecting')}</span
    >
  </div>
  <div class="temperature-layout">
    <div class="ambient">
      <span class="label">Room temperature</span>
      <p>
        <strong>{temperature(confirmed?.room_temperature)}</strong><span
          >°C</span
        >
      </p>
      <span class="muted"
        >{dashboard.state?.connectivity === 'online'
          ? dashboard.state?.provider === 'ocstat'
            ? 'Cloud-reported reading'
            : 'Latest device reading'
          : 'Last known reading'}</span
      >
    </div>
    <div class="setpoint">
      <span class="label"
        >{dashboard.canSetTarget
          ? 'Desired temperature'
          : 'Reported target'}</span
      >
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
            dashboard.target >= 300}>+</button
        >
      </div>
      <p class="muted">
        {#if dashboard.canSetTarget}
          0.5°C per step <span aria-hidden="true">·</span> 5–30°C range
        {:else}Read-only · Temperature changes are disabled{/if}
      </p>
    </div>
  </div>
  {#if dashboard.canSetTarget}
    <CommandProgress {dashboard} />
  {:else}
    <p class="muted">Cloud readings may lag behind the thermostat.</p>
    {#if dashboard.error}<p class="error" role="alert">
        {dashboard.error}
      </p>{/if}
  {/if}
  {#if dashboard.readError}<p class="error" role="status">
      {dashboard.readError}
    </p>{/if}
</section>
