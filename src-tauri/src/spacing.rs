// Measures the closest spacing at which a light keeps streamed frames as
// reliably as it ever does. Lights silently drop frames that arrive too quickly,
// so each trial streams frames at a fixed gap and then reads the light's state
// back to check that the final color and brightness both landed. Same-mode
// streams (breathing) and streams that switch between color and white tolerate
// different gaps. Even well-spaced frames are occasionally lost (under 1% on an
// H6005), so the goal is the gap where drops fall to that background rate.
use std::{future::Future, pin::Pin, time::Duration};

use btleplug::{
    api::{Characteristic, Peripheral as _, ValueNotification, WriteType},
    platform::Peripheral,
};
use futures::{FutureExt as _, Stream, StreamExt};
use uuid::Uuid;

use crate::{
    breathing::color_at_position,
    protocol::{
        DeviceProfile, NOTIFY_CHARACTERISTIC, brightness_frame, color_frame, parse_hex_color,
        status_query_frame, white_frame,
    },
};

pub const SEARCH_CEILING_MS: u64 = 150;
const SCAN_TRIALS: u32 = 10;
const WINDOW_GAPS: usize = 10;
const ALLOWED_DROPS: u32 = 2;
const TRIAL_GROUPS: u16 = 4;
const SETTLE: Duration = Duration::from_millis(300);
const QUERY_SPACING: Duration = Duration::from_millis(100);
const REPLY_TIMEOUT: Duration = Duration::from_millis(800);
const QUERY_ATTEMPTS: u32 = 4;

pub trait SpacingTrial {
    // Streams one trial's frames this many milliseconds apart; true if none were lost.
    fn trial(&mut self, gap_ms: u64) -> impl Future<Output = Result<bool, String>> + Send;
}

