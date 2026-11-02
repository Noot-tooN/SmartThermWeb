import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import App from './App.svelte';
import type { Snapshot } from './lib/types';

const signedOut: Snapshot = {
  session: 'anonymous',
  revision: 0,
  provider: 'ocstat',
  can_set_target: false,
  login: 'required',
  read_error: null,
  connectivity: 'connecting',
  last_sync: null,
  confirmed: null,
  desired: null,
  command: null,
  in_flight: null,
};
const signedIn: Snapshot = {
  ...signedOut,
  session: 'signed-in',
  revision: 1,
  login: 'ready',
  can_set_target: true,
  connectivity: 'online',
  last_sync: 123,
  confirmed: {
    room_temperature: 208,
    target_temperature: 215,
    online: true,
    mode: { kind: 'unknown', raw: 'unverified' },
    revision: 1,
    applied_command: null,
  },
};
const response = (state: Snapshot) =>
  ({ ok: true, json: async () => state }) as Response;
class FakeEventSource extends EventTarget {
  static instances: FakeEventSource[] = [];
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close = vi.fn();
  constructor() {
    super();
    FakeEventSource.instances.push(this);
  }
}
function pendingSession() {
  let resolve!: (value: Response) => void;
  const request = vi.fn<typeof fetch>().mockReturnValueOnce(
    new Promise((r) => {
      resolve = r;
    }),
  );
  vi.stubGlobal('fetch', request);
  return { request, resolve };
}
beforeEach(() => {
  FakeEventSource.instances = [];
  vi.stubGlobal('EventSource', FakeEventSource);
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => Object.assign(new EventTarget(), { matches: false })),
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it('never mounts the thermostat or refresh button before the signed-out session resolves', async () => {
  const { resolve } = pendingSession();
  const { container } = render(App);
  expect(screen.getByRole('status').textContent).toContain('Loading');
  expect(container.querySelector('.thermostat')).toBeNull();
  expect(container.querySelector('footer')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Refresh readings' })).toBeNull();
  expect(screen.queryByText('CONNECTING')).toBeNull();
  expect(FakeEventSource.instances).toHaveLength(0);
  resolve(response(signedOut));
  await screen.findByRole('heading', { name: 'Sign in to OCSTAT' });
  expect(screen.queryByRole('button', { name: 'Refresh readings' })).toBeNull();
  expect(screen.queryByLabelText('Desired target temperature')).toBeNull();
  expect(FakeEventSource.instances).toHaveLength(0);
});

it('mounts the complete data screen once and preserves it through stream connection and refresh', async () => {
  const { request, resolve } = pendingSession();
  const { container } = render(App);
  expect(container.querySelector('.thermostat')).toBeNull();
  request.mockResolvedValue({ ok: false, status: 503 } as Response);
  resolve(response(signedIn));
  const target = await screen.findByLabelText('Desired target temperature');
  expect(target.textContent).toContain('21.5');
  expect(screen.queryByText('OCSTAT')).toBeNull();
  expect(screen.queryByText(/Cloud reports device online/)).toBeNull();
  expect(screen.queryByText(/Live updates connected/)).toBeNull();
  expect(screen.queryByRole('region', { name: 'Reading details' })).toBeNull();
  const info = screen.getByRole('button', { name: 'Reading details' });
  await fireEvent.click(info);
  expect(
    screen.getByRole('region', { name: 'Reading details' }).textContent,
  ).toContain('Last reading');
  expect(info.getAttribute('aria-expanded')).toBe('true');
  await fireEvent.keyDown(window, { key: 'Escape' });
  expect(screen.queryByRole('region', { name: 'Reading details' })).toBeNull();
  await fireEvent.click(info);
  await fireEvent.click(screen.getByRole('heading', { name: 'Thermostat' }));
  expect(info.getAttribute('aria-expanded')).toBe('false');
  const card = container.querySelector('.thermostat');
  const header = container.querySelector('header');
  expect(container.querySelector('footer')).toBeNull();
  await vi.waitFor(() => expect(FakeEventSource.instances).toHaveLength(1));
  flushSync(() => FakeEventSource.instances[0].onopen?.());
  expect(container.querySelector('.thermostat')).toBe(card);
  expect(container.querySelector('header')).toBe(header);
  expect(container.querySelector('footer')).toBeNull();
  expect(
    screen.queryByRole('heading', { name: 'Sign in to OCSTAT' }),
  ).toBeNull();
  let finish!: (value: Response) => void;
  request.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  const refresh = screen.getByRole('button', { name: 'Refresh readings' });
  expect(refresh.closest('.ambient')).toBeTruthy();
  await fireEvent.click(refresh);
  expect(refresh.hasAttribute('disabled')).toBe(true);
  expect(container.querySelector('.thermostat')).toBe(card);
  expect(target.textContent).toContain('21.5');
  finish(response({ ...signedIn, revision: 2 }));
  await vi.waitFor(() => expect(request).toHaveBeenCalledTimes(3));
  expect(request.mock.calls[1][0]).toBe('/api/thermostat/schedule');
  expect(container.querySelector('.thermostat')).toBe(card);
  await vi.waitFor(() => expect(refresh.hasAttribute('disabled')).toBe(false));
  request.mockResolvedValue(response({ ...signedIn, revision: 3 }));
  flushSync(() => FakeEventSource.instances[0].onerror?.());
  expect(
    screen.getByText('Connection interrupted. Reconnecting…'),
  ).toBeTruthy();
  flushSync(() => FakeEventSource.instances[0].onopen?.());
  expect(
    screen.queryByText('Connection interrupted. Reconnecting…'),
  ).toBeNull();
  expect(container.querySelector('.thermostat')).toBe(card);
});

it('offers retry on startup failure without showing either account screen', async () => {
  const request = vi
    .fn<typeof fetch>()
    .mockRejectedValueOnce(new TypeError('offline'))
    .mockResolvedValueOnce(response(signedOut));
  vi.stubGlobal('fetch', request);
  const { container } = render(App);
  await screen.findByRole('alert');
  expect(container.querySelector('.thermostat')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Refresh readings' })).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
  await screen.findByRole('heading', { name: 'Sign in to OCSTAT' });
  expect(screen.queryByRole('alert')).toBeNull();
});

it('removes old readings immediately while a revoked session is checked', async () => {
  const request = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(response(signedIn));
  vi.stubGlobal('fetch', request);
  render(App);
  await screen.findByLabelText('Desired target temperature');
  await vi.waitFor(() => expect(FakeEventSource.instances).toHaveLength(1));
  let finish!: (value: Response) => void;
  request.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  flushSync(() =>
    FakeEventSource.instances[0].dispatchEvent(new Event('signed_out')),
  );
  expect(screen.queryByLabelText('Desired target temperature')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Refresh readings' })).toBeNull();
  finish(response(signedOut));
  await screen.findByRole('heading', { name: 'Sign in to OCSTAT' });
});
