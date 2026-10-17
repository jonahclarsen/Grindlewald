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
    /// `used_since` reports whether someone used the Mac after the given time.
    /// It is only asked while an automation waits in its run-on-use window.
    pub(crate) fn due<T: TimeZone>(
        &mut self,
        now: DateTime<T>,
        schedules: Vec<Schedule>,
        mut used_since: impl FnMut(DateTime<T>) -> bool,
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
            if let Some(start) = self.use_window_start(&now, &schedule, time)
                && used_since(start)
            {
                self.triggered
                    .insert((now.date_naive(), schedule.id.clone()));
                due.push((now.clone(), schedule.clone()));
            }
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

    /// The start of today's run-on-use window when `now` is inside it and the
    /// automation has not already run today. The scheduled time ends the window.
    fn use_window_start<T: TimeZone>(
        &self,
        now: &DateTime<T>,
        schedule: &Schedule,
        time: NaiveTime,
    ) -> Option<DateTime<T>> {
        if !schedule.run_on_use {
            return None;
        }
        let after = NaiveTime::parse_from_str(&schedule.run_on_use_after, "%H:%M").ok()?;
        let date = now.date_naive();
        if after >= time || self.triggered.contains(&(date, schedule.id.clone())) {
            return None;
        }
        let local = |time| {
            now.timezone()
                .from_local_datetime(&date.and_time(time))
                .earliest()
        };
        let (start, end) = (local(after)?, local(time)?);
        (start <= *now && *now < end).then_some(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn at(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).unwrap()
    }

    fn unused(_: DateTime<FixedOffset>) -> bool {
        false
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
                    .due(
                        at("2026-10-07T19:49:00-07:00"),
                        vec![schedule("20:00")],
                        unused
                    )
                    .is_empty()
            );
            let due = scheduler.due(at(wake), vec![schedule("20:00")], unused);
            assert_eq!(due.len(), expected);
            if expected == 1 {
                assert_eq!(due[0].0, at("2026-10-07T20:00:00-07:00"));
            }
            assert!(
                scheduler
                    .due(at(wake), vec![schedule("20:00")], unused)
                    .is_empty()
            );
        }
    }

    #[test]
    fn startup_only_runs_current_minute_and_normal_ticks_run_once() {
        let mut scheduler = Scheduler::default();
        let schedules = vec![schedule("19:00"), schedule("20:00"), schedule("20:01")];
        let due = scheduler.due(at("2026-10-07T20:00:35-07:00"), schedules.clone(), unused);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.time, "20:00");
        assert!(
            scheduler
                .due(at("2026-10-07T20:00:45-07:00"), schedules.clone(), unused)
                .is_empty()
        );
        let due = scheduler.due(at("2026-10-07T20:01:05-07:00"), schedules, unused);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.time, "20:01");
    }

    #[test]
    fn midnight_preserves_deduplication_even_after_clock_moves_back() {
        let mut scheduler = Scheduler::default();
        let schedules = vec![schedule("23:00"), schedule("00:15")];
        assert_eq!(
            scheduler
                .due(at("2026-10-07T23:00:00-07:00"), schedules.clone(), unused)
                .len(),
            1
        );
        let due = scheduler.due(at("2026-10-08T00:30:00-07:00"), schedules.clone(), unused);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.time, "00:15");
        assert!(
            scheduler
                .due(at("2026-10-07T22:59:00-07:00"), schedules.clone(), unused)
                .is_empty()
        );
        assert!(
            scheduler
                .due(at("2026-10-08T00:30:00-07:00"), schedules, unused)
                .is_empty()
        );
    }

    #[test]
    fn disabled_and_temporarily_paused_occurrences_are_not_replayed() {
        let mut scheduler = Scheduler::default();
        scheduler.due(at("2026-10-07T19:00:00-07:00"), vec![], unused);
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
                    vec![disabled, paused, still_paused],
                    unused
                )
                .is_empty()
        );
    }

    #[test]
    fn long_sleep_only_runs_recent_occurrences_in_time_order() {
        let mut scheduler = Scheduler::default();
        scheduler.due(at("2026-10-05T19:00:00-07:00"), vec![], unused);
        let due = scheduler.due(
            at("2026-10-08T03:00:00-07:00"),
            vec![schedule("01:00"), schedule("18:00"), schedule("20:00")],
            unused,
        );
        assert_eq!(
            due.iter().map(|(_, s)| s.time.as_str()).collect::<Vec<_>>(),
            ["20:00", "01:00"]
        );
    }

    fn on_use(time: &str, after: &str) -> Schedule {
        let mut schedule = schedule(time);
        schedule.run_on_use = true;
        schedule.run_on_use_after = after.into();
        schedule
    }

    #[test]
    fn use_inside_window_runs_once_instead_of_the_scheduled_time() {
        let mut scheduler = Scheduler::default();
        let schedules = vec![on_use("08:00", "06:00")];
        let asked = std::cell::RefCell::new(Vec::new());
        let mut used = |start| {
            asked.borrow_mut().push(start);
            true
        };
        assert!(
            scheduler
                .due(
                    at("2026-10-07T05:59:50-07:00"),
                    schedules.clone(),
                    &mut used
                )
                .is_empty()
        );
        let due = scheduler.due(
            at("2026-10-07T06:30:00-07:00"),
            schedules.clone(),
            &mut used,
        );
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, at("2026-10-07T06:30:00-07:00"));
        assert!(
            scheduler
                .due(
                    at("2026-10-07T06:30:10-07:00"),
                    schedules.clone(),
                    &mut used
                )
                .is_empty()
        );
        assert!(
            scheduler
                .due(
                    at("2026-10-07T08:00:00-07:00"),
                    schedules.clone(),
                    &mut used
                )
                .is_empty()
        );
        // Only the one tick inside the window asked whether the Mac was used.
        assert_eq!(*asked.borrow(), [at("2026-10-07T06:00:00-07:00")]);
        let due = scheduler.due(at("2026-10-08T06:00:00-07:00"), schedules, &mut used);
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn without_use_the_scheduled_time_still_runs() {
        let mut scheduler = Scheduler::default();
        let schedules = vec![on_use("08:00", "06:00")];
        assert!(
            scheduler
                .due(at("2026-10-07T06:00:00-07:00"), schedules.clone(), unused)
                .is_empty()
        );
        assert!(
            scheduler
                .due(at("2026-10-07T07:59:50-07:00"), schedules.clone(), unused)
                .is_empty()
        );
        let due = scheduler.due(at("2026-10-07T08:00:00-07:00"), schedules, |_| {
            panic!("window over")
        });
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, at("2026-10-07T08:00:00-07:00"));
    }

    #[test]
    fn use_is_ignored_when_off_paused_or_not_earlier_than_the_scheduled_time() {
        let mut off = on_use("08:00", "06:00");
        off.run_on_use = false;
        let mut paused = on_use("08:00", "06:00");
        paused.disabled_until = Some(at("2026-10-07T09:00:00-07:00").timestamp_millis());
        let schedules = vec![
            off,
            paused,
            on_use("08:00", "08:00"),
            on_use("08:00", "09:00"),
            on_use("08:00", ""),
        ];
        let mut scheduler = Scheduler::default();
        assert!(
            scheduler
                .due(at("2026-10-07T07:00:00-07:00"), schedules, |_| panic!(
                    "not in a window"
                ))
                .is_empty()
        );
    }
}
