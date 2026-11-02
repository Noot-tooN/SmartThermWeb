import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import Thermostat from './Thermostat.svelte';
import { Dashboard } from './dashboard.svelte';
import type { Phase, Snapshot } from './types';
const initial: Snapshot = {
  session: 'test',
  revision: 1,
  provider: 'ocstat',
  can_set_target: true,
  login: 'ready',
  read_error: null,
  connectivity: 'online',
  last_sync: 123,
  desired: 215,
  confirmed: {
    room_temperature: 208,
    target_temperature: 215,
    requested_temperature: 215,
    online: true,
    mode: { kind: 'unknown' },
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
const buttons = () => screen.getAllByRole('button', { name: /Show details/ });
const update = (
  revision: number,
  phase: Phase,
  accepted = false,
): Snapshot => ({
  ...initial,
  revision,
  desired: 225,
  confirmed: {
    ...initial.confirmed!,
    target_temperature: 225,
    requested_temperature: 225,
    matched_command: 1,
  },
  command: {
    id: 1,
    target: 225,
    phase,
    server_accepted: accepted,
    message: 'Request rejected.',
  },
});
it('defaults to two fixed icons with no idle success checkmark', () => {
  const { container } = render(Thermostat, { dashboard });
  expect(buttons()).toHaveLength(2);
  expect(buttons()[0].getAttribute('data-state')).toBe('waiting');
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
  expect(screen.queryByText('Cloud reports')).toBeNull();
  expect(container.querySelector('.neutral .icon-layer')).toBeNull();
  expect(container.querySelector('.neutral .activity')).toBeNull();
});
it('keeps repeated clicks steady and discards an obsolete queued acceptance check', async () => {
  const { container } = render(Thermostat, { dashboard });
  const original = buttons();
  const layers = [...container.querySelectorAll('.icon-layer')];
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  await fireEvent.click(increase);
  await fireEvent.click(increase);
  expect(buttons()).toEqual(original);
  expect(buttons()[0].getAttribute('data-state')).toBe('adjusting');
  expect(container.querySelector('.settling-glow.active')).toBeTruthy();
  await vi.advanceTimersByTimeAsync(350);
  expect(buttons()[0].getAttribute('data-state')).toBe('sending');
  // A new browser's accepted snapshot, followed immediately by another local edit.
  flushSync(() => {
    dashboard.localPending = false;
    dashboard.optimistic = null;
    dashboard.accept(update(2, 'awaiting_device', true));
  });
  await fireEvent.click(increase);
  await vi.advanceTimersByTimeAsync(1200);
  expect(buttons()[0].getAttribute('data-state')).toBe('sending');
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
  expect([...container.querySelectorAll('.icon-layer')]).toEqual(layers);
});
it('checks only server acceptance while the thermostat stays neutral through matching values and timeout', async () => {
  render(Thermostat, { dashboard });
  flushSync(() => dashboard.accept(update(2, 'sending')));
  flushSync(() => dashboard.accept(update(3, 'awaiting_device', true)));
  await vi.advanceTimersByTimeAsync(1200);
  expect(buttons()[0].getAttribute('data-state')).toBe('synced');
  expect(buttons()[0].getAttribute('aria-label')).toContain(
    'Accepted by server',
  );
  for (const [revision, phase] of [
    [4, 'target_matched'],
    [5, 'unknown'],
  ] as const) {
    flushSync(() => dashboard.accept(update(revision, phase, true)));
    expect(buttons()[0].getAttribute('data-state')).toBe('synced');
    expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
  }
});
it('retains server acceptance across reload but never infers it from temperature matches', () => {
  dashboard.accept(update(2, 'target_matched', false));
  const first = render(Thermostat, { dashboard });
  expect(buttons()[0].getAttribute('data-state')).toBe('error');
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
  first.unmount();
  dashboard.dispose();
  dashboard = new Dashboard(vi.fn());
  dashboard.accept(update(3, 'target_matched', true));
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard });
  expect(buttons()[0].getAttribute('data-state')).toBe('synced');
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
});
it('keeps both icons mounted on read failure and disconnection without erasing known acceptance', async () => {
  dashboard.accept(update(2, 'target_matched', true));
  const { container } = render(Thermostat, { dashboard });
  const original = buttons();
  flushSync(() => {
    dashboard.accept({
      ...update(3, 'target_matched', true),
      can_set_target: false,
      connectivity: 'unavailable',
      read_error: 'transport',
    });
    dashboard.streamConnected = false;
  });
  expect(buttons()).toEqual(original);
  expect(buttons()[0].getAttribute('data-state')).toBe('synced');
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
  expect(container.querySelector('.activity.active')).toBeNull();
  await fireEvent.click(buttons()[1]);
  expect(screen.getByRole('tooltip').textContent).toContain(
    'cannot currently be verified',
  );
  expect(screen.getByRole('tooltip').textContent).toContain(
    'Cloud status is unavailable',
  );
});
it('preempts animation on rejection and provides independent keyboard/tap details and reduced motion', async () => {
  render(Thermostat, { dashboard });
  flushSync(() => dashboard.accept(update(2, 'sending')));
  flushSync(() => dashboard.accept(update(3, 'awaiting_device', true)));
  flushSync(() => dashboard.accept(update(4, 'rejected')));
  expect(buttons()[0].getAttribute('data-state')).toBe('error');
  expect(
    screen.getByRole('button', { name: 'Retry desired target' }),
  ).toBeTruthy();
  expect(screen.getByRole('alert').textContent).toContain('Request rejected');
  await fireEvent.focus(buttons()[1]);
  expect(screen.getByRole('tooltip').textContent).toContain(
    'Thermostat unverified',
  );
  await fireEvent.keyDown(buttons()[1], { key: 'Escape' });
  expect(screen.queryByRole('tooltip')).toBeNull();
  await fireEvent.click(buttons()[0]);
  expect(screen.getByRole('tooltip').textContent).toContain('Server rejected');
  flushSync(() => {
    preference.matches = true;
    preference.dispatchEvent(new Event('change'));
    dashboard.accept(update(5, 'awaiting_device', true));
  });
  expect(buttons()[0].getAttribute('data-state')).toBe('synced');
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
});

