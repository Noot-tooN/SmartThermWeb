import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Dashboard } from './dashboard.svelte';
import type { Snapshot } from './types';
export const snapshot = (revision = 1): Snapshot => ({
  session: 'server-a',
  revision,
  provider: 'simulator',
  can_set_target: true,
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
});
const response = (value: Snapshot) =>
  ({ ok: true, json: async () => value }) as Response;
let dashboards: Dashboard[] = [];
function setup(
  request = vi.fn<typeof fetch>().mockResolvedValue(response(snapshot(2))),
) {
  const dashboard = new Dashboard(request);
  dashboard.accept(snapshot());
  dashboards.push(dashboard);
  return { dashboard, request };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  dashboards.forEach((d) => d.dispose());
  dashboards = [];
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
describe('optimistic command flow', () => {
  it('calls browser fetch without binding it to the dashboard instance', async () => {
    let receiver: unknown;
    vi.stubGlobal('fetch', function (this: unknown) {
      receiver = this;
      return Promise.resolve(response(snapshot()));
    });
    const dashboard = new Dashboard();
    dashboards.push(dashboard);
    await dashboard.refresh();
    expect(receiver).not.toBe(dashboard);
    expect(dashboard.error).toBe('');
    expect(dashboard.state?.desired).toBe(215);
  });
  it('clears an initial loading error after a successful refresh', async () => {
    const request = vi
      .fn<typeof fetch>()
      .mockRejectedValueOnce(new TypeError('offline'))
      .mockResolvedValue(response(snapshot(2)));
    const { dashboard } = setup(request);
    await dashboard.refresh();
    expect(dashboard.error).toContain('Cannot reach');
    await dashboard.refresh();
    expect(dashboard.error).toBe('');
  });
  it('updates every click immediately and submits only the debounced latest target', async () => {
    const { dashboard, request } = setup();
    dashboard.change(5);
    expect(dashboard.target).toBe(220);
    dashboard.change(5);
    expect(dashboard.target).toBe(225);
    dashboard.change(5);
    expect(dashboard.target).toBe(230);
    expect(request).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(349);
    expect(request).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(request).toHaveBeenCalledTimes(1);
    expect(JSON.parse(request.mock.calls[0][1]!.body as string)).toMatchObject({
      target_temperature: 230,
      client_revision: 3,
    });
  });
  it('ignores old events without overwriting a local edit', () => {
    const { dashboard } = setup();
    dashboard.change(5);
    dashboard.accept({ ...snapshot(5), desired: 230 });
    dashboard.accept({ ...snapshot(3), desired: 200 });
    dashboard.accept({ ...snapshot(5), desired: 190 });
    expect(dashboard.state?.desired).toBe(230);
    expect(dashboard.target).toBe(220);
  });
  it('serializes browser requests and preserves a newer edit during an older reply', async () => {
    let resolve!: (response: Response) => void;
    const request = vi
      .fn<typeof fetch>()
      .mockImplementationOnce(
        () =>
          new Promise((r) => {
            resolve = r;
          }),
      )
      .mockResolvedValue(response({ ...snapshot(3), desired: 230 }));
    const { dashboard } = setup(request);
    dashboard.change(5);
    await vi.advanceTimersByTimeAsync(350);
    dashboard.change(5);
    dashboard.change(5);
    await vi.advanceTimersByTimeAsync(350);
    expect(request).toHaveBeenCalledTimes(1);
    expect(dashboard.target).toBe(230);
    resolve(response({ ...snapshot(2), desired: 220 }));
    await vi.advanceTimersByTimeAsync(0);
    expect(request).toHaveBeenCalledTimes(2);
    expect(
      JSON.parse(request.mock.calls[1][1]!.body as string).target_temperature,
    ).toBe(230);
    expect(dashboard.target).toBe(230);
  });
  it('retains uncertain intent on a transport error and supports retry', async () => {
    const request = vi
      .fn<typeof fetch>()
      .mockRejectedValueOnce(new TypeError('Network error'))
      .mockResolvedValue(response({ ...snapshot(2), desired: 220 }));
    const { dashboard } = setup(request);
    dashboard.change(5);
    await vi.advanceTimersByTimeAsync(350);
    expect(dashboard.error).toContain('Outcome unknown');
    expect(dashboard.target).toBe(220);
    dashboard.retry();
    await vi.advanceTimersByTimeAsync(350);
    expect(dashboard.error).toBe('');
    expect(request).toHaveBeenCalledTimes(2);
  });
  it('accepts a server restart only from the stream and rejects retired sessions', () => {
    const { dashboard } = setup();
    dashboard.accept({ ...snapshot(1), session: 'server-b' }, 'stream');
    dashboard.accept(snapshot(99));
    dashboard.accept(snapshot(99), 'stream');
    expect(dashboard.state?.session).toBe('server-b');
    dashboard.accept(
      { ...snapshot(2), session: 'server-b', desired: 220 },
      'stream',
    );
    expect(dashboard.target).toBe(220);
  });
  it('clamps controls to the supported range and cancels debounce on disposal', async () => {
    const { dashboard, request } = setup();
    dashboard.accept({ ...snapshot(2), desired: 300 });
    dashboard.change(5);
    expect(dashboard.target).toBe(300);
    dashboard.dispose();
    await vi.advanceTimersByTimeAsync(1000);
    expect(request).not.toHaveBeenCalled();
  });
});

it('cancels pending edits when the server changes to a read-only provider', async () => {
  const { dashboard, request } = setup();
  dashboard.change(5);
  dashboard.accept(
    {
      ...snapshot(1),
      session: 'read-only',
      provider: 'ocstat',
      can_set_target: false,
      desired: null,
    },
    'stream',
  );
  await vi.advanceTimersByTimeAsync(1000);
  expect(request).not.toHaveBeenCalled();
  expect(dashboard.target).toBe(215);
  expect(dashboard.localPending).toBe(false);
});
