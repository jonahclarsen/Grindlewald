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

const MIN_WHITE_KELVIN: u16 = 2_000;
const MAX_WHITE_KELVIN: u16 = 9_000;
const WHITE_ANCHORS: [(u16, [u8; 3]); 5] = [
    (2_000, [0xff, 0x8d, 0x0b]),
    (2_700, [0xff, 0xa9, 0x57]),
    (5_500, [0xff, 0xee, 0xde]),
    (7_500, [0xee, 0xef, 0xff]),
    (9_000, [0xd9, 0xe1, 0xff]),
];

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

pub fn party_frames(profile: DeviceProfile, rgb: [u8; 3], enter: bool) -> Vec<[u8; 20]> {
    match profile {
        DeviceProfile::Classic => vec![color_frame(profile, rgb)],
        DeviceProfile::H6005 => {
            let mut frames = Vec::with_capacity(usize::from(enter) + 1);
            if enter {
                frames.push(frame(0x05, &[0x05, 0x01]));
            }
            frames.push(frame(0x05, &[0x05, 0x00, rgb[0], rgb[1], rgb[2]]));
            frames
        }
    }
}

pub fn white_frame(profile: DeviceProfile, rgb: [u8; 3], kelvin: Option<u16>) -> [u8; 20] {
    match profile {
        DeviceProfile::Classic => frame(
            0x05,
            &[0x02, 0xff, 0xff, 0xff, 0x01, rgb[0], rgb[1], rgb[2]],
        ),
        DeviceProfile::H6005 => {
            let kelvin = kelvin
                .unwrap_or_else(|| infer_white_kelvin(rgb))
                .clamp(MIN_WHITE_KELVIN, MAX_WHITE_KELVIN);
            let [high, low] = kelvin.to_be_bytes();
            frame(
                0x05,
                &[
                    0x0d, rgb[0], rgb[1], rgb[2], high, low, rgb[0], rgb[1], rgb[2],
                ],
            )
        }
    }
}

pub fn keep_alive_frame() -> [u8; 20] {
    let mut bytes = [0_u8; 20];
    bytes[0] = 0xaa;
    bytes[1] = 0x01;
    bytes[19] = 0xab;
    bytes
}

fn infer_white_kelvin(rgb: [u8; 3]) -> u16 {
    WHITE_ANCHORS
        .windows(2)
        .flat_map(|anchors| {
            let (start_kelvin, start_rgb) = anchors[0];
            let (end_kelvin, end_rgb) = anchors[1];
            (start_kelvin..=end_kelvin).step_by(10).map(move |kelvin| {
                let amount =
                    f32::from(kelvin - start_kelvin) / f32::from(end_kelvin - start_kelvin);
                let candidate: [u8; 3] = std::array::from_fn(|index| {
                    (f32::from(start_rgb[index])
                        + (f32::from(end_rgb[index]) - f32::from(start_rgb[index])) * amount)
                        .round() as u8
                });
                let distance = candidate
                    .iter()
                    .zip(rgb)
                    .map(|(candidate, actual)| {
                        let delta = i32::from(*candidate) - i32::from(actual);
                        delta * delta
                    })
                    .sum::<i32>();
                (distance, kelvin)
            })
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, kelvin)| kelvin)
        .unwrap_or(5_500)
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
        let packet = white_frame(DeviceProfile::Classic, [0xff, 0xd5, 0xad], None);
        assert_eq!(
            &packet[2..10],
            &[0x02, 0xff, 0xff, 0xff, 0x01, 0xff, 0xd5, 0xad]
        );
    }

    #[test]
    fn h6005_white_mode_includes_kelvin_and_repeated_rgb() {
        let packet = white_frame(DeviceProfile::H6005, [0xff, 0x8d, 0x0b], Some(2_000));
        assert_eq!(
            &packet[..11],
            &[
                0x33, 0x05, 0x0d, 0xff, 0x8d, 0x0b, 0x07, 0xd0, 0xff, 0x8d, 0x0b
            ]
        );
        assert_eq!(packet[19], 0xec);
    }

    #[test]
    fn h6005_party_mode_enters_once_then_streams_instant_color() {
        let packets = party_frames(DeviceProfile::H6005, [0xff, 0, 0], true);
        assert_eq!(&packets[0][..5], &[0x33, 0x05, 0x05, 0x01, 0]);
        assert_eq!(&packets[1][..7], &[0x33, 0x05, 0x05, 0, 0xff, 0, 0]);
        assert_eq!(packets[0][19], 0x32);
        assert_eq!(packets[1][19], 0xcc);
    }

    #[test]
    fn keep_alive_matches_the_govee_no_op_packet() {
        assert_eq!(
            keep_alive_frame(),
            [
                0xaa, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xab,
            ]
        );
    }

    #[test]
    fn parses_css_hex_colors() {
        assert_eq!(parse_hex_color("#00a1FF").unwrap(), [0, 161, 255]);
        assert!(parse_hex_color("orange").is_err());
    }
}
