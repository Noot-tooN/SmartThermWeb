import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  ICON_FALLBACK_MS as STEP,
  StatusIconSequence,
} from './status-icon-sequence.svelte';
let sequence: StatusIconSequence;
beforeEach(() => {
  vi.useFakeTimers();
  sequence = new StatusIconSequence();
});
afterEach(() => {
  sequence.dispose();
  vi.useRealTimers();
});

it('finishes each queued transition even when sending, cloud and confirmation arrive together', () => {
  sequence.update('adjusting', 'a', false);
  sequence.update('sending', 'a', false);
  sequence.update('cloud', 'a', true);
  sequence.update('synced', 'a', true);
  expect(sequence.display).toBe('sending');
  vi.advanceTimersByTime(STEP - 1);
  expect(sequence.display).toBe('sending');
  vi.advanceTimersByTime(1);
  expect(sequence.display).toBe('cloud');
  vi.advanceTimersByTime(STEP);
  expect(sequence.display).toBe('synced');
});
it('preserves supported intermediate milestones when rendering batches the whole result', () => {
  sequence.update('adjusting', 'a', false);
  sequence.update('synced', 'a', true);
  expect(sequence.display).toBe('sending');
  vi.advanceTimersByTime(STEP);
  expect(sequence.display).toBe('cloud');
  vi.advanceTimersByTime(STEP);
  expect(sequence.display).toBe('synced');
});
it('never invents a cloud acknowledgement after a lost reply', () => {
  sequence.update('adjusting', 'a', false);
  sequence.update('synced', 'a', false);
  expect(sequence.display).toBe('sending');
  vi.advanceTimersByTime(STEP);
  expect(sequence.display).toBe('synced');
});
it('discards stale completion when a new adjustment or retry supersedes the old intent', () => {
  sequence.update('adjusting', 'first', false);
  sequence.update('synced', 'first', true);
  vi.advanceTimersByTime(STEP);
  expect(sequence.display).toBe('cloud');
  sequence.update('adjusting', 'second', false);
  expect(sequence.display).toBe('adjusting');
  vi.advanceTimersByTime(STEP * 5);
  expect(sequence.display).toBe('adjusting');
});
it('does not restart the current fade on repeated edits or duplicate events', () => {
  sequence.update('synced', 'initial', false);
  sequence.update('adjusting', 'a', false);
  vi.advanceTimersByTime(100);
  sequence.update('adjusting', 'b', false);
  sequence.update('sending', 'b', false);
  sequence.update('sending', 'b', false);
  vi.advanceTimersByTime(STEP - 100);
  expect(sequence.display).toBe('sending');
});
it('shows an error immediately and never drains a previously queued success afterward', () => {
  sequence.update('adjusting', 'a', false);
  sequence.update('synced', 'a', true);
  vi.advanceTimersByTime(80);
  sequence.update('error', 'a', true);
  expect(sequence.display).toBe('error');
  vi.advanceTimersByTime(STEP * 5);
  expect(sequence.display).toBe('error');
  sequence.update('synced', 'a', true);
  expect(sequence.display).toBe('synced');
});
it('drops a queue when state regresses without a new local click', () => {
  sequence.update('adjusting', 'a', false);
  sequence.update('synced', 'a', true);
  sequence.update('sending', 'a', false);
  vi.advanceTimersByTime(STEP * 5);
  expect(sequence.display).toBe('sending');
});
it('shows initial state immediately and bypasses queued animations with reduced motion', () => {
  sequence.update('synced', 'initial', false);
  expect(sequence.display).toBe('synced');
  sequence.update('adjusting', 'a', false);
  sequence.update('synced', 'a', true);
  sequence.update('synced', 'a', true, true);
  expect(sequence.display).toBe('synced');
  vi.advanceTimersByTime(STEP * 5);
  expect(sequence.display).toBe('synced');
});
it('cancels queued work on disposal', () => {
  sequence.update('adjusting', 'a', false);
  sequence.update('synced', 'a', true);
  sequence.dispose();
  vi.advanceTimersByTime(STEP * 5);
  expect(sequence.display).toBe('sending');
});

it('advances on actual CSS completion and ignores completion from an outgoing layer', () => {
  sequence.update('adjusting', 'a', false);
  sequence.update('synced', 'a', true);
  sequence.finishTransition('adjusting');
  expect(sequence.display).toBe('sending');
  vi.advanceTimersByTime(220);
  sequence.finishTransition('sending');
  expect(sequence.display).toBe('cloud');
  sequence.finishTransition('sending');
  expect(sequence.display).toBe('cloud');
  sequence.finishTransition('cloud');
  expect(sequence.display).toBe('synced');
});
