import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { Dashboard } from './dashboard.svelte';
import Login from './Login.svelte';
import type { Snapshot } from './types';
const initial: Snapshot = {
  session: 'browser-login',
  revision: 1,
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
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
it('submits credentials only to the backend and clears the password immediately', async () => {
  const dashboard = new Dashboard();
  dashboard.accept(initial);
  let resolve!: (value: Response) => void;
  const request = vi.fn().mockReturnValue(
    new Promise<Response>((r) => {
      resolve = r;
    }),
  );
  vi.stubGlobal('fetch', request);
  const local = vi.spyOn(Storage.prototype, 'setItem');
  render(Login, { dashboard });
  await fireEvent.input(screen.getByLabelText('Email'), {
    target: { value: 'example@example.invalid' },
  });
  await fireEvent.input(screen.getByLabelText('Password'), {
    target: { value: 'TEST_PASSWORD_ONLY' },
  });
  await fireEvent.submit(
    screen.getByRole('button', { name: 'Sign in' }).closest('form')!,
  );
  expect(request).toHaveBeenCalledTimes(1);
  expect(request.mock.calls[0][0]).toBe('/api/login');
  expect(JSON.parse(request.mock.calls[0][1].body)).toEqual({
    email: 'example@example.invalid',
    password: 'TEST_PASSWORD_ONLY',
  });
  expect((screen.getByLabelText('Password') as HTMLInputElement).value).toBe(
    '',
  );
  expect(local).not.toHaveBeenCalled();
  expect(
    (screen.getByRole('button', { name: 'Connecting…' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  resolve({
    ok: true,
    json: async () => ({
      ...initial,
      session: 'authenticated',
      revision: 2,
      login: 'ready',
    }),
  } as Response);
  await vi.waitFor(() => expect(dashboard.state?.login).toBe('ready'));
});
it('shows a rejected login with an empty password field', () => {
  const dashboard = new Dashboard();
  dashboard.accept({
    ...initial,
    login: 'failed',
    read_error: 'authentication',
  });
  render(Login, { dashboard });
  expect(screen.getByRole('alert').textContent).toContain('not accepted');
  expect((screen.getByLabelText('Password') as HTMLInputElement).value).toBe(
    '',
  );
  expect(
    (screen.getByRole('button', { name: 'Sign in' }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
});
it('uses a safe error instead of displaying response messages', async () => {
  const dashboard = new Dashboard();
  dashboard.accept(initial);
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: false,
      json: async () => ({
        error: {
          code: 'login_rate_limited',
          message: 'DO_NOT_RENDER_SECRET',
        },
      }),
    }),
  );
  render(Login, { dashboard });
  await fireEvent.input(screen.getByLabelText('Email'), {
    target: { value: 'example@example.invalid' },
  });
  await fireEvent.input(screen.getByLabelText('Password'), {
    target: { value: 'TEST_PASSWORD_ONLY' },
  });
  await fireEvent.submit(
    screen.getByRole('button', { name: 'Sign in' }).closest('form')!,
  );
  await vi.waitFor(() =>
    expect(screen.getByRole('alert').textContent).toContain('Wait 10 seconds'),
  );
  expect(screen.queryByText('DO_NOT_RENDER_SECRET')).toBeNull();
});
