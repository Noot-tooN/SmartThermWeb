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
  await fireEvent.click(
    screen.getByRole('button', { name: 'Schedule Draft only' }),
  );
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
it('starts collapsed without network traffic and identifies a new draft', async () => {
  const fetch = mockFetch();
  render(Schedule);
  expect(screen.queryByRole('radio', { name: 'Every day' })).toBeNull();
  await fireEvent.click(
    screen.getByRole('button', { name: 'Schedule Draft only' }),
  );
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
  await fireEvent.click(
    screen.getByRole('button', { name: 'Schedule Draft only' }),
  );
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