// Every gap from 1 ms upward gets SCAN_TRIALS trials (fewer once it has
// already dropped too many to count). The answer is the first gap of the first
// WINDOW_GAPS consecutive gaps with at most ALLOWED_DROPS drops between them:
// 100 trials whose drop rate matches the background rather than crowding.
pub async fn find_minimum_spacing(trial: &mut impl SpacingTrial) -> Result<u64, String> {
    let mut drops = Vec::new();
    for gap in 1..=SEARCH_CEILING_MS {
        let mut dropped = 0;
        for _ in 0..SCAN_TRIALS {
            if !trial.trial(gap).await? {
                dropped += 1;
                if dropped > ALLOWED_DROPS {
                    break;
                }
            }
        }
        drops.push(dropped);
        if drops.len() >= WINDOW_GAPS
            && drops[drops.len() - WINDOW_GAPS..].iter().sum::<u32>() <= ALLOWED_DROPS
        {
            return Ok(gap + 1 - WINDOW_GAPS as u64);
        }
    }
    Err(format!(
        "the light kept dropping frames even {SEARCH_CEILING_MS} ms apart"
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialKind {
    // Color and brightness frames in color mode, as breathing sends them.
    SameMode,
    // Alternating color and white frames, each followed by brightness.
    ModeSwitch,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TrialFrames {
    pub frames: Vec<[u8; 20]>,
    pub brightness: u8,
    pub rgb: [u8; 3],
}

// Every color and brightness in a trial is distinct, and consecutive trials end
// on different values, so a dropped final frame always shows up as a mismatch.
// Values stay dim red-orange (about 3–10%) so the test is gentle at night.
pub fn trial_frames(profile: DeviceProfile, kind: TrialKind, sequence: u32) -> TrialFrames {
    let offset = u16::from(!sequence.is_multiple_of(2));
    let level = |index: u16| (8 + 2 * index + offset) as u8;
    let rgb = |index: u16| {
        parse_hex_color(&color_at_position(20 + 8 * index + 4 * offset))
            .expect("breathing colors are valid hex")
    };
    let mut frames = Vec::new();
    for group in 0..TRIAL_GROUPS {
        frames.push(color_frame(profile, rgb(group)));
        frames.push(brightness_frame(f32::from(level(group * 2)) / 255.0).unwrap());
        frames.push(match kind {
            TrialKind::SameMode => color_frame(profile, rgb(group + TRIAL_GROUPS + 1)),
            TrialKind::ModeSwitch => {
                white_frame(profile, [0xff, 0x8d, 0x0b], Some(2_000 + group * 10))
            }
        });
        frames.push(brightness_frame(f32::from(level(group * 2 + 1)) / 255.0).unwrap());
    }
    let last = TRIAL_GROUPS * 2;
    frames.push(color_frame(profile, rgb(TRIAL_GROUPS)));
    frames.push(brightness_frame(f32::from(level(last)) / 255.0).unwrap());
    TrialFrames {
        frames,
        brightness: level(last),
        rgb: rgb(TRIAL_GROUPS),
    }
}

pub struct SpacingProbe<'a> {
    peripheral: &'a Peripheral,
    control: Characteristic,
    notify: Characteristic,
    notifications: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
    profile: DeviceProfile,
    kind: TrialKind,
    sequence: u32,
}

impl<'a> SpacingProbe<'a> {
    pub async fn start(
        peripheral: &'a Peripheral,
        control: Characteristic,
        profile: DeviceProfile,
    ) -> Result<Self, String> {
        let notify_uuid = Uuid::parse_str(NOTIFY_CHARACTERISTIC)
            .map_err(|error| format!("invalid notify UUID: {error}"))?;
        let notify = peripheral
            .characteristics()
            .into_iter()
            .find(|characteristic| characteristic.uuid == notify_uuid)
            .ok_or("the light does not expose the Govee status characteristic")?;
        peripheral
            .subscribe(&notify)
            .await
            .map_err(|error| error.to_string())?;
        let notifications = peripheral
            .notifications()
            .await
            .map_err(|error| error.to_string())?;
        Ok(Self {
            peripheral,
            control,
            notify,
            notifications,
            profile,
            kind: TrialKind::SameMode,
            sequence: 0,
        })
    }

    pub fn set_kind(&mut self, kind: TrialKind) {
        self.kind = kind;
    }

    pub async fn finish(self) {
        let _ = self.peripheral.unsubscribe(&self.notify).await;
    }

    async fn run_trial(&mut self, gap_ms: u64) -> Result<bool, String> {
        if self.sequence == 0 {
            // Start from color mode so the first same-mode trial does not begin
            // with a mode switch.
            let color = trial_frames(self.profile, TrialKind::SameMode, 0).frames[0];
            write_frame(self.peripheral, &self.control, &color).await?;
            tokio::time::sleep(SETTLE).await;
        }
        self.sequence += 1;
        let expected = trial_frames(self.profile, self.kind, self.sequence);
        let gap = Duration::from_millis(gap_ms);
        let started = tokio::time::Instant::now();
        for (index, frame) in expected.frames.iter().enumerate() {
            tokio::time::sleep_until(started + gap * index as u32).await;
            write_frame(self.peripheral, &self.control, frame).await?;
        }
        tokio::time::sleep(SETTLE).await;
        let (brightness, rgb) = self.read_state().await?;
        Ok(brightness == expected.brightness && rgb == expected.rgb)
    }

    pub fn trials_run(&self) -> u32 {
        self.sequence
    }

    // Brightness (0–255) and the RGB value the light reports. The first query
    // after subscribing sometimes goes unanswered, so this retries.
    pub async fn read_state(&mut self) -> Result<(u8, [u8; 3]), String> {
        let mut brightness = None;
        let mut rgb = None;
        for _ in 0..QUERY_ATTEMPTS {
            while let Some(Some(_)) = self.notifications.next().now_or_never() {}
            if brightness.is_none() {
                write_frame(self.peripheral, &self.control, &status_query_frame(0x04)).await?;
                tokio::time::sleep(QUERY_SPACING).await;
            }
            if rgb.is_none() {
                write_frame(self.peripheral, &self.control, &status_query_frame(0x05)).await?;
            }
            let _ = tokio::time::timeout(REPLY_TIMEOUT, async {
                while let Some(notification) = self.notifications.next().await {
                    match notification.value.as_slice() {
                        [0xaa, 0x04, level, ..] => brightness = Some(*level),
                        [0xaa, 0x05, _, red, green, blue, ..] => rgb = Some([*red, *green, *blue]),
                        _ => {}
                    }
                    if brightness.is_some() && rgb.is_some() {
                        break;
                    }
                }
            })
            .await;
            if let (Some(brightness), Some(rgb)) = (brightness, rgb) {
                return Ok((brightness, rgb));
            }
        }
        Err("the light did not report its brightness and color".into())
    }
}

// A free function so trial futures only borrow Sync fields and stay Send.
async fn write_frame(
    peripheral: &Peripheral,
    control: &Characteristic,
    frame: &[u8; 20],
) -> Result<(), String> {
    peripheral
        .write(control, frame, WriteType::WithoutResponse)
        .await
        .map_err(|error| error.to_string())
}

impl SpacingTrial for SpacingProbe<'_> {
    fn trial(&mut self, gap_ms: u64) -> impl Future<Output = Result<bool, String>> + Send {
        self.run_trial(gap_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Below the threshold a third of trials drop a frame; between flaky_below and
    // the threshold one in twelve does; above it, one in 150 (the background rate).
    struct Simulated {
        threshold: u64,
        flaky_below: u64,
        calls: u32,
    }

    impl SpacingTrial for Simulated {
        fn trial(&mut self, gap: u64) -> impl Future<Output = Result<bool, String>> + Send {
            self.calls += 1;
            let period = if gap >= self.threshold {
                150
            } else if gap >= self.flaky_below {
                12
            } else {
                3
            };
            std::future::ready(Ok(!self.calls.is_multiple_of(period)))
        }
    }

    fn simulate(threshold: u64, flaky_below: u64) -> Simulated {
        Simulated {
            threshold,
            flaky_below,
            calls: 0,
        }
    }

    #[tokio::test]
    async fn search_finds_where_drops_fall_to_the_background_rate() {
        for threshold in [
            1,
            2,
            14,
            26,
            47,
            99,
            SEARCH_CEILING_MS - WINDOW_GAPS as u64 + 1,
        ] {
            assert_eq!(
                find_minimum_spacing(&mut simulate(threshold, threshold)).await,
                Ok(threshold)
            );
        }
    }

    #[tokio::test]
    async fn occasional_drops_below_the_threshold_still_count() {
        for (threshold, flaky_below) in [(26, 13), (47, 14)] {
            let found = find_minimum_spacing(&mut simulate(threshold, flaky_below))
                .await
                .unwrap();
            assert!(
                (threshold - 2..=threshold).contains(&found),
                "{found} for {threshold}"
            );
        }
    }

    #[tokio::test]
    async fn a_light_that_never_settles_is_reported() {
        assert!(
            find_minimum_spacing(&mut simulate(SEARCH_CEILING_MS + 1, SEARCH_CEILING_MS + 1))
                .await
                .is_err()
        );
    }

    #[test]
    fn trial_values_are_distinct_and_alternate_between_trials() {
        for (profile, kind) in [DeviceProfile::Classic, DeviceProfile::H6005]
            .into_iter()
            .flat_map(|profile| {
                [
                    (profile, TrialKind::SameMode),
                    (profile, TrialKind::ModeSwitch),
                ]
            })
        {
            for sequence in 1..4 {
                let trial = trial_frames(profile, kind, sequence);
                let next = trial_frames(profile, kind, sequence + 1);
                assert_ne!(trial.brightness, next.brightness);
                assert_ne!(trial.rgb, next.rgb);
                let brightness: HashSet<_> = trial
                    .frames
                    .iter()
                    .filter(|frame| frame[1] == 0x04)
                    .map(|frame| frame[2])
                    .collect();
                assert_eq!(brightness.len(), usize::from(TRIAL_GROUPS * 2 + 1));
                assert_eq!(
                    trial.frames.last().unwrap(),
                    &brightness_frame(f32::from(trial.brightness) / 255.0).unwrap()
                );
                assert_eq!(
                    trial.frames[trial.frames.len() - 2],
                    color_frame(profile, trial.rgb)
                );
                let white =
                    trial
                        .frames
                        .contains(&white_frame(profile, [0xff, 0x8d, 0x0b], Some(2_000)));
                assert_eq!(white, kind == TrialKind::ModeSwitch);
            }
        }
    }
}
