<script lang="ts">
  import type { Dashboard } from './dashboard.svelte';
  let { dashboard }: { dashboard: Dashboard } = $props();
</script>

{#if dashboard.state?.login === 'ready'}
  <span
    class="account"
    aria-label="Signed-in account"
    title={dashboard.state.account_email ?? 'Signed in'}
  >
    {dashboard.state.account_email ?? 'Signed in'}
  </span>
{/if}

<style>
  .account {
    display: block;
    min-width: 0;
    max-width: 32ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 12px;
  }
  @media (max-width: 480px) {
    .account {
      max-width: 18ch;
    }
  }
</style>
