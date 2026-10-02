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

pub fn default_white_sweep_seconds() -> u32 {
    30
}

pub fn validate_white_sweep_seconds(seconds: u32) -> Result<(), String> {
    if !(5..=120).contains(&seconds) || seconds % 5 != 0 {
        return Err("white sweep must be 5–120 seconds in increments of 5".into());
    }
    Ok(())
}

// White uses the same phase range as color playback, reflected at each endpoint.
pub fn white_phase(origin: u16, elapsed: Duration, sweep_seconds: u32) -> u16 {
    ((f64::from(origin)
        + elapsed.as_secs_f64() * f64::from(COLOR_COUNT / 2) / f64::from(sweep_seconds))
        as u64
        % u64::from(COLOR_COUNT)) as u16
}

pub fn white_kelvin_at_phase(phase: u16) -> u16 {
    let phase = phase % COLOR_COUNT;
    let position = phase.min(COLOR_COUNT - phase);
    (2000.0 + f64::from(position) / f64::from(COLOR_COUNT / 2) * 7000.0).round() as u16
}

pub fn white_at_kelvin(kelvin: u16) -> String {
    let anchors = [
        (2000, [255.0, 141.0, 11.0]),
        (2700, [255.0, 169.0, 87.0]),
        (5500, [255.0, 238.0, 222.0]),
        (7500, [238.0, 239.0, 255.0]),
        (9000, [217.0, 225.0, 255.0]),
    ];
    let kelvin = kelvin.clamp(2000, 9000);
    let pair = anchors.windows(2).find(|pair| kelvin <= pair[1].0).unwrap();
    let amount = f64::from(kelvin - pair[0].0) / f64::from(pair[1].0 - pair[0].0);
    let rgb: [u8; 3] = std::array::from_fn(|index| {
        (pair[0].1[index] + (pair[1].1[index] - pair[0].1[index]) * amount).round() as u8
    });
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

pub fn white_kelvin_from_hex(value: &str) -> Result<u16, String> {
    let target = crate::protocol::parse_hex_color(value)?;
    Ok((2000..=9000)
        .step_by(70)
        .min_by_key(|kelvin| {
            let candidate = crate::protocol::parse_hex_color(&white_at_kelvin(*kelvin)).unwrap();
            candidate
                .iter()
                .zip(target)
                .map(|(a, b)| i32::from(*a).abs_diff(i32::from(b)).pow(2))
                .sum::<u32>()
        })
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_hex_color;
    use std::collections::HashSet;

    #[test]
    fn white_sweeps_hit_both_endpoints_and_return_at_the_selected_pace() {
        for seconds in (5..=120).step_by(5) {
            assert!(validate_white_sweep_seconds(seconds).is_ok());
            for (elapsed, kelvin) in [(0, 2000), (seconds, 9000), (2 * seconds, 2000)] {
                assert_eq!(
                    white_kelvin_at_phase(white_phase(
                        0,
                        Duration::from_secs(u64::from(elapsed)),
                        seconds
                    )),
                    kelvin
                );
            }
            assert_eq!(
                white_kelvin_at_phase(white_phase(
                    0,
                    Duration::from_secs_f64(f64::from(seconds) / 2.0),
                    seconds
                )),
                5495
            );
        }
        for seconds in [0, 4, 6, 121, u32::MAX] {
            assert!(validate_white_sweep_seconds(seconds).is_err());
        }
        assert_eq!(white_at_kelvin(2000), "#ff8d0b");
        assert_eq!(white_at_kelvin(5500), "#ffeede");
        assert_eq!(white_at_kelvin(9000), "#d9e1ff");
        for (hex, kelvin) in [("#ff8d0b", 2000), ("#ffeede", 5500), ("#d9e1ff", 9000)] {
            assert_eq!(white_kelvin_from_hex(hex).unwrap(), kelvin);
        }
    }

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
