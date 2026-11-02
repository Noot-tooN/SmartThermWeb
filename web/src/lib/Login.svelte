<script lang="ts">
  import type { Dashboard } from './dashboard.svelte';
  let { dashboard }: { dashboard: Dashboard } = $props();
  let email = $state('');
  let password = $state('');
  let sending = $state(false);
  let error = $state('');
  let busy = $derived(sending || dashboard.state?.login === 'signing_in');
  let failure = $derived.by(() => {
    if (dashboard.state?.login !== 'failed') return '';
    switch (dashboard.state.read_error) {
      case 'authentication':
        return 'Email or password was not accepted. Please try again.';
      case 'device_not_found':
        return 'No thermostat was returned for this account.';
      case 'multiple_devices':
        return 'This account has several thermostats. Multiple-device selection is not supported yet.';
      default:
        return 'Could not connect to your thermostat. Check your connection and try again.';
    }
  });
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    sending = true;
    error = '';
    // Clear the password input immediately; never put credentials in browser storage.
    const body = JSON.stringify({ email: email.trim(), password });
    password = '';
    try {
      const response = await fetch('/api/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
        cache: 'no-store',
        signal: AbortSignal.timeout(40000),
      });
      const data = await response.json();
      if (!response.ok) {
        switch (data.error?.code) {
          case 'authentication':
            error = 'Your OCSTAT email or password was not accepted.';
            break;
          case 'device_not_found':
            error = 'No thermostat was returned for this OCSTAT account.';
            break;
          case 'multiple_devices':
            error = 'Multiple-device selection is not supported yet.';
            break;
          case 'login_rate_limited':
            error = 'Wait 10 seconds before trying again.';
            break;
          case 'invalid_login':
            error = 'Enter your email and password.';
            break;
          case 'login_unavailable':
            error = 'You are already signed in. Refresh the page.';
            break;
          default:
            error =
              'Could not start login. Refresh this local page and try again.';
        }
        return;
      }
      dashboard.useSession(data);
      email = '';
    } catch {
      error = 'Could not connect. Refresh to check whether sign-in completed.';
    } finally {
      sending = false;
    }
  }
</script>

<section
  class="thermostat login-card"
  aria-labelledby="login-title"
  aria-busy={busy}
>
  <h1 id="login-title">Sign in to OCSTAT</h1>
  <p class="muted">Use the account you use in the SmartTherm app.</p>
  <form onsubmit={submit}>
    <label
      >Email<input
        name="email"
        type="email"
        autocomplete="username"
        bind:value={email}
        maxlength="1024"
        required
        disabled={busy}
      /></label
    >
    <label
      >Password<input
        name="password"
        type="password"
        autocomplete="current-password"
        bind:value={password}
        maxlength="1024"
        required
        disabled={busy}
      /></label
    >
    <button class="apply-button" type="submit" disabled={busy}
      >{busy ? 'Connecting…' : 'Sign in'}</button
    >
    {#if error || failure}<p class="error" role="alert">
        {error || failure}
      </p>{/if}
    {#if busy}<p class="muted" role="status">
        Signing in and loading your thermostat…
      </p>{/if}
  </form>
</section>

<style>
  .login-card {
    max-width: 480px;
    margin: 0 auto;
  }
  .login-card > p {
    font-size: 13px;
    line-height: 1.6;
  }
  form {
    display: grid;
    gap: 20px;
    margin-top: 28px;
  }
  form p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
  }
</style>
