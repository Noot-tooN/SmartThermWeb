import type { Snapshot } from './types';

export type Request = typeof fetch;
export class Dashboard {
  state = $state<Snapshot | null>(null);
  optimistic = $state<number | null>(null);
  localPending = $state(false);
  submitting = $state(false);
  streamConnected = $state(false);
  error = $state('');
  loggingOut = $state(false);
  logoutError = $state('');
  authGeneration = $state(0);
  private edit = 0;
  private ready: { target: number; edit: number } | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private disposed = false;
  private retiredSessions = new Set<string>();
  private clientId = crypto.randomUUID();
  constructor(private request: Request = (...args) => fetch(...args)) {}

  get canSetTarget() {
    return this.state?.can_set_target === true;
  }
  get readError() {
    switch (this.state?.read_error) {
      case null:
      case undefined:
        return '';
      case 'authentication':
        return 'Your cloud session was rejected. Sign in again.';
      case 'device_not_found':
        return 'The configured thermostat was not returned by the cloud.';
      case 'invalid_response':
        return 'The cloud returned an unreadable device status. Showing the last known reading.';
      case 'offline':
        return 'The device is offline. Showing the last known reading.';
      default:
        return 'Cloud status is unavailable. Showing the last known reading; retrying automatically.';
    }
  }
  get target() {
    return (
      this.optimistic ??
      this.state?.desired ??
      this.state?.confirmed?.target_temperature ??
      null
    );
  }
  get uncertain() {
    return ['rejected', 'transport_failed', 'timed_out', 'unknown'].includes(
      this.state?.command?.phase ?? '',
    );
  }

  accept(next: Snapshot, source: 'http' | 'stream' = 'http') {
    if (this.disposed || this.retiredSessions.has(next.session)) return;
    if (this.state && next.session !== this.state.session) {
      // Only the live stream establishes a replacement server session. Delayed HTTP
      // responses from a previous server process cannot roll the browser back.
      if (source !== 'stream') return;
      this.retiredSessions.add(this.state.session);
    } else if (this.state && next.revision <= this.state.revision) {
      return;
    }
    this.state = next;
    if (!next.can_set_target) {
      clearTimeout(this.timer);
      this.ready = null;
      this.optimistic = null;
      this.localPending = false;
    }
  }
  useSession(next: Snapshot) {
    this.authGeneration += 1;
    this.retiredSessions.clear();
    this.state = null;
    this.optimistic = null;
    this.localPending = false;
    this.ready = null;
    clearTimeout(this.timer);
    this.accept(next);
  }
  sessionExpired() {
    this.authGeneration += 1;
    this.state = null;
    this.optimistic = null;
    this.localPending = false;
    void this.refresh();
  }
  async refresh() {
    const generation = this.authGeneration;
    try {
      const response = await this.request('/api/session', {
        signal: AbortSignal.timeout(8000),
        cache: 'no-store',
      });
      if (!response.ok)
        throw new Error('State could not be loaded. Try Refresh.');
      const next = await response.json();
      if (this.disposed || generation !== this.authGeneration) return;
      if (this.state && next.session !== this.state.session)
        this.useSession(next);
      else this.accept(next);
      if (this.optimistic === null) this.error = '';
    } catch {
      if (!this.disposed && generation === this.authGeneration)
        this.error =
          'Cannot reach SmartThermWeb. Check the backend and try Refresh.';
    }
  }
  async logout() {
    if (
      this.loggingOut ||
      this.state?.provider !== 'ocstat' ||
      this.state.login === 'signing_out'
    )
      return;
    this.loggingOut = true;
    this.logoutError = '';
    try {
      const response = await this.request('/api/logout', {
        method: 'POST',
        cache: 'no-store',
        signal: AbortSignal.timeout(8000),
      });
      if (!response.ok) throw new Error();
      this.useSession(await response.json());
      this.error = '';
    } catch {
      if (!this.disposed)
        this.logoutError =
          'Could not confirm logout. Refresh to check the connection, then try again.';
    } finally {
      this.loggingOut = false;
    }
  }
  change(delta: number) {
    if (!this.canSetTarget || this.target === null) return;
    const target = Math.min(300, Math.max(50, this.target + delta));
    this.optimistic = target;
    this.localPending = true;
    this.error = '';
    this.edit += 1;
    this.ready = null;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.ready = { target, edit: this.edit };
      void this.drain();
    }, 350);
  }
  retry() {
    if (this.target !== null) this.change(0);
  }
  private async drain() {
    if (!this.canSetTarget || this.submitting || !this.ready || this.disposed)
      return;
    const pending = this.ready;
    this.ready = null;
    this.submitting = true;
    try {
      const response = await this.request('/api/thermostat/target', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          target_temperature: pending.target,
          client_id: this.clientId,
          client_revision: pending.edit,
        }),
        signal: AbortSignal.timeout(8000),
      });
      if (!response.ok) {
        const data = await response.json();
        throw new Error(data.error?.message ?? 'Request rejected. Try again.');
      }
      const next = (await response.json()) as Snapshot;
      this.accept(next);
      if (
        pending.edit === this.edit &&
        !this.disposed &&
        next.session === this.state?.session
      ) {
        this.optimistic = null;
        this.localPending = false;
        this.error = '';
      }
    } catch (error) {
      if (pending.edit === this.edit && !this.disposed) {
        this.localPending = false;
        this.error =
          error instanceof Error && error.name === 'Error'
            ? error.message
            : 'No reliable reply from SmartThermWeb. Outcome unknown. Refresh or retry.';
      }
    } finally {
      this.submitting = false;
      if (this.ready) void this.drain();
    }
  }
  dispose() {
    this.disposed = true;
    clearTimeout(this.timer);
    this.ready = null;
  }
}
