//! APK-derived mapping only; this is not a live write transport.
//! Future writes must preserve/verify disabled slots against a fresh upstream read.
use crate::schedule::{Group, Pattern, Schedule};

#[derive(Debug, PartialEq, Eq)]
pub struct Programme {
    pub day: u8,
    /// Active [minute of day, temperature in tenths] rows, before slot padding.
    pub active_rows: Vec<[u16; 2]>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ProgrammePlan {
    pub mode: u8,
    pub programmes: Vec<Programme>,
}
/// Keeps manufacturer mode/day numbers and positional arrays out of the browser.
/// No message IDs, authentication, disabled-slot guesses, or network requests.
pub fn map_draft(schedule: Schedule) -> Result<ProgrammePlan, &'static str> {
    let schedule = schedule.validate()?;
    let mode = match schedule.pattern {
        Pattern::Weekly => 0,
        Pattern::WeekdayWeekend => 1,
        Pattern::Daily => 2,
    };
    let programmes = schedule
        .groups
        .into_iter()
        .map(|g| Programme {
            day: match g.group {
                Group::Monday => 1,
                Group::Tuesday => 2,
                Group::Wednesday => 3,
                Group::Thursday => 4,
                Group::Friday => 5,
                Group::Saturday => 6,
                Group::Sunday => 7,
                Group::Weekdays => 8,
                Group::Weekends => 9,
                Group::EveryDay => 10,
            },
            active_rows: g
                .changes
                .into_iter()
                .map(|c| [c.start_minute, i16::from(c.target_temperature) as u16])
                .collect(),
        })
        .collect();
    Ok(ProgrammePlan { mode, programmes })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{Change, DayGroup};
    #[test]
    fn translates_each_pattern_and_keeps_tenths_exact() {
        for (pattern, mode, days) in [
            (Pattern::Daily, 2, vec![10]),
            (Pattern::WeekdayWeekend, 1, vec![8, 9]),
            (Pattern::Weekly, 0, vec![1, 2, 3, 4, 5, 6, 7]),
        ] {
            let plan = map_draft(Schedule {
                pattern,
                groups: pattern
                    .groups()
                    .into_iter()
                    .map(|group| DayGroup {
                        group,
                        changes: vec![
                            Change {
                                start_minute: 420,
                                target_temperature: 215.try_into().unwrap(),
                            },
                            Change {
                                start_minute: 1410,
                                target_temperature: 185.try_into().unwrap(),
                            },
                        ],
                    })
                    .collect(),
            })
            .unwrap();
            assert_eq!(plan.mode, mode);
            assert_eq!(
                plan.programmes.iter().map(|p| p.day).collect::<Vec<_>>(),
                days
            );
            assert!(
                plan.programmes
                    .iter()
                    .all(|p| p.active_rows == vec![[420, 215], [1410, 185]])
            );
        }
    }
}
