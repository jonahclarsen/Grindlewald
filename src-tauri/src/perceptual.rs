//! Approximate bulb colorimetry: sRGB primaries/transfer, D65 adaptation, and
//! brightness proportional to linear light output. These are not bulb measurements.
//! Oklab matrices: https://bottosson.github.io/posts/oklab/ (public-domain code).
//! Transfer and distance: https://www.w3.org/TR/css-color-4/#color-conversion-code

pub fn linear_srgb(rgb: [u8; 3]) -> [f64; 3] {
    rgb.map(|channel| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}

pub fn oklab(rgb: [u8; 3], brightness: f64) -> [f64; 3] {
    let [r, g, b] = linear_srgb(rgb).map(|channel| channel * brightness);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}

// Yellow is the maximum Oklab lightness on this saturated RGB wheel. With linear
// intensity scaling, L scales as brightness^(1/3), hence the cube of the L ratio.
// Quantize to the actual 8-bit brightness packet, never below the base packet.
pub fn boosted_brightness(rgb: [u8; 3], base: u8) -> u8 {
    if base == 0 || base == 255 {
        return base;
    }
    let target = oklab([255, 255, 0], 1.0)[0];
    let lightness = oklab(rgb, 1.0)[0];
    if lightness <= 0.0 {
        return 255;
    }
    (f64::from(base) * (target / lightness).powi(3))
        .round()
        .clamp(f64::from(base), 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breathing::{COLOR_COUNT, rgb_at_position};

    #[test]
    fn published_srgb_reference_values_and_transfer_endpoints() {
        for (rgb, expected) in [
            ([255, 255, 255], [1.0, 0.0, 0.0]),
            ([255, 0, 0], [0.62795536, 0.22486306, 0.12584630]),
            ([0, 255, 0], [0.86643961, -0.23388757, 0.17949848]),
            ([0, 0, 255], [0.45201372, -0.03245698, -0.31152815]),
        ] {
            assert!(distance(oklab(rgb, 1.0), expected) < 0.000001);
        }
        assert_eq!(linear_srgb([0, 0, 0]), [0.0; 3]);
        assert_eq!(linear_srgb([255, 255, 255]), [1.0; 3]);
        assert!((linear_srgb([10, 10, 10])[0] - (10.0 / 255.0 / 12.92)).abs() < 1e-12);
    }

    #[test]
    fn compensation_never_dims_or_exceeds_hardware_limits() {
        let maximum = oklab([255, 255, 0], 1.0)[0];
        for position in 0..COLOR_COUNT {
            let rgb = rgb_at_position(position);
            assert!(oklab(rgb, 1.0)[0] <= maximum + 1e-12);
            assert_eq!(boosted_brightness(rgb, 0), 0);
            assert_eq!(boosted_brightness(rgb, 255), 255);
            for base in [1, 10, 25, 102, 200, 254] {
                let boosted = boosted_brightness(rgb, base);
                assert!(boosted >= base);
                if boosted < 255 {
                    // Nearest-byte rounding stays within half a brightness byte of
                    // the exact constant-lightness solution (tested before cube root).
                    let target = f64::from(base) / 255.0 * maximum.powi(3);
                    let actual = oklab(rgb, f64::from(boosted) / 255.0)[0].powi(3);
                    assert!((target - actual).abs() <= maximum.powi(3) / 510.0 + 1e-12);
                }
            }
        }
        assert_eq!(boosted_brightness([255, 255, 0], 102), 102);
        assert_eq!(boosted_brightness([0, 0, 255], 102), 255);
    }
}
