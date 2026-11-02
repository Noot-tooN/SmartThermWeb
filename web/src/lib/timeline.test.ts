import { draftFromPreview, type Preview } from './schedule';
import { expect, it } from 'vitest';
import {
  moveBoundary,
  paint,
  periods,
  normalizeSchedule,
  newDraft,
  type Change,
} from './schedule';
const base: Change[] = [{ start_minute: 0, target_temperature: 185 }];
it('paints a range without gaps, then merges adjacent equal temperatures', () => {
  const changes = paint(periods(base), 420, 1410, 215);
  expect(changes).toEqual([
    { start_minute: 0, target_temperature: 185 },
    { start_minute: 420, target_temperature: 215 },
    { start_minute: 1410, target_temperature: 185 },
  ]);
  expect(paint(periods(changes), 0, 420, 215)).toEqual([
    { start_minute: 0, target_temperature: 215 },
    { start_minute: 1410, target_temperature: 185 },
  ]);
  expect(paint(periods(changes), 0, 1440, 200)).toEqual([
    { start_minute: 0, target_temperature: 200 },
  ]);
});
it('replaces a range spanning multiple blocks while preserving the surrounding temperatures', () => {
  const source = periods(paint(periods(base), 420, 1200, 215));
  const result = periods(paint(source, 360, 1260, 200));
  expect(result).toEqual([
    { start: 0, end: 360, temperature: 185 },
    { start: 360, end: 1260, temperature: 200 },
    { start: 1260, end: 1440, temperature: 185 },
  ]);
});
it('preserves 10-minute boundaries when another portion is edited', () => {
  const source = [
    { start_minute: 0, target_temperature: 185 },
    { start_minute: 560, target_temperature: 215 },
    { start_minute: 1410, target_temperature: 185 },
  ];
  expect(paint(periods(source), 600, 1200, 220)).toContainEqual({
    start_minute: 560,
    target_temperature: 215,
  });
  const moved = moveBoundary(periods(source), 1, 590);
  expect(moved[1].start_minute).toBe(590);
  expect(source[1].start_minute).toBe(560);
});
it('rejects a seventh change without mutating the draft', () => {
  const source = [0, 240, 480, 720, 960, 1200].map((start_minute, i) => ({
    start_minute,
    target_temperature: i % 2 ? 215 : 185,
  }));
  const before = JSON.stringify(source);
  expect(() =>
    normalizeSchedule({
      pattern: 'daily',
      groups: [
        { group: 'every_day', changes: paint(periods(source), 30, 60, 230) },
      ],
    }),
  ).toThrow('six temperature changes');
  expect(JSON.stringify(source)).toBe(before);
});
it('rejects boundary crossings and invalid ranges or temperatures', () => {
  const source = periods(paint(periods(base), 420, 1200, 215));
  expect(() => moveBoundary(source, 1, 1200)).toThrow();
  expect(() => paint(source, 420, 420, 215)).toThrow();
  expect(() => paint(source, 420, 1441, 215)).toThrow();
  expect(() => paint(source, 435, 600, 215)).toThrow();
  expect(() => paint(source, 420, 600, 216)).toThrow();
  expect(() => paint(source, 420, 600, 355)).toThrow();
});

it('omits a redundant midnight anchor from the payload but keeps editor data intact', () => {
  const s = newDraft('daily');
  s.groups[0].changes = paint(periods(base), 450, 1020, 215);
  const result = normalizeSchedule(s);
  expect(result.groups[0].changes).toEqual([
    { start_minute: 450, target_temperature: 215 },
    { start_minute: 1020, target_temperature: 185 },
  ]);
  expect(s.groups[0].changes).toHaveLength(3);
});
it('allows six actual changes even when the editor has a midnight starting block', () => {
  const s = newDraft('daily');
  s.groups[0].changes = [0, 240, 420, 660, 840, 1080, 1260].map(
    (start_minute, i) => ({
      start_minute,
      target_temperature: i % 2 ? 215 : 185,
    }),
  );
  expect(normalizeSchedule(s).groups[0].changes).toHaveLength(6);
});
it('retains midnight where weekday and weekend carry-over differs', () => {
  const s = newDraft('weekday_weekend');
  s.groups[0].changes = paint(periods(base), 420, 1320, 215);
  s.groups[1].changes = [{ start_minute: 0, target_temperature: 200 }];
  expect(normalizeSchedule(s).groups[0].changes[0]).toEqual(base[0]);
});

it('keeps actual overnight carry-over when loading an individual-day schedule', () => {
  const preview = {
    schedule: {
      pattern: 'weekly',
      groups: [
        {
          group: 'monday',
          changes: [{ start_minute: 420, target_temperature: 215 }],
        },
      ],
    },
    can_save: false,
    days: [
      {
        day: 'monday',
        periods: [
          {
            start_minute: 0,
            end_minute: 420,
            target_temperature: 175,
            continues_from_previous_day: true,
          },
          {
            start_minute: 420,
            end_minute: 1440,
            target_temperature: 215,
            continues_from_previous_day: false,
          },
        ],
      },
    ],
  } as Preview;
  expect(draftFromPreview(preview).groups[0].changes).toEqual([
    { start_minute: 0, target_temperature: 175 },
    { start_minute: 420, target_temperature: 215 },
  ]);
});
it('refuses to flatten different overnight periods into a misleading grouped bar', () => {
  const preview = {
    schedule: {
      pattern: 'weekday_weekend',
      groups: [
        {
          group: 'weekends',
          changes: [{ start_minute: 420, target_temperature: 215 }],
        },
      ],
    },
    can_save: false,
    days: ['saturday', 'sunday'].map((day, i) => ({
      day,
      periods: [
        {
          start_minute: 0,
          end_minute: 1440,
          target_temperature: i ? 215 : 175,
          continues_from_previous_day: false,
        },
      ],
    })),
  } as Preview;
  expect(() => draftFromPreview(preview)).toThrow('different overnight');
});
