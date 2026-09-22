use std::time::Duration;

// Six edges of the saturated RGB color wheel, with 255 unit changes per edge.
pub const COLOR_COUNT: u16 = 6 * 255;

pub const MAX_COLOR_STEP: u16 = 100;
// Retained as the UI minimum average pace, not a per-frame rate limiter.
pub const MIN_AVERAGE_INTERVAL: Duration = Duration::from_millis(300);
pub const MAX_CYCLE_SECONDS: u32 = 3600;

pub fn default_color_step() -> u16 {
    1
}

pub fn default_cycle_seconds() -> u32 {
    600
}

pub fn frames_per_cycle(color_step: u16) -> u16 {
    COLOR_COUNT.div_ceil(color_step)
}

pub fn minimum_cycle_seconds(color_step: u16) -> u32 {
    (u32::from(frames_per_cycle(color_step)) * 300).div_ceil(1000)
}

pub fn average_frame_interval(cycle_seconds: u32, color_step: u16) -> Result<Duration, String> {
    validate_color_step(color_step)?;
    let minimum = minimum_cycle_seconds(color_step);
    if !(minimum..=MAX_CYCLE_SECONDS).contains(&cycle_seconds) {
        return Err(format!(
            "full cycle must be between {minimum} and {MAX_CYCLE_SECONDS} seconds for color step {color_step}"
        ));
    }
    Ok(
        Duration::from_secs_f64(f64::from(cycle_seconds) / f64::from(frames_per_cycle(color_step)))
            .max(MIN_AVERAGE_INTERVAL),
    )
}

pub fn cycle_from_legacy_pace(pace: f32, color_step: u16) -> Result<u32, String> {
    validate_color_step(color_step)?;
    if !pace.is_finite() || !(0.1..=2.0).contains(&pace) {
        return Err("legacy breathing pace must be between 0.1 and 2 seconds".into());
    }
    Ok(
        ((f64::from(pace) * f64::from(frames_per_cycle(color_step))).ceil() as u32)
            .max(minimum_cycle_seconds(color_step)),
    )
}

pub fn resolve_cycle_seconds(
    cycle: Option<u32>,
    pace: Option<f32>,
    color_step: u16,
) -> Result<u32, String> {
    let cycle = match (cycle, pace) {
        (Some(_), Some(_)) => {
            return Err("choose cycle_seconds or legacy pace_seconds, not both".into());
        }
        (Some(cycle), None) => cycle,
        (None, Some(pace)) => cycle_from_legacy_pace(pace, color_step)?,
        (None, None) => default_cycle_seconds(),
    };
    average_frame_interval(cycle, color_step)?;
    Ok(cycle)
}

// Rebase after a slow write instead of accumulating overdue frame deadlines.
pub fn next_frame_deadline(
    started: tokio::time::Instant,
    finished: tokio::time::Instant,
    interval: Duration,
) -> tokio::time::Instant {
    (started + interval).max(finished)
}

// Close every lap exactly, even when the requested step does not divide 1,530.
// Only the last step may be shorter; the starting hue remains random.
pub struct ColorCycle {
    origin: u16,
    travelled: u16,
    color_step: u16,
}

impl ColorCycle {
    pub fn new(origin: u16, color_step: u16) -> Result<Self, String> {
        validate_color_step(color_step)?;
        Ok(Self {
            origin: origin % COLOR_COUNT,
            travelled: 0,
            color_step,
        })
    }

    pub fn advance(&mut self) -> u16 {
        self.travelled = (self.travelled + self.color_step).min(COLOR_COUNT) % COLOR_COUNT;
        (self.origin + self.travelled) % COLOR_COUNT
    }
}

pub fn validate_color_step(step: u16) -> Result<(), String> {
    if !(1..=MAX_COLOR_STEP).contains(&step) {
        return Err(format!(
            "breathing color step must be between 1 and {MAX_COLOR_STEP}"
        ));
    }
    Ok(())
}

pub fn color_step_from_degrees(degrees: f32) -> Result<u16, String> {
    if !degrees.is_finite() || !(0.1..=120.0).contains(&degrees) {
        return Err("breathing hue step must be between 0.1 and 120 degrees".into());
    }
    Ok((degrees * 255.0 / 60.0)
        .round()
        .clamp(1.0, f32::from(MAX_COLOR_STEP)) as u16)
}

pub fn rgb_at_position(position: u16) -> [u8; 3] {
    let position = position % COLOR_COUNT;
    let offset = (position % 255) as u8;
    match position / 255 {
        0 => [255, offset, 0],
        1 => [255 - offset, 255, 0],
        2 => [0, 255, offset],
        3 => [0, 255 - offset, 255],
        4 => [offset, 0, 255],
        _ => [255, 0, 255 - offset],
    }
}

pub fn color_at_position(position: u16) -> String {
    let [red, green, blue] = rgb_at_position(position);
    format!("#{red:02x}{green:02x}{blue:02x}")
}

