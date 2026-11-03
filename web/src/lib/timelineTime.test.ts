import { expect, it } from 'vitest';
import {
  calendarChanges,
  timelinePeriods,
  timelinePosition,
  timelineClock,
} from './timelineTime';
import { paint, periods, type Change } from './schedule';

const day: Change[] = [
  { start_minute: 0, target_temperature: 170 },
  { start_minute: 130, target_temperature: 185 },
  { start_minute: 420, target_temperature: 215 },
  { start_minute: 1410, target_temperature: 190 },
];
it('rotates clock labels and preserves every minute without mutating the source', () => {
  const original = structuredClone(day);
  const display = timelinePeriods(day);
  for (let minute = 0; minute < 1440; minute++) {
    const position = timelinePosition(minute);
    expect(
      display.find((p) => p.start <= position && position < p.end)!.temperature,
    ).toBe(
      periods(day).find((p) => p.start <= minute && minute < p.end)!
        .temperature,
    );
  }
  expect(day).toEqual(original);
  expect(timelineClock(0)).toBe('04:00');
  expect(timelineClock(1200)).toBe('00:00');
  expect(timelineClock(1440)).toBe('04:00');
  expect(
    calendarChanges(
      display.map((p) => ({
        start_minute: p.start,
        target_temperature: p.temperature,
      })),
    ),
  ).toEqual(day);
});
it('maps a selection across midnight to late and early hours of the same calendar day', () => {
  const edited = calendarChanges(paint(timelinePeriods(day), 1170, 1260, 220));
  expect(edited).toEqual([
    { start_minute: 0, target_temperature: 220 },
    { start_minute: 60, target_temperature: 170 },
    ...day.slice(1, 3),
    { start_minute: 1410, target_temperature: 220 },
  ]);
});
it('keeps a uniform full day uniform and does not introduce a 04:00 change', () => {
  const uniform = [{ start_minute: 0, target_temperature: 185 }];
  expect(timelinePeriods(uniform)).toEqual([
    { start: 0, end: 1440, temperature: 185 },
  ]);
  expect(
    calendarChanges(paint(timelinePeriods(uniform), 0, 1440, 210)),
  ).toEqual([{ start_minute: 0, target_temperature: 210 }]);
});
