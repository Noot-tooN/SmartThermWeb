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
    <span class="mode">{confirmed?.mode.kind ?? 'Connecting'}</span>
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
          ? 'Latest device reading'
          : 'Last known reading'}</span
      >
    </div>
    <div class="setpoint">
      <span class="label">Desired temperature</span>
      <div class="controls">
        <button
          class="step"
          aria-label="Decrease target temperature by 0.5 degrees"
          onclick={() => dashboard.change(-5)}
          disabled={dashboard.target === null || dashboard.target <= 50}
          >−</button
        >
        <output aria-label="Desired target temperature" aria-live="polite"
          ><strong>{temperature(dashboard.target)}</strong><span>°C</span
          ></output
        >
        <button
          class="step"
          aria-label="Increase target temperature by 0.5 degrees"
          onclick={() => dashboard.change(5)}
          disabled={dashboard.target === null || dashboard.target >= 300}
          >+</button
        >
      </div>
      <p class="muted">
        0.5°C per step <span aria-hidden="true">·</span> 5–30°C range
      </p>
    </div>
  </div>
  <CommandProgress {dashboard} />
</section>
