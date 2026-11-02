import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import { Dashboard } from './dashboard.svelte';
import AccountHeader from './AccountHeader.svelte';
import type { Snapshot } from './types';
const state: Snapshot = {
  session: 'test',
  revision: 1,
  provider: 'ocstat',
  can_set_target: false,
  account_email: 'owner@example.invalid',
  login: 'ready',
  read_error: null,
  connectivity: 'online',
  last_sync: null,
  desired: null,
  command: null,
  in_flight: null,
  confirmed: {
    device_name: 'Living room',
    room_temperature: 208,
    target_temperature: 215,
    online: true,
    mode: { kind: 'unknown' },
    revision: 1,
    applied_command: null,
  },
};
afterEach(cleanup);
it('shows only the signed-in account in the compact header', () => {
  const dashboard = new Dashboard(vi.fn());
  dashboard.accept(state);
  render(AccountHeader, { dashboard });
  expect(screen.getByText('owner@example.invalid')).toBeTruthy();
  expect(screen.queryByText('Living room')).toBeNull();
  flushSync(() =>
    dashboard.useSession({
      ...state,
      login: 'required',
      confirmed: null,
      account_email: null,
    }),
  );
  expect(screen.queryByText('owner@example.invalid')).toBeNull();
});

it('keeps the full account accessible when truncated', () => {
  const dashboard = new Dashboard(vi.fn());
  const email = 'long-account-name@example.invalid';
  dashboard.accept({ ...state, account_email: email });
  render(AccountHeader, { dashboard });
  expect(screen.getByText(email).getAttribute('title')).toBe(email);
});
