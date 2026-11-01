<script lang="ts">
  import { onMount } from 'svelte';
  import { Dashboard } from './lib/dashboard.svelte';
  import Thermostat from './lib/Thermostat.svelte';
  import type { Scenario, SimConfig } from './lib/types';
  const dashboard = new Dashboard();
  let scenario = $state<Scenario>('normal');
  let ackMs = $state(900);
  let applyMs = $state(2400);
  let simError = $state('');
  let simMessage = $state('');
  let simSaving = $state(false);
  let simLoaded = $state(false);
  let simLoadStarted = false;
  $effect(() => {
    if (dashboard.state?.provider === 'simulator' && !simLoadStarted) {
      simLoadStarted = true;
      void loadConfig();
    }
  });
  async function loadConfig() {
    try {
      const response = await fetch('/api/simulator', {
        signal: AbortSignal.timeout(8000),
      });
      if (!response.ok) throw new Error();
      const config: SimConfig = await response.json();
      scenario = config.scenario;
      ackMs = config.ack_ms;
      applyMs = config.apply_ms;
      simLoaded = true;
    } catch {
      simError =
        'Simulator settings could not be loaded. Refresh the page to try again.';
    }
  }
  async function configure(event: SubmitEvent) {
    event.preventDefault();
    simSaving = true;
    simError = '';
    simMessage = '';
    try {
      const response = await fetch('/api/simulator', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ scenario, ack_ms: ackMs, apply_ms: applyMs }),
        signal: AbortSignal.timeout(8000),
      });
      if (!response.ok) throw new Error();
      simMessage =
        'Applied to future simulator requests. An active command keeps its original behavior.';
    } catch {
      simError =
        'Could not update the simulator. Check the connection and try again.';
    } finally {
      simSaving = false;
    }
  }
  onMount(() => {
    void dashboard.refresh();
    const source = new EventSource('/api/events');
    source.onopen = () => {
      dashboard.streamConnected = true;
    };
    source.onerror = () => {
      dashboard.streamConnected = false;
    };
    source.addEventListener('state', (event: MessageEvent) => {
      try {
        dashboard.accept(JSON.parse(event.data), 'stream');
      } catch {
        dashboard.error =
          'An update could not be read. Refresh to synchronize.';
      }
    });
    return () => {
      source.close();
      dashboard.dispose();
    };
  });
</script>

<svelte:head
  ><meta
    name="description"
    content="A responsive, self-hosted thermostat dashboard with truthful command feedback."
  /></svelte:head
>
<main>
  <header>
    <a class="brand" href="/" aria-label="SmartThermWeb home"
      ><span class="brand-mark" aria-hidden="true">⌂</span>SmartTherm<span
        >Web</span
      ></a
    ><span class="demo-badge"
      >{dashboard.state?.provider === 'ocstat'
        ? 'READ ONLY'
        : dashboard.state?.provider === 'simulator'
          ? 'SIMULATOR'
          : 'CONNECTING'}</span
    >
  </header>
  <div class="connection-row">
    <span
      class:online={dashboard.state?.connectivity === 'online'}
      class="connection"
      ><i></i>{dashboard.state?.connectivity === 'online'
        ? dashboard.state?.provider === 'ocstat'
          ? 'Cloud reports device online'
          : 'Device online'
        : dashboard.state?.connectivity === 'offline'
          ? 'Device offline'
          : dashboard.state?.connectivity === 'unavailable'
            ? 'Device status unavailable'
            : 'Connecting to device'}</span
    ><span class="muted"
      >{dashboard.streamConnected
        ? 'Live updates connected'
        : 'Live updates reconnecting…'}</span
    >
  </div>
  <Thermostat {dashboard} />
  <div class="sync-row">
    <span
      >{dashboard.state?.provider === 'ocstat'
        ? 'Last cloud read'
        : 'Last synchronized'}: {dashboard.state?.last_sync
        ? new Date(dashboard.state.last_sync * 1000).toLocaleTimeString()
        : 'Waiting for a reading'}</span
    ><button class="text-button" onclick={() => dashboard.refresh()}
      >Refresh</button
    >
  </div>
  {#if dashboard.state?.provider === 'simulator'}
    <details class="simulator-panel">
      <summary
        >Try a slow connection or a failure<span aria-hidden="true">+</span
        ></summary
      >
      <p class="muted">
        This is a simulated thermostat. No commands are sent to your heating
        system.
      </p>
      <form onsubmit={configure}>
        <label
          >Behavior<select bind:value={scenario}
            ><option value="normal">Normal operation</option><option
              value="rejected">Server rejects the command</option
            ><option value="transport_failure"
              >Connection fails before application</option
            ><option value="reply_lost">Reply lost, device still applies</option
            ><option value="ack_timeout"
              >Reply times out, device still applies</option
            ><option value="device_timeout"
              >Server accepts, device never applies</option
            ><option value="offline">Device offline</option></select
          ></label
        >
        <div class="timing-fields">
          <label
            >Server reply delay (ms)<input
              type="number"
              bind:value={ackMs}
              min="0"
              max="30000"
              step="100"
              required
            /></label
          ><label
            >Device applies after (ms)<input
              type="number"
              bind:value={applyMs}
              min="0"
              max="60000"
              step="100"
              required
            /></label
          >
        </div>
        <button
          class="apply-button"
          type="submit"
          disabled={simSaving || !simLoaded}
          >{simSaving ? 'Applying…' : 'Apply simulator settings'}</button
        >
        {#if simError}<p class="error" role="alert">
            {simError}
          </p>{/if}{#if simMessage}<p class="muted" role="status">
            {simMessage}
          </p>{/if}
      </form>
    </details>
  {/if}
  <footer>
    SmartThermWeb <span aria-hidden="true">·</span> Local control. Clear feedback.
  </footer>
</main>
