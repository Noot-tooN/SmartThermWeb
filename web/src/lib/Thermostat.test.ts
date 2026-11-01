import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import Thermostat from './Thermostat.svelte';
import { Dashboard } from './dashboard.svelte';
import type { Phase, Snapshot } from './types';
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
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
it('renders immediate optimistic changes with accessible controls', async () => {
  vi.useFakeTimers();
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard });
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  await fireEvent.click(increase);
  await fireEvent.click(increase);
  await fireEvent.click(increase);
  expect(
    screen.getByLabelText('Desired target temperature').textContent,
  ).toContain('23.0');
  expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe(
    '1',
  );
  expect(screen.getByRole('progressbar').getAttribute('aria-valuetext')).toBe(
    'Updating',
  );
  expect(screen.getByText('21.5°C')).toBeTruthy();
  dashboard.dispose();
});
it('keeps the same updating strip through repeated clicks and stale acknowledgement events', async () => {
  vi.useFakeTimers();
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  const { container } = render(Thermostat, { dashboard });
  const fill = container.querySelector('.checkpoint-fill');
  const bar = screen.getByRole('progressbar');
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  for (let revision = 2; revision <= 4; revision++) {
    await fireEvent.click(increase);
    const destination = fill?.getAttribute('style');
    expect(destination).toContain('scaleX(0.3333333333333333)');
    flushSync(() =>
      dashboard.accept({
        ...initial,
        revision,
        command: {
          id: 1,
          target: 215,
          phase: 'awaiting_device',
          message: 'Old command acknowledged.',
        },
      }),
    );
    expect(container.querySelector('.checkpoint-fill')).toBe(fill);
    expect(fill?.getAttribute('style')).toBe(destination);
    expect(
      container
        .querySelector('.command-progress')
        ?.classList.contains('working'),
    ).toBe(true);
    expect(bar.getAttribute('aria-valuenow')).toBe('1');
    expect(bar.getAttribute('aria-valuetext')).toBe('Updating');
  }
  dashboard.dispose();
});

it('distinguishes cloud acceptance, device confirmation, and actionable failure', () => {
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard });
  const update = (phase: Phase, revision: number) =>
    flushSync(() =>
      dashboard.accept({
        ...initial,
        revision,
        desired: 230,
        command: {
          id: 1,
          target: 230,
          phase,
          message:
            phase === 'rejected'
              ? 'Request rejected. Check settings.'
              : 'Status detail.',
        },
      }),
    );
  const progress = screen
    .getByRole('progressbar')
    .closest('.command-progress')!;
  const bar = screen.getByRole('progressbar');
  const checkpoints = screen.getAllByRole('button', { name: /details$/ });
  update('awaiting_device', 2);
  expect(bar.getAttribute('aria-valuenow')).toBe('2');
  expect(bar.getAttribute('aria-valuetext')).toBe('Server acknowledged');
  expect(progress.classList.contains('working')).toBe(true);
  // A confirmed command must also match the current device observation.
  flushSync(() =>
    dashboard.accept({
      ...initial,
      revision: 3,
      desired: 230,
      confirmed: {
        ...initial.confirmed!,
        target_temperature: 230,
        applied_command: 1,
      },
      command: {
        id: 1,
        target: 230,
        phase: 'confirmed',
        message: 'Device confirmed.',
      },
    }),
  );
  expect(bar.getAttribute('aria-valuenow')).toBe('3');
  expect(bar.getAttribute('aria-valuetext')).toBe('Synced with device');
  expect(progress.classList.contains('working')).toBe(false);
  update('rejected', 4);
  expect(bar.getAttribute('aria-valuenow')).toBe('1');
  expect(bar.getAttribute('aria-valuetext')).toBe('Request rejected');
  expect(progress.classList.contains('working')).toBe(false);
  expect(
    screen.getByRole('button', { name: 'Retry desired target' }),
  ).toBeTruthy();
  expect(screen.getAllByRole('button', { name: /details$/ })).toEqual(
    checkpoints,
  );
  expect(screen.getByRole('progressbar')).toBe(bar);
  dashboard.dispose();
});

it('exposes checkpoint explanations on keyboard focus without changing the milestone', async () => {
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard });
  const cloud = screen.getByRole('button', {
    name: 'Server acknowledged details',
  });
  await fireEvent.focus(cloud);
  expect(screen.getByRole('tooltip').textContent).toContain(
    'Device application still needs confirmation',
  );
  expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe(
    '3',
  );
  await fireEvent.keyDown(cloud, { key: 'Escape' });
  expect(screen.queryByRole('tooltip')).toBeNull();
  dashboard.dispose();
});

it('shows cloud readings without temperature controls or a false synced checkpoint', async () => {
  vi.useFakeTimers();
  const request = vi.fn();
  const dashboard = new Dashboard(request);
  dashboard.accept({
    ...initial,
    provider: 'ocstat',
    can_set_target: false,
    desired: null,
  });
  render(Thermostat, { dashboard });
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  }) as HTMLButtonElement;
  expect(increase.disabled).toBe(true);
  await fireEvent.click(increase);
  dashboard.change(5);
  dashboard.retry();
  await vi.advanceTimersByTimeAsync(1000);
  expect(request).not.toHaveBeenCalled();
  expect(screen.queryByRole('progressbar')).toBeNull();
  expect(screen.getByText('Cloud-reported reading')).toBeTruthy();
  flushSync(() =>
    dashboard.accept({
      ...initial,
      revision: 2,
      provider: 'ocstat',
      can_set_target: false,
      desired: null,
      confirmed: { ...initial.confirmed!, target_temperature: 230 },
    }),
  );
  expect(
    screen.getByLabelText('Desired target temperature').textContent,
  ).toContain('23.0');
});
