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
  vi.unstubAllGlobals();
});
it('renders immediate optimistic changes with accessible controls', async () => {
  vi.useFakeTimers();
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
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
  expect(screen.getByText('21.5°C', { selector: 'strong' })).toBeTruthy();
  dashboard.dispose();
});
it('keeps the same updating strip through repeated clicks and stale acknowledgement events', async () => {
  vi.useFakeTimers();
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  const { container } = render(Thermostat, {
    dashboard,
    progressStyle: 'checkpoints',
  });
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
    ).toBe(false);
    expect(bar.getAttribute('aria-valuenow')).toBe('1');
    expect(bar.getAttribute('aria-valuetext')).toBe('Updating');
  }
  dashboard.dispose();
});

it('distinguishes cloud acceptance, device confirmation, and actionable failure', () => {
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
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
  render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
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
  render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
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
  expect(screen.queryByText('Cloud-reported reading')).toBeNull();
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

it('keeps one steady debounce glow through repeated clicks and hands off to the Updating pulse', async () => {
  vi.useFakeTimers();
  const request = vi.fn().mockReturnValue(new Promise(() => {}));
  const dashboard = new Dashboard(request);
  dashboard.accept(initial);
  dashboard.streamConnected = true;
  const { container } = render(Thermostat, {
    dashboard,
    progressStyle: 'checkpoints',
  });
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  const fill = container.querySelector('.checkpoint-fill');
  await fireEvent.click(increase);
  const glow = container.querySelector('.debounce-glow');
  const activity = container.querySelectorAll('.checkpoint-activity')[0];
  expect(glow?.classList.contains('active')).toBe(true);
  await vi.advanceTimersByTimeAsync(200);
  await fireEvent.click(increase);
  expect(container.querySelector('.debounce-glow')).toBe(glow);
  expect(glow?.classList.contains('active')).toBe(true);
  expect(container.querySelectorAll('.checkpoint-activity')[0]).toBe(activity);
  expect(container.querySelector('.checkpoint-fill')).toBe(fill);
  await vi.advanceTimersByTimeAsync(349);
  expect(glow?.classList.contains('active')).toBe(true);
  expect(request).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(container.querySelector('.debounce-glow')).toBe(glow);
  expect(glow?.classList.contains('active')).toBe(false);
  expect(
    container.querySelector('.command-progress')?.classList.contains('working'),
  ).toBe(true);
  expect(request).toHaveBeenCalledTimes(1);
  await fireEvent.click(increase);
  expect(container.querySelector('.debounce-glow')).toBe(glow);
  expect(glow?.classList.contains('active')).toBe(true);
  expect(container.querySelectorAll('.checkpoint-activity')[0]).toBe(activity);
  expect(
    container.querySelector('.command-progress')?.classList.contains('working'),
  ).toBe(false);
  expect(dashboard.localPending).toBe(true);
  expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe(
    '1',
  );
  dashboard.dispose();
});

it('keeps cloud requested and device reported targets distinct with live controls disabled', () => {
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept({
    ...initial,
    provider: 'ocstat',
    can_set_target: false,
    desired: null,
    confirmed: {
      ...initial.confirmed!,
      target_temperature: 215,
      requested_temperature: 225,
      mode: { kind: 'unknown', raw: 'unverified' },
    },
  });
  render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
  expect(
    screen.getByLabelText('Desired target temperature').textContent,
  ).toContain('21.5');
  expect(screen.getByText(/Cloud requested 22.5°C/).textContent).toMatch(
    /Cloud\s+status 21.5°C/,
  );
  expect(
    screen
      .getByRole('button', {
        name: 'Increase target temperature by 0.5 degrees',
      })
      .hasAttribute('disabled'),
  ).toBe(true);
  expect(screen.getByText('Mode unavailable')).toBeTruthy();
  dashboard.dispose();
});

it('enables OCSTAT controls and sends a debounced target through the local API', async () => {
  vi.useFakeTimers();
  const writable: Snapshot = {
    ...initial,
    provider: 'ocstat',
    login: 'ready',
    confirmed: {
      ...initial.confirmed!,
      requested_temperature: 215,
      target_writable: true,
    },
  };
  const request = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({
      ...writable,
      revision: 2,
      desired: 220,
      command: { id: 1, target: 220, phase: 'queued', message: 'Queued' },
    }),
  });
  const dashboard = new Dashboard(request);
  dashboard.accept(writable);
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  expect(increase.hasAttribute('disabled')).toBe(false);
  await fireEvent.click(increase);
  await vi.advanceTimersByTimeAsync(350);
  expect(request).toHaveBeenCalledTimes(1);
  expect(JSON.parse(request.mock.calls[0][1].body).target_temperature).toBe(
    220,
  );
  expect(request.mock.calls[0][0]).toBe('/api/thermostat/target');
  flushSync(() =>
    dashboard.accept({
      ...writable,
      revision: 3,
      desired: 220,
      confirmed: {
        ...writable.confirmed!,
        target_temperature: 220,
        requested_temperature: 220,
        matched_command: 1,
      },
      command: {
        id: 1,
        target: 220,
        phase: 'target_matched',
        message: 'Targets match',
      },
    }),
  );
  expect(screen.getByRole('progressbar').getAttribute('aria-valuetext')).toBe(
    'Target matches device',
  );
  dashboard.dispose();
});

