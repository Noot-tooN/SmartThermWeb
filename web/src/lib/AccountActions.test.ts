import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { Dashboard } from './dashboard.svelte';
import AccountActions from './AccountActions.svelte';
import type { Snapshot } from './types';
const initial: Snapshot = {
  session: 'test',
  revision: 1,
  provider: 'ocstat',
  can_set_target: false,
  login: 'ready',
  read_error: null,
  connectivity: 'online',
  last_sync: null,
  confirmed: null,
  desired: null,
  command: null,
  in_flight: null,
};
afterEach(cleanup);
it('offers logout for a browser-authenticated account', async () => {
  const request = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({ ...initial, revision: 2, login: 'required' }),
  });
  const dashboard = new Dashboard(request);
  dashboard.accept(initial);
  render(AccountActions, { dashboard });
  await fireEvent.click(screen.getByRole('button', { name: 'Log out' }));
  expect(request).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('button', { name: 'Log out' })).toBeNull();
});
it('disables logout while the worker is clearing the session', () => {
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept({ ...initial, login: 'signing_out' });
  render(AccountActions, { dashboard });
  expect(
    (screen.getByRole('button', { name: 'Logging out…' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
