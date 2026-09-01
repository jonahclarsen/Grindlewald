use serde::{Deserialize, Serialize};

pub const CONTROL_CHARACTERISTIC: &str = "00010203-0405-0607-0809-0a0b0c0d2b11";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceProfile {
    Classic,
    H6005,
}

impl DeviceProfile {
    fn manual_mode(self) -> u8 {
        match self {
            Self::Classic => 0x02,
            Self::H6005 => 0x0d,
        }
    }
}

pub fn parse_hex_color(value: &str) -> Result<[u8; 3], String> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("expected a color like #ff6b2c, got {value:?}"));
    }

    Ok([
        u8::from_str_radix(&hex[0..2], 16).map_err(|error| error.to_string())?,
        u8::from_str_radix(&hex[2..4], 16).map_err(|error| error.to_string())?,
        u8::from_str_radix(&hex[4..6], 16).map_err(|error| error.to_string())?,
    ])
}

pub fn color_frame(profile: DeviceProfile, rgb: [u8; 3]) -> [u8; 20] {
    frame(0x05, &[profile.manual_mode(), rgb[0], rgb[1], rgb[2]])
}

pub fn white_frame(profile: DeviceProfile, rgb: [u8; 3]) -> [u8; 20] {
    frame(
        0x05,
        &[
            profile.manual_mode(),
            0xff,
            0xff,
            0xff,
            0x01,
            rgb[0],
            rgb[1],
            rgb[2],
        ],
    )
}

pub fn brightness_frame(brightness: f32) -> Result<[u8; 20], String> {
    if !(0.0..=1.0).contains(&brightness) {
        return Err(format!(
            "brightness must be between 0 and 1, got {brightness}"
        ));
    }
    Ok(frame(0x04, &[(brightness * 255.0).round() as u8]))
}

pub fn power_frame(on: bool) -> [u8; 20] {
    frame(0x01, &[u8::from(on)])
}

fn frame(command: u8, payload: &[u8]) -> [u8; 20] {
    debug_assert!(payload.len() <= 17);
    let mut bytes = [0_u8; 20];
    bytes[0] = 0x33;
    bytes[1] = command;
    bytes[2..2 + payload.len()].copy_from_slice(payload);
    bytes[19] = bytes[..19]
        .iter()
        .fold(0_u8, |checksum, byte| checksum ^ byte);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_is_twenty_bytes_with_xor_checksum() {
        let packet = color_frame(DeviceProfile::H6005, [0xff, 0x6b, 0x2c]);
        assert_eq!(&packet[..6], &[0x33, 0x05, 0x0d, 0xff, 0x6b, 0x2c]);
        assert_eq!(
            packet.iter().fold(0_u8, |checksum, byte| checksum ^ byte),
            0
        );
    }

    #[test]
    fn white_mode_uses_the_dedicated_white_led_flag() {
        let packet = white_frame(DeviceProfile::Classic, [0xff, 0xd5, 0xad]);
        assert_eq!(
            &packet[2..10],
            &[0x02, 0xff, 0xff, 0xff, 0x01, 0xff, 0xd5, 0xad]
        );
    }

    #[test]
    fn parses_css_hex_colors() {
        assert_eq!(parse_hex_color("#00a1FF").unwrap(), [0, 161, 255]);
        assert!(parse_hex_color("orange").is_err());
    }
}
