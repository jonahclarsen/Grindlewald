use std::time::Duration;

// Six edges of the saturated RGB color wheel, with 255 unit changes per edge.
pub const COLOR_COUNT: u16 = 6 * 255;
pub const MAX_COLOR_STEP: u16 = 100;

pub fn default_color_step() -> u16 {
    1
}
pub fn default_interval_ms() -> u32 {
    350
}

pub fn frame_interval(interval_ms: u32) -> Result<Duration, String> {
    if !(250..=1000).contains(&interval_ms) || interval_ms % 50 != 0 {
        return Err("breathing interval must be 250–1000 milliseconds in increments of 50".into());
    }
    Ok(Duration::from_millis(u64::from(interval_ms)))
}

// Used only when migrating old settings or whole-cycle commands.
pub fn rounded_interval_ms(milliseconds: f64) -> Result<u32, String> {
    if !milliseconds.is_finite() || milliseconds <= 0.0 {
        return Err("invalid legacy breathing interval".into());
    }
    Ok(((milliseconds / 50.0).round() * 50.0).clamp(250.0, 1000.0) as u32)
}

pub fn resolve_interval_ms(
    interval_ms: Option<u32>,
    pace_seconds: Option<f32>,
    cycle_seconds: Option<u32>,
    color_step: u16,
) -> Result<u32, String> {
    validate_color_step(color_step)?;
    if usize::from(interval_ms.is_some())
        + usize::from(pace_seconds.is_some())
        + usize::from(cycle_seconds.is_some())
        > 1
    {
        return Err("choose only one breathing interval format".into());
    }
    let interval = if let Some(interval) = interval_ms {
        interval
    } else if let Some(pace) = pace_seconds {
        let milliseconds = f64::from(pace) * 1000.0;
        if !milliseconds.is_finite()
            || (milliseconds - milliseconds.round()).abs() > 0.001
            || !(250.0..=1000.0).contains(&milliseconds.round())
        {
            return Err("breathing pace must be 0.25–1 seconds in increments of 0.05".into());
        }
        milliseconds.round() as u32
    } else if let Some(cycle) = cycle_seconds {
        rounded_interval_ms(
            f64::from(cycle) * 1000.0 / f64::from(COLOR_COUNT.div_ceil(color_step)),
        )?
    } else {
        default_interval_ms()
    };
    frame_interval(interval)?;
    Ok(interval)
}

// Rebase after a slow write instead of accumulating overdue frame deadlines.
pub fn next_frame_deadline(
    started: tokio::time::Instant,
    finished: tokio::time::Instant,
    interval: Duration,
) -> tokio::time::Instant {
    (started + interval).max(finished)
}

pub struct ColorCycle {
    pub position: u16,
    color_step: u16,
}

impl ColorCycle {
    pub fn new(origin: u16, color_step: u16) -> Result<Self, String> {
        validate_color_step(color_step)?;
        Ok(Self {
            position: origin % COLOR_COUNT,
            color_step,
        })
    }

    pub fn advance(&mut self) -> u16 {
        self.position = (self.position + self.color_step) % COLOR_COUNT;
        self.position
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_hex_color;
    use std::collections::HashSet;

    #[test]
    fn every_update_advances_the_selected_step_even_across_wrap() {
        for step in 1..=MAX_COLOR_STEP {
            let mut cycle = ColorCycle::new(1529, step).unwrap();
            for _ in 0..COLOR_COUNT {
                let previous = cycle.position;
                let position = cycle.advance();
                assert_eq!((position + COLOR_COUNT - previous) % COLOR_COUNT, step);
            }
        }
    }

    #[test]
    fn interval_bounds_and_increments_are_enforced_for_all_commands() {
        for ms in 0..=1100 {
            let valid = (250..=1000).contains(&ms) && ms % 50 == 0;
            assert_eq!(frame_interval(ms).is_ok(), valid);
            assert_eq!(resolve_interval_ms(Some(ms), None, None, 1).is_ok(), valid);
            assert_eq!(
                resolve_interval_ms(None, Some(ms as f32 / 1000.0), None, 1).is_ok(),
                valid
            );
        }
        assert_eq!(resolve_interval_ms(None, None, None, 1).unwrap(), 350);
        assert_eq!(resolve_interval_ms(None, None, Some(600), 1).unwrap(), 400);
        assert_eq!(resolve_interval_ms(None, None, Some(5), 100).unwrap(), 300);
        assert!(resolve_interval_ms(Some(400), Some(0.4), None, 1).is_err());
        assert!(resolve_interval_ms(None, Some(f32::NAN), None, 1).is_err());
        assert!(resolve_interval_ms(None, Some(0.2504), None, 1).is_err());
        assert!(resolve_interval_ms(None, None, Some(0), 1).is_err());
        assert!(resolve_interval_ms(Some(400), None, None, 0).is_err());
    }

    #[test]
    fn scheduler_uses_the_same_interval_without_catch_up_bursts() {
        let start = tokio::time::Instant::now();
        for ms in (250..=1000).step_by(50) {
            let interval = frame_interval(ms).unwrap();
            assert_eq!(
                next_frame_deadline(start, start + Duration::from_millis(20), interval),
                start + interval
            );
            let late = start + Duration::from_secs(2);
            assert_eq!(next_frame_deadline(start, late, interval), late);
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
