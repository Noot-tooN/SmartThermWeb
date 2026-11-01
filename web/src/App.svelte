<script lang="ts">
  import { onMount } from 'svelte';
  import { Dashboard } from './lib/dashboard.svelte';
  import Thermostat from './lib/Thermostat.svelte';
  import Login from './lib/Login.svelte';
  import AccountActions from './lib/AccountActions.svelte';
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
  let mounted = $state(false);
  let streamWanted = $derived(
    dashboard.state !== null &&
      (dashboard.state.provider === 'simulator' ||
        dashboard.state.login === 'ready' ||
        dashboard.state.login === 'failed'),
  );
  onMount(() => {
    mounted = true;
    void dashboard.refresh();
    return () => {
      mounted = false;
      dashboard.dispose();
    };
  });
  $effect(() => {
    if (!mounted || !streamWanted) return;
    const generation = dashboard.authGeneration;
    dashboard.streamConnected = false;
    const source = new EventSource('/api/events');
    source.onopen = () => {
      if (generation === dashboard.authGeneration)
        dashboard.streamConnected = true;
    };
    source.onerror = () => {
      if (generation !== dashboard.authGeneration) return;
      dashboard.streamConnected = false;
      void dashboard.refresh();
    };
    source.addEventListener('signed_out', () => {
      if (generation === dashboard.authGeneration) dashboard.sessionExpired();
    });
    source.addEventListener('state', (event: MessageEvent) => {
      if (generation !== dashboard.authGeneration) return;
      try {
        dashboard.accept(JSON.parse(event.data), 'stream');
      } catch {
        dashboard.error =
          'An update could not be read. Refresh to synchronize.';
      }
    });
    return () => {
      source.close();
      dashboard.streamConnected = false;
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
    >
    <div class="header-actions">
      <span class="demo-badge"
        >{dashboard.state?.provider === 'ocstat'
          ? 'READ ONLY'
          : dashboard.state?.provider === 'simulator'
            ? 'SIMULATOR'
            : 'CONNECTING'}</span
      >
      <AccountActions {dashboard} />
    </div>
  </header>
  {#if dashboard.logoutError}<p class="error" role="alert">
      {dashboard.logoutError}
    </p>{/if}
  {#if dashboard.state?.login === 'signing_out'}
    <section class="thermostat"><p role="status">Logging out…</p></section>
  {:else if dashboard.state?.login && dashboard.state.login !== 'ready'}
    <Login {dashboard} />
    <div class="sync-row">
      <button class="text-button" onclick={() => dashboard.refresh()}
        >Refresh</button
      >
    </div>
  {:else}
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
              ><option value="reply_lost"
                >Reply lost, device still applies</option
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
  {/if}
  <footer>
    SmartThermWeb <span aria-hidden="true">·</span> Local control. Clear feedback.
  </footer>
</main>

<style>
  .header-actions {
    display: flex;
    align-items: center;
    gap: 16px;
  }
</style>