it('explains Auto overrides and follows observed mode without replacing controls', async () => {
  vi.useFakeTimers();
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
  const request = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({ ...auto, revision: 2, desired: 220 }),
  });
  const auto: Snapshot = {
    ...initial,
    provider: 'ocstat',
    login: 'ready',
    confirmed: {
      ...initial.confirmed!,
      mode: { kind: 'schedule' },
      target_writable: true,
    },
  };
  const dashboard = new Dashboard(request);
  dashboard.accept(auto);
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard });
  expect(screen.getByText('Auto')).toBeTruthy();
  expect(
    screen.getByText(/Adjustments temporarily override your schedule/),
  ).toBeTruthy();
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  expect(increase.hasAttribute('disabled')).toBe(false);
  await fireEvent.click(increase);
  await vi.advanceTimersByTimeAsync(350);
  expect(request).toHaveBeenCalledTimes(1);
  expect(JSON.parse(request.mock.calls[0][1].body).target_temperature).toBe(
    220,
  );
  expect(screen.getByText('Auto')).toBeTruthy(); // A click is not a mode observation.
  flushSync(() =>
    dashboard.accept({
      ...auto,
      revision: 3,
      desired: 220,
      confirmed: { ...auto.confirmed!, mode: { kind: 'temporary_override' } },
    }),
  );
  expect(screen.getByText('Auto · Override')).toBeTruthy();
  expect(screen.getByText(/Temporary override active/)).toBeTruthy();
  expect(
    screen.getByRole('button', {
      name: 'Increase target temperature by 0.5 degrees',
    }),
  ).toBe(increase);
  flushSync(() =>
    dashboard.accept({
      ...auto,
      revision: 4,
      can_set_target: false,
      confirmed: {
        ...auto.confirmed!,
        target_writable: false,
        mode: { kind: 'unknown' },
      },
    }),
  );
  expect(increase.hasAttribute('disabled')).toBe(true);
  expect(screen.getByText(/Controls unavailable in this mode/)).toBeTruthy();
  dashboard.dispose();
});

it.each([
  [50, true, false],
  [300, false, false],
  [350, false, true],
])(
  'applies Android range bounds at %i tenths',
  (target, decreaseDisabled, increaseDisabled) => {
    const dashboard = new Dashboard(vi.fn());
    dashboard.accept({ ...initial, desired: target });
    render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
    expect(
      screen
        .getByRole('button', {
          name: 'Decrease target temperature by 0.5 degrees',
        })
        .hasAttribute('disabled'),
    ).toBe(decreaseDisabled);
    expect(
      screen
        .getByRole('button', {
          name: 'Increase target temperature by 0.5 degrees',
        })
        .hasAttribute('disabled'),
    ).toBe(increaseDisabled);
    dashboard.dispose();
  },
);

it('reaches 35°C in half-degree steps and stops at the upper bound', async () => {
  vi.useFakeTimers();
  const request = vi.fn();
  const dashboard = new Dashboard(request);
  dashboard.accept({ ...initial, desired: 340 });
  dashboard.streamConnected = true;
  render(Thermostat, { dashboard, progressStyle: 'checkpoints' });
  const increase = screen.getByRole('button', {
    name: 'Increase target temperature by 0.5 degrees',
  });
  await fireEvent.click(increase);
  expect(dashboard.target).toBe(345);
  await fireEvent.click(increase);
  expect(dashboard.target).toBe(350);
  expect(increase.hasAttribute('disabled')).toBe(true);
  dashboard.change(5);
  expect(dashboard.target).toBe(350);
  dashboard.dispose();
  await vi.advanceTimersByTimeAsync(1000);
  expect(request).not.toHaveBeenCalled();
});
