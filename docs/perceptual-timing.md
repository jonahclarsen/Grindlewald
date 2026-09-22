# Proposed perceptual timing for breathing

This is a design proposal, not the current animation algorithm. The app currently uses equal intervals, a 1–100 RGB step, and a whole-cycle duration with a 300 ms minimum between breathing frames.

## Model and calibration

Keep the current RGB sequence. Convert each emitted color to Oklab and measure each adjacent pair with ΔEOK, the Euclidean distance between its L, a, and b coordinates. This metric includes lightness, chroma, and hue differences; it does not promise equal time in named color categories. The conversion and metric are published in [Ottosson's Oklab derivation](https://bottosson.github.io/posts/oklab/) and [CSS Color 4's color-difference definition](https://www.w3.org/TR/css-color-4/#color-difference-OK).

An initial estimate can treat the packet bytes as sRGB: divide by 255, apply the standard sRGB decoding transfer function, then apply the published linear-sRGB-to-Oklab matrices. This assumption is explicit: the Govee protocol's three bytes establish quantization, not calibrated primaries or a known transfer function. Use no invented red/green/blue sensitivity multipliers.

To calibrate, measure the light reflected from the intended surface using a suitable spectroradiometer or a colorimeter calibrated for the LEDs. Sample each channel at multiple drive levels, capture black/background and reference white, fit the RGB-to-XYZ relationship, and verify mixed colors. Repeat at relevant brightness settings if the response changes. Then use measured XYZ values and a defined reference white instead of assumed sRGB. See [CIE Colorimetry](https://www.cie.co.at/publications/colorimetry-4th-edition) for standard observers, tristimulus values, viewing conditions, and color differences.

Oklab is a useful starting approximation for ordinary viewing conditions. A color-difference metric is not itself a validated model of temporal perception. The bulb's native fade, room adaptation, and observer differences still need evaluation. For larger color steps, measuring or sampling the actual fade path would be better than assuming a straight perceptual transition between endpoints.

## Allocate the cycle time

For transition i, let d_i be its ΔEOK and T the requested full-cycle time. Without a rate limit, assign:

`duration_i = T × d_i / sum(d)`

Larger perceived changes take longer, making perceptual distance per second constant in the model. Small changes happen sooner. The number and order of RGB updates remain unchanged.

With the 300 ms floor, use:

`duration_i = max(0.300 seconds, k × d_i)`

Solve the single scale factor k by bisection so the durations sum to T. Do not merely clamp an already-normalized schedule: that would increase its total time. The formula preserves proportional timing among transitions above the floor; transitions at the floor move more slowly than the ideal perceptual speed. Normalize once per cycle configuration, including the closing transition.

At step 1, there are 1,530 transitions. At T = 459 seconds, every interval must be 300 ms. There is no freedom to redistribute time. A longer cycle provides room for weighting. Below that duration, satisfying the floor would require skipping RGB values; that is outside the requested timing-only approach.

## Calculated example, not bulb measurements

Using standard sRGB decoding, Ottosson's January 2021 Oklab matrices, ΔEOK, step 1, and a 600-second cycle:

| Transition | RGB change | ΔEOK | Allocated time |
| --- | --- | --- | --- |
| (255, 0, 0) → (255, 1, 0) | Green +1 | 0.000276658 | 300 ms |
| (255, 127, 0) → (255, 128, 0) | Green +1 | 0.002115993 | 501 ms |
| (255, 254, 0) → (255, 255, 0) | Green +1 | 0.002487824 | 589 ms |
| (0, 255, 255) → (0, 254, 255) | Green −1 | 0.002781549 | 658 ms |

Across the whole wheel, intervals range from 300 to approximately 663 ms; 641 of 1,530 intervals hit the floor. Unrounded intervals sum to 600 seconds. Every transition still changes exactly one channel by one.

Strictly proportional timing with no intervals clamped would require approximately 8,609 seconds (2h 23m 29s) under this particular model: `T >= 0.3 × sum(d) / min(d)`. That exceeds the current one-hour UI maximum and illustrates why bounded weighting is a practical compromise. Calibration may change these figures substantially. Neither a full cycle nor its smallest numerical RGB step guarantees perceptually uniform motion.
