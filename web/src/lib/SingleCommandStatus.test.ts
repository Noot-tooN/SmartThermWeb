import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import Thermostat from './Thermostat.svelte';
import { Dashboard } from './dashboard.svelte';
import type { Snapshot } from './types';
import { ICON_FALLBACK_MS as STEP } from './status-icon-sequence.svelte';
const initial: Snapshot = {
  session: 'test',
  revision: 1,
  provider: 'simulator',
  can_set_target: true,
  login: null,
  read_error: null,
  connectivity: 'online',
  last_sync: 123,
  desired: 215,
  confirmed: {
    room_temperature: 208,
    target_temperature: 215,
    online: true,
    mode: { kind: 'manual' },
    revision: 1,
    applied_command: null,
  },
  command: null,
  in_flight: null,
};
let dashboard: Dashboard;
let preference: EventTarget & { matches: boolean };
beforeEach(() => {
  vi.useFakeTimers();
  preference = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => preference),
  );
  dashboard = new Dashboard(vi.fn().mockReturnValue(new Promise(() => {})));
  dashboard.accept(initial);
  dashboard.streamConnected = true;
});
afterEach(() => {
  cleanup();
  dashboard.dispose();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const statusButton = () => screen.getByRole('button', { name: /Show details/ });
const snapshot = (
  revision: number,
  phase: 'sending' | 'awaiting_device' | 'confirmed' | 'rejected',
  target = 230,
): Snapshot => ({
  ...initial,
  revision,
  desired: target,
  confirmed: {
    ...initial.confirmed!,
    target_temperature: phase === 'confirmed' ? target : 215,
    applied_command: phase === 'confirmed' ? 1 : null,
  },
  command: {
    id: 1,
    target,
    phase,
    message:
      phase === 'rejected' ? 'Request rejected.' : 'Device state updated.',
  },
});

it('legacy view uses one persistent icon and keeps target edits immediate with a steady debounce glow', async () => {
  const { container } = render(Thermostat, {
    dashboard,
    progressStyle: 'icon',
  });
  const button = statusButton();
  const layers = [...container.querySelectorAll('.icon-layer')];
  expect(container.querySelector('.checkpoint-strip')).toBeNull();
  expect(button.getAttribute('data-state')).toBe('synced');
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  await fireEvent.click(increase);
  await fireEvent.click(increase);
  await fireEvent.click(increase);
  expect(
    screen.getByLabelText('Desired target temperature').textContent,
  ).toContain('23.0');
  expect(statusButton()).toBe(button);
  expect(button.getAttribute('data-state')).toBe('adjusting');
  expect(
    container.querySelector('.settling-glow')?.classList.contains('active'),
  ).toBe(true);
  expect([...container.querySelectorAll('.icon-layer')]).toEqual(layers);
  await vi.advanceTimersByTimeAsync(350);
  expect(button.getAttribute('data-state')).toBe('sending');
  expect(
    container.querySelector('.settling-glow')?.classList.contains('active'),
  ).toBe(false);
});
it('queues batched verified states but cancels the old checkmark on a new click', async () => {
  render(Thermostat, { dashboard, progressStyle: 'icon' });
  flushSync(() => dashboard.accept(snapshot(2, 'sending')));
  flushSync(() => {
    dashboard.accept(snapshot(3, 'awaiting_device'));
    dashboard.accept(snapshot(4, 'confirmed'));
  });
  expect(dashboard.cloudAcknowledged).toBe(true);
  expect(statusButton().getAttribute('data-state')).toBe('sending');
  // Actual data and accessible status do not wait for the animation queue.
  expect(
    screen.getByLabelText('Desired target temperature').textContent,
  ).toContain('23.0');
  expect(statusButton().getAttribute('aria-label')).toContain('Synced');
  await vi.advanceTimersByTimeAsync(STEP);
  expect(statusButton().getAttribute('data-state')).toBe('cloud');
  await fireEvent.click(
    screen.getByRole('button', {
      name: 'Increase target temperature by 0.5 degrees',
    }),
  );
  expect(statusButton().getAttribute('data-state')).toBe('adjusting');
  await vi.advanceTimersByTimeAsync(1000);
  expect(statusButton().getAttribute('data-state')).toBe('sending');
});
it('preempts queued completion for errors and exposes retry immediately', async () => {
  render(Thermostat, { dashboard, progressStyle: 'icon' });
  flushSync(() => dashboard.accept(snapshot(2, 'sending')));
  flushSync(() => dashboard.accept(snapshot(3, 'awaiting_device')));
  flushSync(() => dashboard.accept(snapshot(4, 'confirmed')));
  flushSync(() => dashboard.accept(snapshot(5, 'rejected')));
  expect(statusButton().getAttribute('data-state')).toBe('error');
  expect(screen.getByRole('alert').textContent).toContain('Request rejected');
  expect(
    screen.getByRole('button', { name: 'Retry desired target' }),
  ).toBeTruthy();
  await vi.advanceTimersByTimeAsync(1000);
  expect(statusButton().getAttribute('data-state')).toBe('error');
});
it('shows disconnection promptly even while a local adjustment is debouncing', async () => {
  render(Thermostat, { dashboard, progressStyle: 'icon' });
  await fireEvent.click(
    screen.getByRole('button', {
      name: 'Increase target temperature by 0.5 degrees',
    }),
  );
  flushSync(() => {
    dashboard.streamConnected = false;
  });
  expect(statusButton().getAttribute('data-state')).toBe('error');
  expect(screen.getByRole('alert').textContent).toContain(
    'Live updates disconnected',
  );
});
it('provides focus and tap explanations and honors changes to reduced motion', async () => {
  render(Thermostat, { dashboard, progressStyle: 'icon' });
  await fireEvent.focus(statusButton());
  expect(screen.getByRole('tooltip').textContent).toContain(
    'device has confirmed',
  );
  await fireEvent.keyDown(statusButton(), { key: 'Escape' });
  expect(screen.queryByRole('tooltip')).toBeNull();
  await fireEvent.click(statusButton());
  expect(statusButton().getAttribute('aria-expanded')).toBe('true');
  flushSync(() => dashboard.accept(snapshot(2, 'sending')));
  flushSync(() => dashboard.accept(snapshot(3, 'awaiting_device')));
  flushSync(() => dashboard.accept(snapshot(4, 'confirmed')));
  flushSync(() => {
    preference.matches = true;
    preference.dispatchEvent(new Event('change'));
  });
  expect(statusButton().getAttribute('data-state')).toBe('synced');
});

it('invalidates a queued checkmark even when retrying the same numeric target', async () => {
  render(Thermostat, { dashboard, progressStyle: 'icon' });
  flushSync(() => dashboard.accept(snapshot(2, 'sending')));
  flushSync(() => dashboard.accept(snapshot(3, 'awaiting_device')));
  flushSync(() => dashboard.accept(snapshot(4, 'confirmed')));
  await vi.advanceTimersByTimeAsync(STEP);
  expect(statusButton().getAttribute('data-state')).toBe('cloud');
  const target = dashboard.target;
  flushSync(() => dashboard.retry());
  expect(dashboard.target).toBe(target);
  expect(statusButton().getAttribute('data-state')).toBe('adjusting');
  await vi.advanceTimersByTimeAsync(1000);
  expect(statusButton().getAttribute('data-state')).toBe('sending');
});

it('preempts completion when a device read fails, preserving the last reading', async () => {
  render(Thermostat, { dashboard, progressStyle: 'icon' });
  flushSync(() => dashboard.accept(snapshot(2, 'sending')));
  flushSync(() => dashboard.accept(snapshot(3, 'awaiting_device')));
  flushSync(() => dashboard.accept(snapshot(4, 'confirmed')));
  flushSync(() =>
    dashboard.accept({
      ...snapshot(5, 'confirmed'),
      connectivity: 'unavailable',
      read_error: 'transport',
    }),
  );
  expect(statusButton().getAttribute('data-state')).toBe('error');
  expect(screen.getByRole('alert').textContent).toContain(
    'Device status unavailable',
  );
  expect(
    screen.getByLabelText('Desired target temperature').textContent,
  ).toContain('23.0');
  await vi.advanceTimersByTimeAsync(1000);
  expect(statusButton().getAttribute('data-state')).toBe('error');
});
