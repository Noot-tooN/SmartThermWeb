import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/svelte';
import Schedule from './Schedule.svelte';
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
function mockFetch() {
  const fetch = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    json: async () => ({ can_save: false }),
  });
  vi.stubGlobal('fetch', fetch);
  return fetch;
}
async function open() {
  render(Schedule);
  await vi.advanceTimersByTimeAsync(0);
}
async function customRange(group?: string) {
  const scope = group
    ? within(screen.getByRole('region', { name: `${group} schedule` }))
    : screen;
  await fireEvent.click(scope.getByText('Choose a preset'));
  await fireEvent.click(
    screen.getByRole('button', { name: 'Choose custom hours…' }),
  );
}
async function paintRange(group?: string) {
  await customRange(group);
  await fireEvent.click(
    screen.getByRole('button', { name: 'Set 21.5°C for this range' }),
  );
}
it('always shows the schedule without network traffic in draft mode', async () => {
  const fetch = mockFetch();
  render(Schedule);
  expect(screen.getByRole('radio', { name: 'Every day' })).toBeTruthy();
  expect(
    screen.queryByRole('button', { name: 'Schedule Draft only' }),
  ).toBeNull();
  await vi.advanceTimersByTimeAsync(0);
  expect(
    screen.getByText(
      'Try a new schedule. Nothing here changes your thermostat.',
    ),
  ).toBeTruthy();
  expect(
    screen.getByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeTruthy();
  expect(fetch).not.toHaveBeenCalled();
});
it('paints immediately, automatically checks the Rust model, and undoes the whole edit', async () => {
  const fetch = mockFetch();
  await open();
  await paintRange();
  expect(
    screen.getByRole('button', { name: '07:00 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
  await vi.advanceTimersByTimeAsync(300);
  expect(fetch).toHaveBeenCalledOnce();
  expect(fetch.mock.calls[0][0]).toBe('/api/thermostat/schedule/preview');
  expect(JSON.parse(fetch.mock.calls[0][1].body).groups[0].changes).toEqual([
    { start_minute: 420, target_temperature: 215 },
    { start_minute: 1320, target_temperature: 185 },
  ]);
  await fireEvent.click(screen.getByRole('button', { name: '↶ Undo' }));
  expect(
    screen.getByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeTruthy();
});
it('moves boundaries by keyboard and keeps direct selection accessible', async () => {
  mockFetch();
  await open();
  await paintRange();
  await fireEvent.keyDown(screen.getByRole('slider', { name: 'Boundary 1' }), {
    key: 'ArrowRight',
  });
  expect(
    screen.getByRole('button', { name: '07:30 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
  await fireEvent.click(
    screen.getByRole('button', { name: '22:00 to 24:00, 18.5 degrees' }),
  );
  await fireEvent.click(
    screen.getByRole('button', { name: 'Increase period temperature' }),
  );
  expect(
    screen.getByRole('button', { name: '22:00 to 24:00, 19.0 degrees' }),
  ).toBeTruthy();
});
it('keeps independent drafts when switching patterns and day groups', async () => {
  mockFetch();
  await open();
  await paintRange();
  await fireEvent.click(
    screen.getByRole('radio', { name: 'Weekdays / weekends' }),
  );
  await paintRange('Weekends');
  expect(
    screen.getByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeTruthy();
  await fireEvent.click(screen.getByRole('radio', { name: 'Every day' }));
  expect(
    screen.getByRole('button', { name: '07:00 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
});
it('ignores stale validation errors and retains edits after a current failure', async () => {
  let resolve!: (v: unknown) => void;
  const fetch = vi
    .fn()
    .mockReturnValueOnce(new Promise((r) => (resolve = r)))
    .mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({ error: { message: 'Current validation error' } }),
    });
  vi.stubGlobal('fetch', fetch);
  await open();
  await paintRange();
  await vi.advanceTimersByTimeAsync(300);
  expect(fetch).toHaveBeenCalledOnce();
  await fireEvent.click(
    screen.getByRole('button', { name: 'Increase period temperature' }),
  );
  resolve({
    ok: false,
    status: 400,
    json: async () => ({ error: { message: 'Stale error' } }),
  });
  await vi.advanceTimersByTimeAsync(300);
  expect(screen.getByText('Current validation error')).toBeTruthy();
  expect(screen.queryByText('Stale error')).toBeNull();
  expect(
    screen.getByRole('button', { name: '07:00 to 22:00, 22.0 degrees' }),
  ).toBeTruthy();
});
it('expires a rejected browser session', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ status: 401 }));
  const expired = vi.fn();
  render(Schedule, { onExpired: expired });
  await vi.advanceTimersByTimeAsync(0);
  await paintRange();
  await vi.advanceTimersByTimeAsync(300);
  expect(expired).toHaveBeenCalledOnce();
});

it('cancels pointer gestures without edits and commits a completed drag as one undo step', async () => {
  mockFetch();
  await open();
  const timeline = document.querySelector('.timeline') as HTMLDivElement;
  timeline.setPointerCapture = vi.fn();
  vi.spyOn(timeline, 'getBoundingClientRect').mockReturnValue({
    left: 0,
    top: 0,
    width: 480,
    height: 86,
    right: 480,
    bottom: 86,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  async function pointer(type: string, x: number, target: Element = timeline) {
    const event = new MouseEvent(type, {
      bubbles: true,
      clientX: x,
      button: 0,
    });
    Object.defineProperties(event, {
      pointerId: { value: 1 },
      isPrimary: { value: true },
    });
    await fireEvent(target, event);
  }
  await pointer('pointerdown', 100);
  await pointer('pointermove', 200);
  await pointer('pointercancel', 200);
  expect(screen.queryByText('Selected range')).toBeNull();
  expect(
    screen.getByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeTruthy();
  await pointer('pointerdown', 100);
  await pointer('pointermove', 200);
  await pointer('pointerup', 200);
  expect(screen.getByRole('button', { name: /05:00–10:00/ })).toBeTruthy();
  await fireEvent.click(
    screen.getByRole('button', { name: 'Set 21.5°C for this range' }),
  );
  const handle = screen.getByRole('slider', { name: 'Boundary 1' });
  handle.setPointerCapture = vi.fn();
  await pointer('pointerdown', 100, handle);
  await pointer('pointermove', 120, handle);
  await pointer('pointercancel', 120, handle);
  expect(
    screen.getByRole('button', { name: '05:00 to 10:00, 21.5 degrees' }),
  ).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: '↶ Undo' }));
  expect(
    screen.getByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeTruthy();
});

it('shows snapped hover time without changing the draft', async () => {
  const fetch = mockFetch();
  await open();
  const bar = document.querySelector('.timeline') as HTMLDivElement;
  vi.spyOn(bar, 'getBoundingClientRect').mockReturnValue({
    left: 0,
    width: 480,
  } as DOMRect);
  await fireEvent(
    bar,
    new MouseEvent('pointermove', { bubbles: true, clientX: 190 }),
  );
  expect(document.querySelector('.hover-time')?.textContent).toBe('09:30');
  expect(
    screen.queryByRole('group', { name: 'Edit selected period' }),
  ).toBeNull();
  expect(fetch).not.toHaveBeenCalled();
  await fireEvent.pointerLeave(bar);
  expect(document.querySelector('.hover-time')).toBeNull();
});
it('adjusts pending range handles before applying and Escape discards it', async () => {
  const fetch = mockFetch();
  await open();
  await customRange();
  await fireEvent.keyDown(
    screen.getByRole('slider', { name: 'Selection start' }),
    { key: 'ArrowRight' },
  );
  await fireEvent.keyDown(
    screen.getByRole('slider', { name: 'Selection end' }),
    { key: 'ArrowLeft' },
  );
  expect(
    screen
      .getByRole('slider', { name: 'Selection start' })
      .getAttribute('aria-valuetext'),
  ).toBe('07:30');
  expect(
    screen
      .getByRole('slider', { name: 'Selection end' })
      .getAttribute('aria-valuetext'),
  ).toBe('21:30');
  expect(fetch).not.toHaveBeenCalled();
  await fireEvent.keyDown(window, { key: 'Escape' });
  expect(screen.queryByRole('slider', { name: 'Selection start' })).toBeNull();
  expect(
    screen.getByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeTruthy();
});
it('keeps exact time controls contextual and removes the permanent counter and editor', async () => {
  mockFetch();
  await open();
  expect(screen.queryByLabelText('Period start')).toBeNull();
  expect(screen.queryByText(/changes ·/)).toBeNull();
  await paintRange();
  await fireEvent.click(screen.getByRole('button', { name: /07:00–22:00/ }));
  await fireEvent.change(screen.getByLabelText('Period start'), {
    target: { value: '560' },
  });
  expect(
    screen.getByRole('button', { name: '09:20 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
  await fireEvent.click(
    screen.getByRole('button', { name: 'Close period editor' }),
  );
  expect(screen.queryByLabelText('Period start')).toBeNull();
});

it('shows both snapped endpoints throughout a drag in either direction, before release', async () => {
  const fetch = mockFetch();
  await open();
  const bar = document.querySelector('.timeline') as HTMLDivElement;
  bar.setPointerCapture = vi.fn();
  vi.spyOn(bar, 'getBoundingClientRect').mockReturnValue({
    left: 0,
    width: 480,
  } as DOMRect);
  async function pointer(type: string, x: number) {
    const event = new MouseEvent(type, {
      bubbles: true,
      clientX: x,
      button: 0,
    });
    Object.defineProperties(event, {
      pointerId: { value: 1 },
      isPrimary: { value: true },
    });
    await fireEvent(bar, event);
  }
  await pointer('pointerdown', 100);
  expect(
    screen.getByRole('status', { name: 'Drawing range' }).textContent,
  ).toBe('05:00–05:00');
  await pointer('pointermove', 200);
  expect(
    screen.getByRole('status', { name: 'Drawing range' }).textContent,
  ).toBe('05:00–10:00');
  await pointer('pointermove', 50);
  expect(
    screen.getByRole('status', { name: 'Drawing range' }).textContent,
  ).toBe('02:30–05:00');
  await pointer('pointermove', 0);
  expect(
    screen.getByRole('status', { name: 'Drawing range' }).textContent,
  ).toBe('00:00–05:00');
  await pointer('pointerup', 0);
  expect(screen.queryByRole('status', { name: 'Drawing range' })).toBeNull();
  expect(
    screen
      .getByRole('slider', { name: 'Selection start' })
      .getAttribute('aria-valuetext'),
  ).toBe('00:00');
  expect(
    screen
      .getByRole('slider', { name: 'Selection end' })
      .getAttribute('aria-valuetext'),
  ).toBe('05:00');
  await pointer('pointerdown', 400);
  await pointer('pointermove', 480);
  expect(
    screen.getByRole('status', { name: 'Drawing range' }).textContent,
  ).toBe('20:00–24:00');
  await pointer('pointercancel', 480);
  expect(screen.queryByRole('status', { name: 'Drawing range' })).toBeNull();
  expect(fetch).not.toHaveBeenCalled();
});

it('shows preset temperatures and hours before selection, with no request on browsing', async () => {
  const fetch = mockFetch();
  await open();
  expect(screen.getAllByRole('radio')).toHaveLength(3);
  expect(
    (screen.getByRole('radio', { name: 'Every day' }) as HTMLInputElement)
      .checked,
  ).toBe(true);
  await fireEvent.click(screen.getByText('Choose a preset'));
  expect(screen.getByText('21.5°C · 07:00–22:00')).toBeTruthy();
  expect(screen.getByText('21.5°C · 06:30–08:30 and 17:00–22:00')).toBeTruthy();
  expect(screen.getByText('18°C · all day')).toBeTruthy();
  expect(
    screen.getByText('Lower heating target; this is not Off'),
  ).toBeTruthy();
  expect(fetch).not.toHaveBeenCalled();
});
it('applies a preset only to the selected day group and restores the draft with one Undo', async () => {
  mockFetch();
  await open();
  await fireEvent.click(
    screen.getByRole('radio', { name: 'Weekdays / weekends' }),
  );
  await fireEvent.click(
    within(screen.getByRole('region', { name: 'Weekends schedule' })).getByText(
      'Choose a preset',
    ),
  );
  expect(
    screen.getByText('Replaces weekends only. You can edit or undo it.'),
  ).toBeTruthy();
  await fireEvent.click(
    screen.getByRole('button', { name: 'Use Morning & evening preset' }),
  );
  expect(
    screen.getByRole('button', { name: '06:30 to 08:30, 21.5 degrees' }),
  ).toBeTruthy();
  expect(
    screen.getByRole('button', { name: '17:00 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
  expect(
    screen.getByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: '↶ Undo' }));
  expect(
    screen.getAllByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toHaveLength(2);
});
it('preset templates remain unchanged after individual edits', async () => {
  mockFetch();
  await open();
  await fireEvent.click(screen.getByText('Choose a preset'));
  await fireEvent.click(
    screen.getByRole('button', { name: 'Use Winter day preset' }),
  );
  await fireEvent.click(
    screen.getByRole('button', { name: '07:00 to 22:00, 21.5 degrees' }),
  );
  await fireEvent.click(
    screen.getByRole('button', { name: 'Increase period temperature' }),
  );
  await fireEvent.click(screen.getByText('Choose a preset'));
  await fireEvent.click(
    screen.getByRole('button', { name: 'Use Winter day preset' }),
  );
  expect(
    screen.getByRole('button', { name: '07:00 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
});

it('shows seven editable rows and discards edits across the entire week', async () => {
  const fetch = mockFetch();
  await open();
  await fireEvent.click(screen.getByRole('radio', { name: 'Each day' }));
  for (const day of [
    'Monday',
    'Tuesday',
    'Wednesday',
    'Thursday',
    'Friday',
    'Saturday',
    'Sunday',
  ]) {
    expect(
      screen.getByRole('region', { name: `${day} schedule` }),
    ).toBeTruthy();
  }
  await paintRange('Monday');
  await paintRange('Sunday');
  expect(
    screen.getAllByRole('button', { name: '07:00 to 22:00, 21.5 degrees' }),
  ).toHaveLength(2);
  await vi.advanceTimersByTimeAsync(300);
  const body = JSON.parse(fetch.mock.calls.at(-1)![1].body);
  expect(body.groups).toHaveLength(7);
  expect(
    body.groups[0].changes.some(
      (c: { target_temperature: number }) => c.target_temperature === 215,
    ),
  ).toBe(true);
  expect(
    body.groups[6].changes.some(
      (c: { target_temperature: number }) => c.target_temperature === 215,
    ),
  ).toBe(true);
  expect(
    (screen.getByRole('button', { name: 'Save schedule' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  await fireEvent.click(
    screen.getByRole('button', { name: 'Discard changes' }),
  );
  expect(
    screen.getAllByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toHaveLength(7);
  expect(
    (screen.getByRole('button', { name: '↶ Undo' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    (
      screen.getByRole('button', {
        name: 'Discard changes',
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
});
it('edits an inactive row boundary without changing the other rows', async () => {
  mockFetch();
  await open();
  await fireEvent.click(
    screen.getByRole('radio', { name: 'Weekdays / weekends' }),
  );
  await paintRange('Weekdays');
  await paintRange('Weekends');
  const weekdays = within(
    screen.getByRole('region', { name: 'Weekdays schedule' }),
  );
  const weekends = within(
    screen.getByRole('region', { name: 'Weekends schedule' }),
  );
  await fireEvent.keyDown(
    weekdays.getByRole('slider', { name: 'Boundary 1' }),
    { key: 'ArrowRight' },
  );
  expect(
    weekdays.getByRole('button', { name: '07:30 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
  expect(
    weekends.getByRole('button', { name: '07:00 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: '↶ Undo' }));
  expect(
    weekdays.getByRole('button', { name: '07:00 to 22:00, 21.5 degrees' }),
  ).toBeTruthy();
});

function loadedPreview() {
  const changes = [
    { start_minute: 420, target_temperature: 215 },
    { start_minute: 1410, target_temperature: 185 },
  ];
  return {
    can_save: false,
    schedule: {
      pattern: 'weekday_weekend',
      groups: ['weekdays', 'weekends'].map((group) => ({ group, changes })),
    },
    days: [
      'monday',
      'tuesday',
      'wednesday',
      'thursday',
      'friday',
      'saturday',
      'sunday',
    ].map((day) => ({
      day,
      periods: [
        {
          start_minute: 0,
          end_minute: 420,
          target_temperature: 185,
          continues_from_previous_day: true,
        },
        {
          start_minute: 420,
          end_minute: 1410,
          target_temperature: 215,
          continues_from_previous_day: false,
        },
        {
          start_minute: 1410,
          end_minute: 1440,
          target_temperature: 185,
          continues_from_previous_day: false,
        },
      ],
    })),
  };
}
it('loads real periods on mount and discards back to the loaded baseline', async () => {
  const fetch = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    json: async () => loadedPreview(),
  });
  vi.stubGlobal('fetch', fetch);
  render(Schedule, { live: true });
  await vi.advanceTimersByTimeAsync(0);
  expect(fetch.mock.calls[0][0]).toBe('/api/thermostat/schedule');
  expect(
    screen.getAllByRole('button', { name: '07:00 to 23:30, 21.5 degrees' }),
  ).toHaveLength(2);
  expect(
    screen.queryByRole('button', { name: '00:00 to 24:00, 18.5 degrees' }),
  ).toBeNull();
  const row = within(screen.getByRole('region', { name: 'Weekdays schedule' }));
  await fireEvent.click(
    row.getByRole('button', { name: '07:00 to 23:30, 21.5 degrees' }),
  );
  await fireEvent.click(
    screen.getByRole('button', { name: 'Increase period temperature' }),
  );
  expect(
    (
      screen.getByRole('button', {
        name: 'Reload schedule',
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  await fireEvent.click(
    screen.getByRole('button', { name: 'Discard changes' }),
  );
  expect(
    screen.getAllByRole('button', { name: '07:00 to 23:30, 21.5 degrees' }),
  ).toHaveLength(2);
  expect(
    (
      screen.getByRole('button', {
        name: 'Reload schedule',
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(false);
  expect(
    (screen.getByRole('button', { name: 'Save schedule' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
it('never presents sample periods after a failed live load and allows retry', async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce({ ok: false, status: 503 })
    .mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => loadedPreview(),
    });
  vi.stubGlobal('fetch', fetch);
  render(Schedule, { live: true });
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByRole('alert').textContent).toContain('Could not load');
  expect(
    screen.queryByRole('region', { name: 'Every day schedule' }),
  ).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry load' }));
  expect(
    screen.getAllByRole('button', { name: '07:00 to 23:30, 21.5 degrees' }),
  ).toHaveLength(2);
});
it('expires the browser session when schedule access is rejected', async () => {
  const expired = vi.fn();
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 401 }));
  render(Schedule, { live: true, onExpired: expired });
  await vi.advanceTimersByTimeAsync(0);
  expect(expired).toHaveBeenCalledOnce();
  expect(
    screen.queryByRole('region', { name: 'Every day schedule' }),
  ).toBeNull();
});
it('blocks reload across pattern changes until all draft edits are discarded', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => loadedPreview(),
    }),
  );
  render(Schedule, { live: true });
  await vi.advanceTimersByTimeAsync(0);
  expect(
    screen
      .getByRole('radio', { name: 'Weekdays / weekends' })
      .closest('label')
      ?.querySelector('.mode-marker.is-current'),
  ).toBeTruthy();
  expect(document.querySelectorAll('.mode-marker.is-current')).toHaveLength(1);
  await fireEvent.click(screen.getByRole('radio', { name: 'Each day' }));
  expect(
    screen
      .getByRole('radio', { name: 'Weekdays / weekends' })
      .closest('label')
      ?.querySelector('.mode-marker.is-current'),
  ).toBeTruthy();
  expect(document.querySelectorAll('.mode-marker.is-current')).toHaveLength(1);
  expect(
    (
      screen.getByRole('button', {
        name: 'Reload schedule',
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  await fireEvent.click(
    screen.getByRole('button', { name: 'Discard changes' }),
  );
  expect(
    screen.getByRole('region', { name: 'Weekdays schedule' }),
  ).toBeTruthy();
  expect(screen.queryByRole('region', { name: 'Monday schedule' })).toBeNull();
  expect(
    screen
      .getByRole('button', { name: 'Reload schedule' })
      .closest('.schedule-heading'),
  ).toBeTruthy();
});

it('shows changeover times without hovering and updates them after moving a boundary', async () => {
  mockFetch();
  await open();
  await paintRange();
  await fireEvent.click(
    screen.getByRole('button', { name: 'Close period editor' }),
  );
  const times = () =>
    [...document.querySelectorAll('.changeover-time')].map(
      (node) => node.textContent,
    );
  expect(times()).toEqual(['07:00', '22:00']);
  await fireEvent.keyDown(screen.getByRole('slider', { name: 'Boundary 1' }), {
    key: 'ArrowRight',
  });
  expect(times()).toEqual(['07:30', '22:00']);
  await fireEvent.click(screen.getByRole('button', { name: /Undo/ }));
  expect(times()).toEqual(['07:00', '22:00']);
});

it('reserves a skeleton immediately, then keeps loaded timelines in place during reload', async () => {
  let resolve!: (response: unknown) => void;
  const fetch = vi.fn().mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  vi.stubGlobal('fetch', fetch);
  const { container } = render(Schedule, { live: true });
  expect(container.querySelectorAll('.skeleton .timeline')).toHaveLength(2);
  expect(
    container.querySelector('.schedule-panel')?.getAttribute('aria-busy'),
  ).toBe('true');
  expect(screen.getByRole('status').textContent).toContain('Loading schedule');
  expect(screen.queryByRole('radio', { name: 'Every day' })).toBeNull();
  resolve({ ok: true, status: 200, json: async () => loadedPreview() });
  await vi.advanceTimersByTimeAsync(0);
  expect(container.querySelector('.skeleton')).toBeNull();
  const timeline = screen.getByRole('region', { name: 'Weekdays schedule' });
  await fireEvent.click(
    screen.getByRole('button', { name: 'Reload schedule' }),
  );
  expect(screen.getByRole('region', { name: 'Weekdays schedule' })).toBe(
    timeline,
  );
  expect(container.querySelector('.skeleton')).toBeNull();
  expect(screen.getByRole('status').classList.contains('sr-only')).toBe(true);
  resolve({ ok: true, status: 200, json: async () => loadedPreview() });
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByRole('region', { name: 'Weekdays schedule' })).toBe(
    timeline,
  );
  expect(
    container.querySelector('.schedule-panel')?.getAttribute('aria-busy'),
  ).toBe('false');
});

function savablePreview(changed = false) {
  const preview = JSON.parse(JSON.stringify(loadedPreview()));
  preview.can_save = true;
  preview.save_pending = false;
  if (changed) {
    preview.schedule.groups[0].changes[0].target_temperature = 220;
    preview.days
      .slice(0, 5)
      .forEach(
        (d: { periods: { target_temperature: number }[] }) =>
          (d.periods[1].target_temperature = 220),
      );
  }
  return preview;
}
async function editWeekdayTemperature() {
  const row = within(screen.getByRole('region', { name: 'Weekdays schedule' }));
  await fireEvent.click(
    row.getByRole('button', { name: '07:00 to 23:30, 21.5 degrees' }),
  );
  await fireEvent.click(
    screen.getByRole('button', { name: 'Increase period temperature' }),
  );
  await vi.advanceTimersByTimeAsync(300);
}
const ok = (body: unknown) => ({
  ok: true,
  status: 200,
  json: async () => body,
});
it('saves only on explicit click with the untouched baseline and resets undo after server acceptance', async () => {
  let resolve!: (v: unknown) => void;
  const fetch = vi
    .fn()
    .mockImplementation((_url: string, options?: RequestInit) => {
      if (options?.method === 'PUT') return new Promise((r) => (resolve = r));
      return Promise.resolve(ok(savablePreview()));
    });
  vi.stubGlobal('fetch', fetch);
  render(Schedule, { live: true });
  await vi.advanceTimersByTimeAsync(0);
  await editWeekdayTemperature();
  const button = screen.getByRole('button', {
    name: 'Save schedule',
  }) as HTMLButtonElement;
  expect(button.disabled).toBe(false);
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method === 'PUT'),
  ).toHaveLength(0);
  await fireEvent.click(button);
  expect(button.disabled).toBe(true);
  await fireEvent.click(button);
  const puts = fetch.mock.calls.filter(
    ([, options]) => options?.method === 'PUT',
  );
  expect(puts).toHaveLength(1);
  const body = JSON.parse(puts[0][1].body);
  expect(body.baseline).toEqual(savablePreview().schedule);
  expect(body.schedule).toEqual(savablePreview(true).schedule);
  resolve(
    ok({
      status: 'accepted',
      accepted_groups: ['weekdays'],
      current: savablePreview(true),
      message: 'Accepted by server',
    }),
  );
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByText('Accepted by server')).toBeTruthy();
  expect(
    (screen.getByRole('button', { name: '↶ Undo' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    (screen.getByRole('button', { name: 'Save schedule' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
it('keeps edits after a lost save reply and checks with GET without ever resending', async () => {
  let readback = savablePreview();
  const fetch = vi
    .fn()
    .mockImplementation((_url: string, options?: RequestInit) =>
      options?.method === 'PUT'
        ? Promise.reject(new TypeError('network error'))
        : Promise.resolve(ok(readback)),
    );
  vi.stubGlobal('fetch', fetch);
  render(Schedule, { live: true });
  await vi.advanceTimersByTimeAsync(0);
  await editWeekdayTemperature();
  await fireEvent.click(screen.getByRole('button', { name: 'Save schedule' }));
  await vi.advanceTimersByTimeAsync(0);
  expect(
    screen.getByRole('button', { name: '07:00 to 23:30, 22.0 degrees' }),
  ).toBeTruthy();
  expect(
    (screen.getByRole('button', { name: 'Save schedule' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  await fireEvent.click(screen.getByRole('button', { name: 'Check server' }));
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByText(/server differs from your draft/)).toBeTruthy();
  readback = savablePreview(true);
  await fireEvent.click(screen.getByRole('button', { name: 'Check server' }));
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByText('Accepted by server')).toBeTruthy();
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method === 'PUT'),
  ).toHaveLength(1);
});
it('keeps a conflicting draft and requires an explicit discard before loading a replacement', async () => {
  const fetch = vi
    .fn()
    .mockImplementation((_url: string, options?: RequestInit) =>
      Promise.resolve(
        ok(
          options?.method === 'PUT'
            ? {
                status: 'conflict',
                accepted_groups: [],
                current: savablePreview(),
                message: 'Server schedule changed. Your draft is kept.',
              }
            : savablePreview(),
        ),
      ),
    );
  vi.stubGlobal('fetch', fetch);
  render(Schedule, { live: true });
  await vi.advanceTimersByTimeAsync(0);
  await editWeekdayTemperature();
  await fireEvent.click(screen.getByRole('button', { name: 'Save schedule' }));
  await vi.advanceTimersByTimeAsync(0);
  expect(
    screen.getByRole('button', { name: '07:00 to 23:30, 22.0 degrees' }),
  ).toBeTruthy();
  await fireEvent.click(
    screen.getByRole('button', { name: 'Reload and discard edits' }),
  );
  await vi.advanceTimersByTimeAsync(0);
  expect(
    screen.queryByRole('button', { name: '07:00 to 23:30, 22.0 degrees' }),
  ).toBeNull();
});

it('allows time and grouping edits and submits the full desired schedule with its original baseline', async () => {
  let submitted: any;
  const fetch = vi
    .fn()
    .mockImplementation((_url: string, options?: RequestInit) => {
      if (options?.method === 'PUT') {
        submitted = JSON.parse(options.body as string);
        return Promise.resolve(
          ok({
            status: 'unverified',
            accepted_groups: [],
            message: 'Grouping changed; period save unverified.',
            current: {
              ...savablePreview(),
              schedule: submitted.schedule,
              save_pending: true,
            },
          }),
        );
      }
      return Promise.resolve(ok(savablePreview()));
    });
  vi.stubGlobal('fetch', fetch);
  render(Schedule, { live: true });
  await vi.advanceTimersByTimeAsync(0);
  const row = within(screen.getByRole('region', { name: 'Weekdays schedule' }));
  await fireEvent.keyDown(row.getByRole('slider', { name: 'Boundary 1' }), {
    key: 'ArrowRight',
  });
  await vi.advanceTimersByTimeAsync(300);
  expect(
    (screen.getByRole('button', { name: 'Save schedule' }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
  await fireEvent.click(screen.getByRole('radio', { name: 'Every day' }));
  await vi.advanceTimersByTimeAsync(300);
  expect(
    screen.getByRole('button', { name: '07:30 to 23:30, 21.5 degrees' }),
  ).toBeTruthy();
  expect(screen.getByText(/Switching grouping activates/)).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Save schedule' }));
  await vi.advanceTimersByTimeAsync(0);
  expect(submitted.baseline).toEqual(savablePreview().schedule);
  expect(submitted.schedule.pattern).toBe('daily');
  expect(submitted.schedule.groups[0].changes[0].start_minute).toBe(450);
  expect(
    screen
      .getByRole('radio', { name: 'Every day' })
      .closest('label')
      ?.querySelector('.mode-marker.is-current'),
  ).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Check server' })).toBeTruthy();
});
