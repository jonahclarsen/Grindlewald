// Six edges of the saturated RGB color wheel, with 255 unit changes per edge.
pub const COLOR_COUNT: u16 = 6 * 255;
pub const MAX_COLOR_STEP: u16 = 2 * 255;

pub fn default_color_step() -> u16 {
    9 // Nearest integer to the previous 2-degree default (8.5 RGB steps).
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
    Ok((degrees * 255.0 / 60.0).round().max(1.0) as u16)
}

pub fn color_at_position(position: u16) -> String {
    let position = position % COLOR_COUNT;
    let offset = (position % 255) as u8;
    let [red, green, blue] = match position / 255 {
        0 => [255, offset, 0],
        1 => [255 - offset, 255, 0],
        2 => [0, 255, offset],
        3 => [0, 255 - offset, 255],
        4 => [offset, 0, 255],
        _ => [255, 0, 255 - offset],
    };
    format!("#{red:02x}{green:02x}{blue:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_hex_color;
    use std::collections::HashSet;

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
        for (degrees, expected) in [(0.1, 1), (2.0, 9), (6.0, 26), (18.0, 77), (120.0, 510)] {
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