const applied = (revision = 2, accepted = true): Snapshot => ({
  ...update(revision, 'confirmed', accepted),
  provider: 'simulator',
  confirmed: {
    ...update(revision, 'confirmed', accepted).confirmed!,
    applied_command: 1,
  },
});
it('shows verified device application independently of a lost server reply', async () => {
  dashboard.accept(applied(2, false));
  render(Thermostat, { dashboard });
  expect(buttons()[0].getAttribute('data-state')).toBe('error');
  expect(buttons()[1].getAttribute('data-state')).toBe('verified');
  await fireEvent.click(buttons()[1]);
  expect(screen.getByRole('tooltip').textContent).toContain(
    'evidence linked to the latest command',
  );
});
it.each([
  [
    'missing evidence',
    (s: Snapshot) => {
      s.confirmed!.applied_command = null;
    },
  ],
  [
    'old command',
    (s: Snapshot) => {
      s.confirmed!.applied_command = 99;
    },
  ],
  [
    'cloud equality only',
    (s: Snapshot) => {
      s.command!.phase = 'target_matched';
    },
  ],
  [
    'different target',
    (s: Snapshot) => {
      s.confirmed!.target_temperature = 215;
    },
  ],
  [
    'offline reading',
    (s: Snapshot) => {
      s.confirmed!.online = false;
    },
  ],
  [
    'unavailable connection',
    (s: Snapshot) => {
      s.connectivity = 'unavailable';
    },
  ],
  [
    'failed read',
    (s: Snapshot) => {
      s.read_error = 'transport';
    },
  ],
] as const)('does not verify application with %s', (_, change) => {
  const state = applied();
  change(state);
  dashboard.accept(state);
  render(Thermostat, { dashboard });
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
});
it('withdraws old device confirmation on a new edit and preserves the mounted icons', async () => {
  dashboard.accept(applied());
  render(Thermostat, { dashboard });
  const original = buttons();
  expect(original[1].getAttribute('data-state')).toBe('verified');
  // Same-value retries still represent a new intent and must discard old evidence.
  flushSync(() => dashboard.retry());
  expect(buttons()).toEqual(original);
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
  await vi.advanceTimersByTimeAsync(1200);
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
});
it('withdraws device confirmation while live updates are disconnected', () => {
  dashboard.accept(applied());
  render(Thermostat, { dashboard });
  expect(buttons()[1].getAttribute('data-state')).toBe('verified');
  flushSync(() => {
    dashboard.streamConnected = false;
  });
  expect(buttons()[1].getAttribute('data-state')).toBe('unverified');
  expect(buttons()[0].getAttribute('aria-label')).toContain(
    'Accepted by server',
  );
});