#[derive(Debug, Clone)]
pub struct PerceptualFrame {
    pub position: u16,
    pub brightness: u8,
    // Time from this frame to the following frame, including the closing edge.
    pub interval: Duration,
}

pub struct PerceptualCycle {
    pub frames: Vec<PerceptualFrame>,
}

impl PerceptualCycle {
    pub fn new(
        origin: u16,
        color_step: u16,
        cycle_seconds: u32,
        base_brightness: f32,
    ) -> Result<Self, String> {
        average_frame_interval(cycle_seconds, color_step)?;
        let base = crate::protocol::brightness_frame(base_brightness)?[2];
        let mut cycle = ColorCycle::new(origin, color_step)?;
        let mut position = origin % COLOR_COUNT;
        let mut frames = Vec::with_capacity(usize::from(frames_per_cycle(color_step)));
        for _ in 0..frames_per_cycle(color_step) {
            frames.push(PerceptualFrame {
                position,
                brightness: crate::perceptual::boosted_brightness(rgb_at_position(position), base),
                interval: Duration::ZERO,
            });
            position = cycle.advance();
        }
        let labs: Vec<_> = frames
            .iter()
            .map(|frame| {
                // An all-black cycle has no perceptual distance. Use the full-output
                // hue path for timing when brightness is zero, while still sending zero.
                let brightness = if base == 0 {
                    1.0
                } else {
                    f64::from(frame.brightness) / 255.0
                };
                crate::perceptual::oklab(rgb_at_position(frame.position), brightness)
            })
            .collect();
        let distances: Vec<_> = (0..frames.len())
            .map(|index| crate::perceptual::distance(labs[index], labs[(index + 1) % labs.len()]))
            .collect();
        let total: f64 = distances.iter().sum();
        for (frame, distance) in frames.iter_mut().zip(distances) {
            frame.interval = Duration::from_secs_f64(f64::from(cycle_seconds) * distance / total);
        }
        Ok(Self { frames })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_hex_color;
    use std::collections::HashSet;

    #[test]
    fn weighted_cycles_preserve_duration_order_and_brightness_limits() {
        for step in 1..=MAX_COLOR_STEP {
            for base in [0.0, 0.01, 0.1, 0.4, 1.0] {
                let seconds = minimum_cycle_seconds(step);
                let plan = PerceptualCycle::new(1529, step, seconds, base).unwrap();
                assert_eq!(plan.frames.len(), usize::from(frames_per_cycle(step)));
                assert_eq!(plan.frames[0].position, 1529);
                let total: f64 = plan
                    .frames
                    .iter()
                    .map(|frame| frame.interval.as_secs_f64())
                    .sum();
                assert!((total - f64::from(seconds)).abs() < 0.00001);
                let mut expected = ColorCycle::new(1529, step).unwrap();
                for (index, frame) in plan.frames.iter().enumerate() {
                    if index > 0 {
                        assert_eq!(frame.position, expected.advance());
                    }
                    assert!(frame.interval > Duration::ZERO);
                    assert!(
                        frame.brightness >= crate::protocol::brightness_frame(base).unwrap()[2]
                    );
                    if base == 0.0 {
                        assert_eq!(frame.brightness, 0);
                    }
                    if base == 1.0 {
                        assert_eq!(frame.brightness, 255);
                    }
                }
            }
        }
        let plan = PerceptualCycle::new(0, 1, 459, 1.0).unwrap();
        assert!(
            plan.frames
                .iter()
                .any(|frame| frame.interval < Duration::from_millis(20))
        );
        assert!(
            plan.frames
                .iter()
                .any(|frame| frame.interval > Duration::from_millis(400))
        );
        assert!(PerceptualCycle::new(0, 1, 458, 1.0).is_err());
        assert!(PerceptualCycle::new(0, 1, 600, f32::NAN).is_err());
    }

    #[test]
    fn every_step_completes_one_exact_lap_with_the_expected_number_of_updates() {
        for step in 1..=MAX_COLOR_STEP {
            for origin in [0, 254, 765, 1529] {
                let mut cycle = ColorCycle::new(origin, step).unwrap();
                let count = frames_per_cycle(step);
                let mut previous = origin;
                let mut travelled = 0;
                for index in 1..=count {
                    let position = cycle.advance();
                    let distance = (position + COLOR_COUNT - previous) % COLOR_COUNT;
                    assert_eq!(
                        distance,
                        if index == count {
                            COLOR_COUNT - travelled
                        } else {
                            step
                        }
                    );
                    travelled += distance;
                    previous = position;
                }
                assert_eq!(travelled, COLOR_COUNT);
                assert_eq!(previous, origin);
                assert_eq!(cycle.advance(), (origin + step) % COLOR_COUNT);
            }
            let minimum = minimum_cycle_seconds(step);
            assert!(average_frame_interval(minimum - 1, step).is_err());
            for duration in [minimum, 600, MAX_CYCLE_SECONDS] {
                let interval = average_frame_interval(duration, step).unwrap();
                assert!(interval >= MIN_AVERAGE_INTERVAL);
                assert!(
                    (interval.as_secs_f64() * f64::from(frames_per_cycle(step))
                        - f64::from(duration))
                    .abs()
                        < 0.00001
                );
            }
        }
        assert_eq!(minimum_cycle_seconds(1), 459);
        assert_eq!(minimum_cycle_seconds(100), 5);
        assert!(average_frame_interval(600, 0).is_err());
        assert!(average_frame_interval(600, 101).is_err());
    }

    #[test]
    fn scheduler_accounts_for_writes_and_never_catches_up_in_a_burst() {
        let start = tokio::time::Instant::now();
        let interval = Duration::from_secs(1);
        assert_eq!(
            next_frame_deadline(start, start + Duration::from_millis(50), interval),
            start + interval
        );
        assert_eq!(
            next_frame_deadline(start, start + Duration::from_secs(3), interval),
            start + Duration::from_secs(3)
        );
        assert_eq!(
            next_frame_deadline(
                start,
                start + Duration::from_millis(50),
                MIN_AVERAGE_INTERVAL
            ),
            start + MIN_AVERAGE_INTERVAL
        );
    }

    #[test]
    fn legacy_and_new_commands_preserve_cycle_duration_bounds() {
        assert_eq!(resolve_cycle_seconds(None, None, 1).unwrap(), 600);
        assert_eq!(resolve_cycle_seconds(None, Some(0.1), 1).unwrap(), 459);
        assert_eq!(resolve_cycle_seconds(None, Some(0.1), 100).unwrap(), 5);
        assert!(resolve_cycle_seconds(Some(458), None, 1).is_err());
        assert!(resolve_cycle_seconds(Some(600), Some(0.75), 1).is_err());
        assert!(resolve_cycle_seconds(None, Some(f32::NAN), 1).is_err());
        assert!(resolve_cycle_seconds(Some(3601), None, 1).is_err());
        for json in [
            r#"{"command":"breathe","cycle_seconds":600,"color_step":1}"#,
            r#"{"command":"breathe","pace_seconds":0.1,"color_step":1}"#,
            r#"{"command":"breathe"}"#,
        ] {
            let command: crate::command::ControlCommand = serde_json::from_str(json).unwrap();
            let crate::command::ControlCommand::Breathe {
                cycle_seconds,
                pace_seconds,
                color_step,
                ..
            } = command
            else {
                panic!("expected breathe")
            };
            assert!(resolve_cycle_seconds(cycle_seconds, pace_seconds, color_step).is_ok());
        }
    }

    #[test]
    fn minimum_step_changes_exactly_one_channel_by_one_including_corners_and_wrap() {
        let mut colors = HashSet::new();
        for position in 0..COLOR_COUNT {
            let current = color_at_position(position);
            assert!(colors.insert(current.clone()));
            let current = parse_hex_color(&current).unwrap();
            let next = parse_hex_color(&color_at_position(position + 1)).unwrap();
            let change: u16 = current
                .iter()
                .zip(next)
                .map(|(a, b)| u16::from(a.abs_diff(b)))
                .sum();
            assert_eq!(change, 1, "position {position}");
        }
        assert_eq!(colors.len(), usize::from(COLOR_COUNT));
        assert_eq!(color_at_position(COLOR_COUNT), color_at_position(0));
    }

    #[test]
    fn preserves_the_existing_color_order() {
        for (position, expected) in [
            (0, "#ff0000"),
            (255, "#ffff00"),
            (510, "#00ff00"),
            (765, "#00ffff"),
            (1020, "#0000ff"),
            (1275, "#ff00ff"),
        ] {
            assert_eq!(color_at_position(position), expected);
        }
    }

    #[test]
    fn every_supported_step_produces_a_different_color_at_every_position() {
        for position in 0..COLOR_COUNT {
            for step in 1..=MAX_COLOR_STEP {
                assert_ne!(
                    color_at_position(position),
                    color_at_position(position + step)
                );
            }
        }
    }

    #[test]
    fn legacy_steps_round_to_the_nearest_representable_change() {
        for (degrees, expected) in [(0.1, 1), (2.0, 9), (6.0, 26), (18.0, 77), (120.0, 100)] {
            assert_eq!(color_step_from_degrees(degrees).unwrap(), expected);
        }
        for invalid in [0.0, 0.05, 121.0, f32::NAN, f32::INFINITY] {
            assert!(color_step_from_degrees(invalid).is_err());
        }
        for step in [1, MAX_COLOR_STEP] {
            assert!(validate_color_step(step).is_ok());
        }
        for step in [0, MAX_COLOR_STEP + 1, u16::MAX] {
            assert!(validate_color_step(step).is_err());
        }
    }
}
