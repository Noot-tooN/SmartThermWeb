export type Pattern = 'daily' | 'weekday_weekend' | 'weekly';
export const days = [
  'monday',
  'tuesday',
  'wednesday',
  'thursday',
  'friday',
  'saturday',
  'sunday',
] as const;
export type Day = (typeof days)[number];
export type Group = Day | 'every_day' | 'weekdays' | 'weekends';
export const groups: Record<Pattern, readonly Group[]> = {
  daily: ['every_day'],
  weekday_weekend: ['weekdays', 'weekends'],
  weekly: days,
};
export const labels: Record<Group, string> = {
  monday: 'Monday',
  tuesday: 'Tuesday',
  wednesday: 'Wednesday',
  thursday: 'Thursday',
  friday: 'Friday',
  saturday: 'Saturday',
  sunday: 'Sunday',
  every_day: 'Every day',
  weekdays: 'Weekdays',
  weekends: 'Weekends',
};
export interface Change {
  start_minute: number;
  target_temperature: number;
}
export interface Schedule {
  pattern: Pattern;
  groups: { group: Group; changes: Change[] }[];
}
export interface Preview {
  schedule: Schedule;
  can_save: false;
  days: {
    day: Day;
    periods: {
      start_minute: number;
      end_minute: number;
      target_temperature: number;
      continues_from_previous_day: boolean;
    }[];
  }[];
}
export function clock(minute: number): string {
  return `${Math.floor(minute / 60)
    .toString()
    .padStart(2, '0')}:${(minute % 60).toString().padStart(2, '0')}`;
}

export interface Period {
  start: number;
  end: number;
  temperature: number;
}
export function newDraft(pattern: Pattern): Schedule {
  return {
    pattern,
    groups: groups[pattern].map((group) => ({
      group,
      changes: [{ start_minute: 0, target_temperature: 185 }],
    })),
  };
}
export function periods(
  changes: Change[],
  previousTemperature = changes.at(-1)!.target_temperature,
): Period[] {
  const result: Period[] = [];
  if (changes[0].start_minute > 0)
    result.push({
      start: 0,
      end: changes[0].start_minute,
      temperature: previousTemperature,
    });
  changes.forEach((c, i) =>
    result.push({
      start: c.start_minute,
      end: changes[i + 1]?.start_minute ?? 1440,
      temperature: c.target_temperature,
    }),
  );
  return result;
}
function toChanges(blocks: Period[]): Change[] {
  const changes: Change[] = [];
  for (const block of blocks) {
    if (block.start >= block.end) continue;
    if (changes.at(-1)?.target_temperature !== block.temperature)
      changes.push({
        start_minute: block.start,
        target_temperature: block.temperature,
      });
  }
  return changes;
}
export function paint(
  blocks: Period[],
  start: number,
  end: number,
  temperature: number,
): Change[] {
  if (
    !Number.isInteger(start) ||
    !Number.isInteger(end) ||
    start < 0 ||
    end > 1440 ||
    start >= end ||
    start % 10 ||
    end % 10
  )
    throw new Error('Choose a range within the day, in 10-minute steps.');
  if (
    !Number.isInteger(temperature) ||
    temperature < 50 ||
    temperature > 350 ||
    temperature % 5
  )
    throw new Error('Choose 5–35°C in 0.5°C steps.');
  const result: Period[] = [];
  for (const b of blocks) {
    if (b.start < start) result.push({ ...b, end: Math.min(start, b.end) });
    if (b.start < end && b.end > start)
      result.push({
        start: Math.max(start, b.start),
        end: Math.min(end, b.end),
        temperature,
      });
    if (b.end > end) result.push({ ...b, start: Math.max(end, b.start) });
  }
  return toChanges(result);
}
export function moveBoundary(
  blocks: Period[],
  index: number,
  minute: number,
): Change[] {
  if (
    index <= 0 ||
    index >= blocks.length ||
    !Number.isInteger(minute) ||
    minute % 10 ||
    minute <= blocks[index - 1].start ||
    minute >= blocks[index].end
  )
    throw new Error('Keep the boundary between its neighbouring changes.');
  return toChanges(
    blocks.map((b, i) => ({
      ...b,
      ...(i === index - 1 ? { end: minute } : {}),
      ...(i === index ? { start: minute } : {}),
    })),
  );
}

/** Keep full-day editor data intact. Only the validated payload drops redundant midnight anchors. */
export function normalizeSchedule(schedule: Schedule): Schedule {
  const groupForDay = (day: number) =>
    schedule.pattern === 'daily'
      ? 'every_day'
      : schedule.pattern === 'weekday_weekend'
        ? day < 5
          ? 'weekdays'
          : 'weekends'
        : days[day];
  const result: Schedule = {
    pattern: schedule.pattern,
    groups: schedule.groups.map((g) => ({
      group: g.group,
      changes: g.changes.map((c) => ({ ...c })),
    })),
  };
  for (const group of result.groups) {
    if (group.changes.length > 1 && group.changes[0].start_minute === 0) {
      const preceding = days
        .map((_, i) => i)
        .filter((i) => groupForDay(i) === group.group)
        .map(
          (i) =>
            schedule.groups
              .find((g) => g.group === groupForDay((i + 6) % 7))!
              .changes.at(-1)!.target_temperature,
        );
      if (preceding.every((t) => t === group.changes[0].target_temperature))
        group.changes.shift();
    }
    if (group.changes.length > 6)
      throw new Error(
        'That would create more than six temperature changes. Merge a period or widen your selection.',
      );
  }
  return result;
}

export const patternChoices: { value: Pattern; label: string }[] = [
  { value: 'daily', label: 'Every day' },
  { value: 'weekday_weekend', label: 'Weekdays / weekends' },
  { value: 'weekly', label: 'Each day' },
];
export interface SchedulePreset {
  id: string;
  name: string;
  description: string;
  detail: string;
  changes: Change[];
}
// Editable starting points, never inferred device schedules or operating modes.
export const schedulePresets: SchedulePreset[] = [
  {
    id: 'winter',
    name: 'Winter day',
    description: '21.5°C · 07:00–22:00',
    detail: '18.5°C for the rest of the day',
    changes: [
      { start_minute: 0, target_temperature: 185 },
      { start_minute: 420, target_temperature: 215 },
      { start_minute: 1320, target_temperature: 185 },
    ],
  },
  {
    id: 'morning_evening',
    name: 'Morning & evening',
    description: '21.5°C · 06:30–08:30 and 17:00–22:00',
    detail: '18.5°C for the rest of the day',
    changes: [
      { start_minute: 0, target_temperature: 185 },
      { start_minute: 390, target_temperature: 215 },
      { start_minute: 510, target_temperature: 185 },
      { start_minute: 1020, target_temperature: 215 },
      { start_minute: 1320, target_temperature: 185 },
    ],
  },
  {
    id: 'summer',
    name: 'Summer',
    description: '18°C · all day',
    detail: 'Lower heating target; this is not Off',
    changes: [{ start_minute: 0, target_temperature: 180 }],
  },
];
