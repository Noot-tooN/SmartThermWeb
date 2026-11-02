//! Browser-facing schedule semantics. No manufacturer codes or network side effects.
use crate::domain::TargetTemperature;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Daily,
    WeekdayWeekend,
    Weekly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
    Weekdays,
    Weekends,
    EveryDay,
}
pub const DAYS: [Group; 7] = [
    Group::Monday,
    Group::Tuesday,
    Group::Wednesday,
    Group::Thursday,
    Group::Friday,
    Group::Saturday,
    Group::Sunday,
];
impl Pattern {
    pub fn groups(self) -> Vec<Group> {
        match self {
            Self::Daily => vec![Group::EveryDay],
            Self::WeekdayWeekend => vec![Group::Weekdays, Group::Weekends],
            Self::Weekly => DAYS.to_vec(),
        }
    }
    fn index_for_day(self, day: usize) -> usize {
        match self {
            Self::Daily => 0,
            Self::WeekdayWeekend => usize::from(day >= 5),
            Self::Weekly => day,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub start_minute: u16,
    pub target_temperature: TargetTemperature,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayGroup {
    pub group: Group,
    pub changes: Vec<Change>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub pattern: Pattern,
    pub groups: Vec<DayGroup>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Period {
    pub start_minute: u16,
    pub end_minute: u16,
    pub target_temperature: TargetTemperature,
    pub continues_from_previous_day: bool,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DayPreview {
    pub day: Group,
    pub periods: Vec<Period>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Preview {
    pub schedule: Schedule,
    pub days: Vec<DayPreview>,
    pub can_save: bool,
    /// A dispatched write has not yet been observed in a server read.
    #[serde(default)]
    pub save_pending: bool,
}

/// Baseline is the last loaded server model, never the independently edited draft.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    pub baseline: Schedule,
    pub schedule: Schedule,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SaveStatus {
    Accepted,
    Conflict,
    Unverified,
    Unsupported,
}
#[derive(Debug, Serialize)]
pub struct SaveResult {
    pub status: SaveStatus,
    pub accepted_groups: Vec<Group>,
    pub message: String,
    pub current: Option<Preview>,
}

impl Schedule {
    pub fn validate(mut self) -> Result<Self, &'static str> {
        let expected = self.pattern.groups();
        if self.groups.len() != expected.len() {
            return Err("Provide every day group for the selected pattern.");
        }
        // Accept arbitrary group/row order, then canonicalize it in one place.
        let mut ordered = Vec::new();
        for group in expected {
            let index = self
                .groups
                .iter()
                .position(|g| g.group == group)
                .ok_or("Day groups must match the selected pattern without duplicates.")?;
            let mut entry = self.groups.remove(index);
            if entry.changes.is_empty() || entry.changes.len() > 7 {
                return Err("Each day group needs between one and six temperature changes.");
            }
            if entry
                .changes
                .iter()
                .any(|c| c.start_minute >= 1440 || c.start_minute % 10 != 0)
            {
                return Err("Start times must be from 00:00 to 23:50, in 10-minute steps.");
            }
            entry.changes.sort_by_key(|c| c.start_minute);
            if entry
                .changes
                .windows(2)
                .any(|w| w[0].start_minute == w[1].start_minute)
            {
                return Err("Each temperature change must have a different start time.");
            }
            ordered.push(entry);
        }
        // Midnight is only a change when it differs from the previous actual day.
        // For grouped weekdays/weekends, check every day that uses that group.
        let previous_targets: Vec<_> = ordered
            .iter()
            .map(|g| g.changes.last().unwrap().target_temperature)
            .collect();
        for (index, group) in ordered.iter_mut().enumerate() {
            if group.changes.len() > 1
                && group.changes[0].start_minute == 0
                && (0..7)
                    .filter(|day| self.pattern.index_for_day(*day) == index)
                    .all(|day| {
                        previous_targets[self.pattern.index_for_day((day + 6) % 7)]
                            == group.changes[0].target_temperature
                    })
            {
                group.changes.remove(0);
            }
            if group.changes.len() > 6 {
                return Err("Each day group supports at most six temperature changes.");
            }
        }
        self.groups = ordered;
        Ok(self)
    }
    pub fn preview(self) -> Result<Preview, &'static str> {
        let schedule = self.validate()?;
        let mut days = Vec::new();
        for (day_index, day) in DAYS.into_iter().enumerate() {
            let changes = &schedule.groups[schedule.pattern.index_for_day(day_index)].changes;
            let previous =
                &schedule.groups[schedule.pattern.index_for_day((day_index + 6) % 7)].changes;
            let mut periods = Vec::new();
            if changes[0].start_minute > 0 {
                periods.push(Period {
                    start_minute: 0,
                    end_minute: changes[0].start_minute,
                    target_temperature: previous.last().unwrap().target_temperature,
                    continues_from_previous_day: true,
                });
            }
            for (index, change) in changes.iter().enumerate() {
                periods.push(Period {
                    start_minute: change.start_minute,
                    end_minute: changes.get(index + 1).map_or(1440, |c| c.start_minute),
                    target_temperature: change.target_temperature,
                    continues_from_previous_day: false,
                });
            }
            days.push(DayPreview { day, periods });
        }
        Ok(Preview {
            schedule,
            days,
            can_save: false,
            save_pending: false,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn changes(temp: i16) -> Vec<Change> {
        vec![
            Change {
                start_minute: 420,
                target_temperature: temp.try_into().unwrap(),
            },
            Change {
                start_minute: 1410,
                target_temperature: (temp - 30).try_into().unwrap(),
            },
        ]
    }
    fn split() -> Schedule {
        Schedule {
            pattern: Pattern::WeekdayWeekend,
            groups: vec![
                DayGroup {
                    group: Group::Weekdays,
                    changes: changes(210),
                },
                DayGroup {
                    group: Group::Weekends,
                    changes: changes(230),
                },
            ],
        }
    }
    #[test]
    fn carries_the_previous_actual_day_across_group_and_week_boundaries() {
        let p = split().preview().unwrap();
        assert_eq!(i16::from(p.days[0].periods[0].target_temperature), 200); // Sunday -> Monday
        assert_eq!(i16::from(p.days[5].periods[0].target_temperature), 180); // Friday -> Saturday
        assert_eq!(i16::from(p.days[6].periods[0].target_temperature), 200); // Saturday -> Sunday
        for day in p.days {
            assert_eq!(day.periods.first().unwrap().start_minute, 0);
            assert_eq!(day.periods.last().unwrap().end_minute, 1440);
            assert!(
                day.periods
                    .windows(2)
                    .all(|w| w[0].end_minute == w[1].start_minute)
            );
            assert_eq!(
                day.periods
                    .iter()
                    .map(|p| p.end_minute - p.start_minute)
                    .sum::<u16>(),
                1440
            );
        }
        assert!(!p.can_save);
    }
    #[test]
    fn supports_all_day_and_sorts_without_mutating_intent() {
        let s = Schedule {
            pattern: Pattern::Daily,
            groups: vec![DayGroup {
                group: Group::EveryDay,
                changes: vec![Change {
                    start_minute: 0,
                    target_temperature: 215.try_into().unwrap(),
                }],
            }],
        };
        let p = s.preview().unwrap();
        assert!(
            p.days
                .iter()
                .all(|d| d.periods.len() == 1 && !d.periods[0].continues_from_previous_day)
        );
        let mut s = split();
        s.groups.reverse();
        s.groups[0].changes.reverse();
        assert_eq!(s.validate().unwrap(), split());
    }
    #[test]
    fn redundant_midnight_is_not_a_change_but_cross_day_changes_are_retained() {
        let mut daily = Schedule {
            pattern: Pattern::Daily,
            groups: vec![DayGroup {
                group: Group::EveryDay,
                changes: vec![
                    Change {
                        start_minute: 0,
                        target_temperature: 185.try_into().unwrap(),
                    },
                    Change {
                        start_minute: 450,
                        target_temperature: 215.try_into().unwrap(),
                    },
                    Change {
                        start_minute: 1020,
                        target_temperature: 185.try_into().unwrap(),
                    },
                ],
            }],
        };
        let preview = daily.clone().preview().unwrap();
        assert_eq!(preview.schedule.groups[0].changes.len(), 2);
        assert_eq!(
            i16::from(preview.days[0].periods[0].target_temperature),
            185
        );
        daily.groups[0].changes = [0, 240, 420, 660, 840, 1080, 1260]
            .into_iter()
            .enumerate()
            .map(|(i, start_minute)| Change {
                start_minute,
                target_temperature: (if i % 2 == 0 { 185 } else { 215 }).try_into().unwrap(),
            })
            .collect();
        assert_eq!(daily.validate().unwrap().groups[0].changes.len(), 6);
        let mut split = split();
        split.groups[0].changes.insert(
            0,
            Change {
                start_minute: 0,
                target_temperature: 180.try_into().unwrap(),
            },
        );
        // Monday follows Sunday (200), even though Tue–Fri follow a weekday (180).
        assert_eq!(
            split.validate().unwrap().groups[0].changes[0].start_minute,
            0
        );
    }
    #[test]
    fn rejects_ambiguous_incomplete_and_unrepresentable_drafts() {
        for minute in [1, 1439, 1440, 2880] {
            let mut s = split();
            s.groups[0].changes[0].start_minute = minute;
            assert!(s.validate().is_err());
        }
        let mut s = split();
        s.groups[0].changes[1].start_minute = 420;
        assert!(s.validate().is_err());
        let mut s = split();
        s.groups[0].changes.clear();
        assert!(s.validate().is_err());
        let mut s = split();
        s.groups[1].group = Group::Weekdays;
        assert!(s.validate().is_err());
        let mut s = split();
        s.groups.pop();
        assert!(s.validate().is_err());
        let mut s = split();
        s.groups[0].changes = vec![s.groups[0].changes[0].clone(); 7];
        assert!(s.validate().is_err());
    }
}
