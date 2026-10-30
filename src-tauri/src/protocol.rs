use serde::{Deserialize, Serialize};

pub const CONTROL_CHARACTERISTIC: &str = "00010203-0405-0607-0809-0a0b0c0d2b11";
pub const NOTIFY_CHARACTERISTIC: &str = "00010203-0405-0607-0809-0a0b0c0d2b10";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceProfile {
    Classic,
    H6005,
}

impl DeviceProfile {
    // Lights silently drop frames written closer together than this, measured with
    // `grindlewaldctl measure-step-interval` (src/spacing.rs). The H6001 (Classic)
    // is assumed to match the H6005 until it is measured.
    pub const fn min_write_spacing_ms(self) -> u64 {
        match self {
            Self::Classic => H6005_MIN_WRITE_SPACING_MS,
            Self::H6005 => H6005_MIN_WRITE_SPACING_MS,
        }
    }

    // Frames sent soon after a switch between color and white are dropped too,
    // so the frame after a switch waits this long, and the next switch twice this.
    pub const fn mode_switch_spacing_ms(self) -> u64 {
        match self {
            Self::Classic => H6005_MODE_SWITCH_SPACING_MS,
            Self::H6005 => H6005_MODE_SWITCH_SPACING_MS,
        }
    }

    pub const fn mode_switch_spacing(self) -> std::time::Duration {
        std::time::Duration::from_millis(self.mode_switch_spacing_ms())
    }

    pub const fn min_write_spacing(self) -> std::time::Duration {
        std::time::Duration::from_millis(self.min_write_spacing_ms())
    }

    // Bounds anything shared by every light, such as the breathing step interval.
    pub const fn slowest_write_spacing_ms() -> u64 {
        let classic = Self::Classic.min_write_spacing_ms();
        let h6005 = Self::H6005.min_write_spacing_ms();
        if classic > h6005 { classic } else { h6005 }
    }

    pub const fn slowest_write_spacing() -> std::time::Duration {
        std::time::Duration::from_millis(Self::slowest_write_spacing_ms())
    }

    pub const fn model_name(self) -> &'static str {
        match self {
            Self::Classic => "H6001",
            Self::H6005 => "H6005",
        }
    }

    fn manual_mode(self) -> u8 {
        match self {
            Self::Classic => 0x02,
            Self::H6005 => 0x0d,
        }
    }
}

// Measured on 2026-10-14. Across several connections, drops fell to the background
// rate (under 1%) from 26 ms between same-mode frames and 47 ms after a switch.
const H6005_MIN_WRITE_SPACING_MS: u64 = 26;
const H6005_MODE_SWITCH_SPACING_MS: u64 = 47;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameMode {
    Color,
    White,
    // Scene, music, and party sub-modes, by their 33 05 sub-command byte.
    Other(u8),
}

