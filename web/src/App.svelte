<script lang="ts">
  import { onMount } from 'svelte';
  import { Dashboard } from './lib/dashboard.svelte';
  import Thermostat from './lib/Thermostat.svelte';
  import Schedule from './lib/Schedule.svelte';
  import Login from './lib/Login.svelte';
  import AccountActions from './lib/AccountActions.svelte';
  import AccountHeader from './lib/AccountHeader.svelte';
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
    dashboard.streamReconnecting = false;
    const source = new EventSource('/api/events');
    source.onopen = () => {
      if (generation === dashboard.authGeneration) {
        dashboard.streamConnected = true;
        dashboard.streamReconnecting = false;
      }
    };
    source.onerror = () => {
      if (generation !== dashboard.authGeneration) return;
      dashboard.streamConnected = false;
      dashboard.streamReconnecting = true;
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
      dashboard.streamReconnecting = false;
    };
  });
</script>

<svelte:head
  ><meta name="description" content="Home heating controls." /></svelte:head
>
<main>
  <header>
    <div class="header-actions">
      {#if dashboard.state?.provider === 'simulator'}<span class="demo-badge"
          >Simulator</span
        >{/if}
      <AccountHeader {dashboard} />
      <AccountActions {dashboard} />
    </div>
  </header>
  {#if !dashboard.state}
    <section
      class="page-status"
      aria-label="Session status"
      aria-busy={!dashboard.error}
    >
      {#if dashboard.error}
        <p class="error" role="alert">{dashboard.error}</p>
        <button
          class="text-button"
          onclick={() => {
            dashboard.error = '';
            void dashboard.refresh();
          }}>Try again</button
        >
      {:else}
        <p class="muted loading-message" role="status">Loading…</p>
      {/if}
    </section>
  {:else}
    {#if dashboard.logoutError}<p class="error" role="alert">
        {dashboard.logoutError}
      </p>{/if}
    {#if dashboard.state?.login === 'signing_out'}
      <section class="thermostat"><p role="status">Logging out…</p></section>
    {:else if dashboard.state?.login && dashboard.state.login !== 'ready'}
      <Login {dashboard} />
    {:else}
      <Thermostat {dashboard} />
      {#key dashboard.state.session}<Schedule
          live={dashboard.state.provider === 'ocstat'}
          onExpired={() => dashboard.sessionExpired()}
        />{/key}
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
  {/if}
</main>

<style>
  .header-actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 44px;
    min-width: 0;
  }
  .page-status {
    position: absolute;
    inset: 160px 24px auto;
    text-align: center;
    font-size: 13px;
  }
  .loading-message {
    animation: reveal-loading 150ms ease-out 300ms both;
  }
  @keyframes reveal-loading {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
