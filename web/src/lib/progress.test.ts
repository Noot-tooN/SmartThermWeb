import { describe, expect, it } from 'vitest';
import { commandProgress } from './progress';
import type { Phase, Snapshot } from './types';

const state: Snapshot = {
  session: 'test',
  revision: 1,
  provider: 'simulator',
  connectivity: 'online',
  last_sync: 123,
  desired: 230,
  confirmed: {
    room_temperature: 208,
    target_temperature: 215,
    mode: { kind: 'manual' },
    revision: 1,
    applied_command: null,
  },
  command: null,
  in_flight: null,
};
const input = (phase: Phase) => ({
  state: {
    ...state,
    command: { id: 1, target: 230, phase, message: 'Outcome detail' },
  },
  target: 230,
  localPending: false,
  error: '',
  streamConnected: true,
});

describe('checkpoint evidence', () => {
  it.each<[Phase, number]>([
    ['queued', 1],
    ['sending', 1],
    ['accepted_by_cloud', 2],
    ['awaiting_device', 2],
    ['rejected', 1],
    ['transport_failed', 1],
    ['timed_out', 1],
    ['unknown', 2],
  ])('%s stops at checkpoint %i', (phase, checkpoint) => {
    expect(commandProgress(input(phase)).checkpoint).toBe(checkpoint);
  });
  it('fills the bar only for a matching confirmed device observation', () => {
    const source = input('confirmed');
    expect(commandProgress(source).checkpoint).not.toBe(3);
    source.state.confirmed = {
      ...state.confirmed!,
      target_temperature: 230,
      applied_command: 1,
    };
    expect(commandProgress(source).checkpoint).toBe(3);
    source.state.connectivity = 'offline';
    expect(commandProgress(source)).toMatchObject({
      checkpoint: 2,
      issue: true,
    });
  });
  it('a new local edit overrides old acknowledgement and confirmation events', () => {
    for (const phase of ['awaiting_device', 'confirmed'] as const) {
      const source = input(phase);
      source.state.confirmed = { ...state.confirmed!, target_temperature: 230 };
      expect(
        commandProgress({ ...source, localPending: true, target: 235 }),
      ).toMatchObject({ checkpoint: 1, issue: false, retry: false });
    }
  });
  it('holds one Updating milestone throughout repeated selection, queueing, and sending', () => {
    for (const target of [220, 225, 230, 235]) {
      for (const phase of ['queued', 'sending'] as const) {
        const source = { ...input(phase), target };
        expect(
          commandProgress({ ...source, localPending: true }),
        ).toMatchObject({ checkpoint: 1, status: 'Updating' });
        expect(commandProgress(source)).toMatchObject({
          checkpoint: 1,
          status: 'Updating',
        });
      }
    }
  });
  it('only retreats from acknowledged or synced when a newer edit requires it', () => {
    const source = input('awaiting_device');
    expect(commandProgress(source).checkpoint).toBe(2);
    expect(
      commandProgress({ ...source, localPending: true, target: 235 })
        .checkpoint,
    ).toBe(1);
    source.state.command.phase = 'confirmed';
    source.state.confirmed = {
      ...state.confirmed!,
      target_temperature: 230,
      applied_command: 1,
    };
    expect(commandProgress(source).checkpoint).toBe(3);
    expect(
      commandProgress({ ...source, localPending: true, target: 235 })
        .checkpoint,
    ).toBe(1);
  });
  it('does not report synchronization while browser delivery or updates are uncertain', () => {
    expect(
      commandProgress({ ...input('confirmed'), error: 'No response' }),
    ).toMatchObject({ checkpoint: 1, issue: true, retry: true });
    expect(
      commandProgress({ ...input('confirmed'), streamConnected: false }),
    ).toMatchObject({ checkpoint: 2, issue: true, retry: false });
  });
});