// The light mode a frame selects, or None for frames such as brightness and power.
pub fn frame_mode(profile: DeviceProfile, frame: &[u8; 20]) -> Option<FrameMode> {
    if frame[0] != 0x33 || frame[1] != 0x05 {
        return None;
    }
    Some(match (profile, frame[2]) {
        (DeviceProfile::Classic, 0x02) if frame[3..6] == [0xff; 3] && frame[6] == 0x01 => {
            FrameMode::White
        }
        (DeviceProfile::Classic, 0x02) => FrameMode::Color,
        // H6005 white frames carry a nonzero Kelvin value after the RGB bytes.
        (DeviceProfile::H6005, 0x0d) if frame[6..8] != [0, 0] => FrameMode::White,
        (DeviceProfile::H6005, 0x0d) => FrameMode::Color,
        (_, mode) => FrameMode::Other(mode),
    })
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

pub fn experimental_mode_frame(value: &str) -> Result<[u8; 20], String> {
    let payload = value
        .split_whitespace()
        .map(|part| {
            let byte = part.strip_prefix("0x").unwrap_or(part);
            if byte.is_empty() || byte.len() > 2 {
                return Err(format!("invalid experimental byte {part:?}"));
            }
            u8::from_str_radix(byte, 16).map_err(|_| format!("invalid experimental byte {part:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if payload.is_empty() || payload.len() > 17 {
        return Err("experimental payload needs 1 to 17 hexadecimal bytes".into());
    }
    Ok(frame(0x05, &payload))
}

// Built-in light scenes. Each model offers its own set under the shared IDs
// below, read from the Govee Home 3.6.1 app (H6001: com.govee.h6001.adjust.
// ScenesFragment; H6005: com.govee.bulblightv2.adjust.ui.ScenesFragment).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scene {
    Breathe,
    Reading,
    Romantic,
    Relax,
    Vivid,
    Colorful,
    Morning,
    Sunset,
    Candlelight,
    Fade,
    Energetic,
    Heartbeat,
    Blinking,
}

impl Scene {
    pub const ALL: [Self; 13] = [
        Self::Breathe,
        Self::Reading,
        Self::Romantic,
        Self::Relax,
        Self::Vivid,
        Self::Colorful,
        Self::Morning,
        Self::Sunset,
        Self::Candlelight,
        Self::Fade,
        Self::Energetic,
        Self::Heartbeat,
        Self::Blinking,
    ];

    // The scene ID, or None if the model's app does not offer the scene.
    pub const fn code(self, profile: DeviceProfile) -> Option<u8> {
        match (self, profile) {
            (Self::Breathe, _) => Some(0x0a),
            (Self::Reading, _) => Some(0x0d),
            (Self::Romantic, _) => Some(0x07),
            (Self::Relax, DeviceProfile::Classic) => Some(0x0e),
            (Self::Vivid, DeviceProfile::Classic) => Some(0x0c),
            (Self::Colorful, DeviceProfile::Classic) => Some(0x0b),
            (Self::Morning, DeviceProfile::Classic) => Some(0x00),
            (Self::Sunset, DeviceProfile::Classic) => Some(0x01),
            (Self::Candlelight, DeviceProfile::H6005) => Some(0x09),
            (Self::Fade, DeviceProfile::H6005) => Some(0x11),
            (Self::Energetic, DeviceProfile::H6005) => Some(0x10),
            (Self::Heartbeat, DeviceProfile::H6005) => Some(0x14),
            (Self::Blinking, DeviceProfile::H6005) => Some(0x19),
            _ => None,
        }
    }
}

pub fn scene_frame(profile: DeviceProfile, scene: Scene) -> Option<[u8; 20]> {
    scene.code(profile).map(|code| frame(0x05, &[0x04, code]))
}

pub const MAX_DIY_COLORS: usize = 8;
pub const MAX_DIY_SPEED: u8 = 100;
// The light runs DIY effects by a caller-chosen code; the Govee app uses 0xFE
// for effects shared between users.
const DIY_CODE: u8 = 0xfe;
const DIY_BREATHE_EFFECT: u8 = 0x05;
// The Govee app waits this long between the packets of a multi-packet upload.
pub const MULTI_PACKET_SPACING: std::time::Duration = std::time::Duration::from_millis(300);

// H6005 only: uploads a DIY breathing effect through the given colors, then
// switches to it. Speed runs from 0 (slowest) to 100.
pub fn diy_breathe_frames(colors: &[[u8; 3]], speed: u8) -> Result<Vec<[u8; 20]>, String> {
    if colors.is_empty() || colors.len() > MAX_DIY_COLORS {
        return Err(format!("DIY breathing needs 1 to {MAX_DIY_COLORS} colors"));
    }
    if speed > MAX_DIY_SPEED {
        return Err(format!(
            "DIY speed must be 0 to {MAX_DIY_SPEED}, got {speed}"
        ));
    }
    // Code, effect, sub-effect, speed, color byte count, colors, then an empty
    // list of mixed effects.
    let mut data = vec![
        DIY_CODE,
        DIY_BREATHE_EFFECT,
        0,
        speed,
        (colors.len() * 3) as u8,
    ];
    data.extend(colors.iter().flatten());
    data.push(0);
    let mut frames = multi_packet_frames(0x02, &data);
    frames.push(frame(0x05, &[0x0a, DIY_CODE]));
    Ok(frames)
}

// Splits data into a start packet with the packet count, 16-byte numbered
// packets, and an end packet, all under the A1 multi-packet write header.
fn multi_packet_frames(command: u8, data: &[u8]) -> Vec<[u8; 20]> {
    let chunks = data.chunks(16);
    let mut frames = vec![packet(0xa1, command, &[0, chunks.len() as u8, 0])];
    frames.extend(chunks.enumerate().map(|(index, chunk)| {
        let mut payload = [0_u8; 17];
        payload[0] = index as u8 + 1;
        payload[1..=chunk.len()].copy_from_slice(chunk);
        packet(0xa1, command, &payload)
    }));
    frames.push(packet(0xa1, command, &[0xff]));
    frames
}

pub fn is_multi_packet(frame: &[u8; 20]) -> bool {
    frame[0] == 0xa1
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

// Asks the light to report a setting on the notify characteristic (0x04 is
// brightness, 0x05 is the color mode and its RGB value).
pub fn status_query_frame(kind: u8) -> [u8; 20] {
    let mut bytes = [0_u8; 20];
    bytes[0] = 0xaa;
    bytes[1] = kind;
    bytes[19] = bytes[..19]
        .iter()
        .fold(0_u8, |checksum, byte| checksum ^ byte);
    bytes
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
    packet(0x33, command, payload)
}

fn packet(header: u8, command: u8, payload: &[u8]) -> [u8; 20] {
    debug_assert!(payload.len() <= 17);
    let mut bytes = [0_u8; 20];
    bytes[0] = header;
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
    fn experimental_mode_is_limited_to_a_color_command_payload() {
        let packet = experimental_mode_frame("04 08").unwrap();
        assert_eq!(&packet[..5], &[0x33, 0x05, 0x04, 0x08, 0]);
        assert_eq!(packet[19], 0x3a);
        assert!(experimental_mode_frame("04 definitely-not-hex").is_err());
    }

    #[test]
    fn scenes_use_each_models_ids_and_skip_unsupported_ones() {
        let breathe = scene_frame(DeviceProfile::Classic, Scene::Breathe).unwrap();
        assert_eq!(&breathe[..5], &[0x33, 0x05, 0x04, 0x0a, 0]);
        assert_eq!(breathe[19], 0x38);
        assert_eq!(
            scene_frame(DeviceProfile::H6005, Scene::Breathe),
            Some(breathe)
        );
        assert_eq!(
            scene_frame(DeviceProfile::H6005, Scene::Heartbeat).unwrap()[3],
            0x14
        );
        assert_eq!(scene_frame(DeviceProfile::Classic, Scene::Heartbeat), None);
        assert_eq!(
            scene_frame(DeviceProfile::Classic, Scene::Vivid).unwrap()[3],
            0x0c
        );
        assert_eq!(scene_frame(DeviceProfile::H6005, Scene::Vivid), None);
        for scene in Scene::ALL {
            assert!(
                [DeviceProfile::Classic, DeviceProfile::H6005]
                    .iter()
                    .any(|profile| scene.code(*profile).is_some()),
                "{scene:?}"
            );
        }
    }

    #[test]
    fn diy_breathing_uploads_in_numbered_packets_then_selects_the_effect() {
        let frames = diy_breathe_frames(&[[0xff, 0, 0], [0, 0, 0xff]], 50).unwrap();
        assert_eq!(frames.len(), 4);
        assert_eq!(&frames[0][..5], &[0xa1, 0x02, 0x00, 0x01, 0x00]);
        assert_eq!(
            &frames[1][..16],
            &[
                0xa1, 0x02, 0x01, 0xfe, 0x05, 0x00, 0x32, 0x06, 0xff, 0, 0, 0, 0, 0xff, 0, 0
            ]
        );
        assert_eq!(&frames[2][..3], &[0xa1, 0x02, 0xff]);
        assert_eq!(&frames[3][..4], &[0x33, 0x05, 0x0a, 0xfe]);
        for frame in &frames {
            assert_eq!(frame.iter().fold(0_u8, |checksum, byte| checksum ^ byte), 0);
        }
        assert!(frames[..3].iter().all(is_multi_packet));
        assert!(!is_multi_packet(&frames[3]));
    }

    #[test]
    fn diy_uploads_split_across_sixteen_byte_packets() {
        let colors = [[0x11, 0x22, 0x33]; MAX_DIY_COLORS];
        let frames = diy_breathe_frames(&colors, 100).unwrap();
        // 5 header bytes, 24 color bytes and the empty mix list make 30 bytes.
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[0][3], 2);
        assert_eq!(frames[2][2], 2);
        // The first packet ends partway through the fourth color.
        assert_eq!(&frames[1][3..8], &[0xfe, 0x05, 0x00, 0x64, 0x18]);
        assert_eq!(&frames[1][8..17], &[0x11, 0x22, 0x33].repeat(3)[..]);
        assert_eq!(&frames[1][17..19], &[0x11, 0x22]);
        assert_eq!(frames[2][3], 0x33);
        assert_eq!(&frames[2][4..16], &[0x11, 0x22, 0x33].repeat(4)[..]);
        assert_eq!(&frames[2][16..19], &[0, 0, 0]);
        assert!(diy_breathe_frames(&[], 50).is_err());
        assert!(diy_breathe_frames(&[[0; 3]; MAX_DIY_COLORS + 1], 50).is_err());
        assert!(diy_breathe_frames(&colors, 101).is_err());
    }

    #[test]
    fn frames_are_classified_by_the_mode_they_select() {
        for profile in [DeviceProfile::Classic, DeviceProfile::H6005] {
            for rgb in [[0xff, 0xff, 0xff], [0xff, 0x41, 0x00], [0, 0, 0]] {
                assert_eq!(
                    frame_mode(profile, &color_frame(profile, rgb)),
                    Some(FrameMode::Color)
                );
                assert_eq!(
                    frame_mode(profile, &white_frame(profile, rgb, None)),
                    Some(FrameMode::White)
                );
            }
            assert_eq!(frame_mode(profile, &brightness_frame(0.5).unwrap()), None);
            assert_eq!(frame_mode(profile, &power_frame(true)), None);
            assert_eq!(frame_mode(profile, &keep_alive_frame()), None);
        }
        assert_eq!(
            frame_mode(
                DeviceProfile::H6005,
                &party_frames(DeviceProfile::H6005, [0xff, 0, 0], true)[0]
            ),
            Some(FrameMode::Other(0x05))
        );
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
