import { clock, periods, type Change, type Period } from './schedule';

export const TIMELINE_START = 240;
export const timelineClock = (position: number) =>
  clock((position + TIMELINE_START) % 1440);
export const timelinePosition = (minute: number) =>
  (minute - TIMELINE_START + 1440) % 1440;

/** Rotate one calendar day's data. The right-hand early hours are the SAME day. */
export function timelinePeriods(changes: Change[]): Period[] {
  const result: Period[] = [];
  for (const offset of [0, 1440]) {
    for (const block of periods(changes)) {
      const start =
        Math.max(TIMELINE_START, block.start + offset) - TIMELINE_START;
      const end =
        Math.min(TIMELINE_START + 1440, block.end + offset) - TIMELINE_START;
      if (start >= end) continue;
      const previous = result.at(-1);
      if (previous?.end === start && previous.temperature === block.temperature)
        previous.end = end;
      else result.push({ start, end, temperature: block.temperature });
    }
  }
  return result;
}

/** Convert an edited display back to calendar minutes without changing day/group ownership. */
export function calendarChanges(changes: Change[]): Change[] {
  const midnight = timelinePosition(0);
  const midnightTemperature = periods(changes).find(
    (p) => p.start <= midnight && p.end > midnight,
  )!.temperature;
  const sorted = changes.map((c) => ({
    ...c,
    start_minute: (c.start_minute + TIMELINE_START) % 1440,
  }));
  if (!sorted.some((c) => c.start_minute === 0))
    sorted.push({ start_minute: 0, target_temperature: midnightTemperature });
  sorted.sort((a, b) => a.start_minute - b.start_minute);
  return sorted.filter(
    (c, i) =>
      i === 0 || c.target_temperature !== sorted[i - 1].target_temperature,
  );
}
