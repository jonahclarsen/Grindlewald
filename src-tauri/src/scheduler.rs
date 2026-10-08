use std::collections::HashSet;

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Timelike};

use crate::settings::Schedule;

/// Track wall-clock intervals so sleep does not discard scheduled occurrences.
/// Startup only checks the current minute; it does not replay earlier commands.
#[derive(Default)]
pub(crate) struct Scheduler {
    previous: Option<DateTime<chrono::Utc>>,
    triggered: HashSet<(chrono::NaiveDate, String)>,
}

impl Scheduler {
    pub(crate) fn due<T: TimeZone>(
        &mut self,
        now: DateTime<T>,
        schedules: Vec<Schedule>,
    ) -> Vec<(DateTime<T>, Schedule)> {
        let now_utc = now.to_utc();
        let previous = self.previous.unwrap_or_else(|| {
            now_utc.with_second(0).unwrap().with_nanosecond(0).unwrap() - Duration::nanoseconds(1)
        });
        self.previous = Some(now_utc);
        let cutoff = now_utc - Duration::hours(8);
        let first_date = cutoff.with_timezone(&now.timezone()).date_naive();
        self.triggered.retain(|(date, _)| *date >= first_date);
        let mut due = Vec::new();

        for schedule in schedules {
            if !schedule.is_enabled_at(now.timestamp_millis()) {
                continue;
            }
            let Ok(time) = NaiveTime::parse_from_str(&schedule.time, "%H:%M") else {
                continue;
            };
            let mut date = first_date;
            while date <= now.date_naive() {
                // Skip nonexistent DST times; repeated times count once per local date.
                if let Some(occurrence) = now
                    .timezone()
                    .from_local_datetime(&date.and_time(time))
                    .earliest()
                {
                    if occurrence.to_utc() > previous
                        && occurrence.to_utc() >= cutoff
                        && occurrence <= now
                        && schedule.is_enabled_at(occurrence.timestamp_millis())
                        && self.triggered.insert((date, schedule.id.clone()))
                    {
                        due.push((occurrence, schedule.clone()));
                    }
                }
                let Some(next) = date.succ_opt() else { break };
                date = next;
            }
        }
        due.sort_by(|a, b| a.0.cmp(&b.0));
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn at(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).unwrap()
    }

    fn schedule(time: &str) -> Schedule {
        serde_json::from_value(serde_json::json!({
            "id": time.replace(':', "-"), "name": "Test", "time": time
        }))
        .unwrap()
    }

    #[test]
    fn wake_catches_up_once_and_respects_eight_hour_boundary() {
        for (wake, expected) in [
            ("2026-10-07T20:30:00-07:00", 1),
            ("2026-10-08T04:00:00-07:00", 1),
            ("2026-10-08T04:00:01-07:00", 0),
        ] {
            let mut scheduler = Scheduler::default();
            assert!(
                scheduler
                    .due(at("2026-10-07T19:49:00-07:00"), vec![schedule("20:00")])
                    .is_empty()
            );
            let due = scheduler.due(at(wake), vec![schedule("20:00")]);
            assert_eq!(due.len(), expected);
            if expected == 1 {
                assert_eq!(due[0].0, at("2026-10-07T20:00:00-07:00"));
            }
            assert!(scheduler.due(at(wake), vec![schedule("20:00")]).is_empty());
        }
    }

    #[test]
    fn startup_only_runs_current_minute_and_normal_ticks_run_once() {
        let mut scheduler = Scheduler::default();
        let schedules = vec![schedule("19:00"), schedule("20:00"), schedule("20:01")];
        let due = scheduler.due(at("2026-10-07T20:00:35-07:00"), schedules.clone());
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.time, "20:00");
        assert!(
            scheduler
                .due(at("2026-10-07T20:00:45-07:00"), schedules.clone())
                .is_empty()
        );
        let due = scheduler.due(at("2026-10-07T20:01:05-07:00"), schedules);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.time, "20:01");
    }

    #[test]
    fn midnight_preserves_deduplication_even_after_clock_moves_back() {
        let mut scheduler = Scheduler::default();
        let schedules = vec![schedule("23:00"), schedule("00:15")];
        assert_eq!(
            scheduler
                .due(at("2026-10-07T23:00:00-07:00"), schedules.clone())
                .len(),
            1
        );
        let due = scheduler.due(at("2026-10-08T00:30:00-07:00"), schedules.clone());
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.time, "00:15");
        assert!(
            scheduler
                .due(at("2026-10-07T22:59:00-07:00"), schedules.clone())
                .is_empty()
        );
        assert!(
            scheduler
                .due(at("2026-10-08T00:30:00-07:00"), schedules)
                .is_empty()
        );
    }

    #[test]
    fn disabled_and_temporarily_paused_occurrences_are_not_replayed() {
        let mut scheduler = Scheduler::default();
        scheduler.due(at("2026-10-07T19:00:00-07:00"), vec![]);
        let mut disabled = schedule("20:00");
        disabled.enabled = false;
        let mut paused = schedule("21:00");
        paused.disabled_until = Some(at("2026-10-07T21:30:00-07:00").timestamp_millis());
        let mut still_paused = schedule("22:00");
        still_paused.disabled_until = Some(at("2026-10-08T01:00:00-07:00").timestamp_millis());
        assert!(
            scheduler
                .due(
                    at("2026-10-07T23:00:00-07:00"),
                    vec![disabled, paused, still_paused]
                )
                .is_empty()
        );
    }

    #[test]
    fn long_sleep_only_runs_recent_occurrences_in_time_order() {
        let mut scheduler = Scheduler::default();
        scheduler.due(at("2026-10-05T19:00:00-07:00"), vec![]);
        let due = scheduler.due(
            at("2026-10-08T03:00:00-07:00"),
            vec![schedule("01:00"), schedule("18:00"), schedule("20:00")],
        );
        assert_eq!(
            due.iter().map(|(_, s)| s.time.as_str()).collect::<Vec<_>>(),
            ["20:00", "01:00"]
        );
    }
}
