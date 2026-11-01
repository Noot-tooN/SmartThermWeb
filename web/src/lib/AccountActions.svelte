<script lang="ts">
  import type { Dashboard } from './dashboard.svelte';
  let { dashboard }: { dashboard: Dashboard } = $props();
</script>

{#if dashboard.state?.provider === 'ocstat' && dashboard.state.login !== 'required'}
  <button
    class="text-button"
    onclick={() => dashboard.logout()}
    disabled={dashboard.loggingOut || dashboard.state.login === 'signing_out'}
  >
    {dashboard.loggingOut || dashboard.state.login === 'signing_out'
      ? 'Logging out…'
      : 'Log out'}
  </button>
{/if}
