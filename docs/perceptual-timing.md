# Perceptual breathing: implemented models

Breathing preserves the saturated RGB wheel, step range 1–100, and existing whole-cycle duration bounds (default 600 seconds, maximum 3,600). At step 1, the fastest cycle remains 459 seconds. The 300 ms figure now defines the minimum **average** interval through `ceil(ceil(1530 / step) × 0.3)`; it no longer limits individual updates.

## Colorimetry and explicit assumptions

Packet RGB values are treated as sRGB with a D65 reference white. Each channel is divided by 255 and decoded using the standard piecewise sRGB transfer function: `c / 12.92` below 0.04045, otherwise `((c + 0.055) / 1.055)^2.4`. The separate brightness control is assumed proportional to linear light output, so it multiplies the decoded channels before color-space conversion. These are uncalibrated assumptions about the bulbs, not measurements of their primaries, driver response, or firmware.

The linear values are transformed into Oklab using the published January 2021 matrices and cube-root responses. Oklab's L coordinate estimates lightness; a and b describe opponent color directions. The implementation uses Euclidean distance in these coordinates, ΔEOK. Sources:

- [Ottosson's Oklab derivation and public-domain conversion code](https://bottosson.github.io/posts/oklab/).
- [CSS Color 4 sample color conversions](https://www.w3.org/TR/css-color-4/#color-conversion-code).
- [CSS Color 4 ΔEOK definition](https://www.w3.org/TR/css-color-4/#color-difference-OK).

The model uses all three coordinates, not hue alone. It aims for approximately equal perceptual distance per second, not equal time in named categories such as green or orange. There are no manually chosen per-color timing multipliers.

## Brightness compensation that only boosts

Let B be the user's brightness, quantized to the same 8-bit packet the bulb normally receives. Let L(c) be the Oklab lightness of wheel color c at full output. Yellow, RGB (255,255,0), has the greatest L on this wheel; this is verified across all 1,530 colors in tests.

The target lightness is the lightness of yellow at B. Because Oklab applies cube roots after a linear transform, scaling linear intensity by b scales L by `b^(1/3)`. Solving for the brightness that reaches the target gives:

`boost(c) = clamp(B × (L(yellow) / L(c))^3, B, 1)`

The result is rounded to the nearest of 256 brightness values and clamped to never fall below B's packet value. Zero remains zero; full brightness remains full brightness at every hue. Thus “only brighten” means relative to the selected base, not that brightness can never fall back from a previous boost.

At a 40% base, yellow stays at 40%, green rises to approximately 55.7%, and red and blue reach the 100% cap. At 100%, no color can be boosted, so no equalization is possible under the no-dimming constraint. Where the cap is reached, the target lightness cannot be achieved. Quantization also prevents exact equality even below the cap. The model improves consistency where headroom exists; it cannot guarantee constant apparent brightness for all settings.

Brightness compensation is transient. It does not rewrite the slider, settings, or presets. The BLE state tracks temporary brightness separately and restores the selected static level when ordinary controls resume. A brightness packet is omitted when its encoded value has not changed.

## Timing after brightness compensation

For each frame, calculate Oklab from the original RGB bytes and the actual compensated 8-bit brightness. Let d_i be ΔEOK from frame i to the next frame, including the closing edge. For requested cycle duration T:

`interval_i = T × d_i / sum(d)`

Every RGB step remains in the sequence. Large predicted changes get longer intervals and small changes get shorter intervals, while planned durations sum to T. Computing distances after brightness quantization accounts for actual brightness jumps as well as RGB changes. At brightness zero all colors are black, so timing falls back to the full-output hue distances while still emitting zero brightness.

The entire plan is precomputed at effect start and rebuilt when the user seeks to a new origin. A step that does not divide 1,530 gets a shorter closing step. The final color returns exactly to the origin. The minimum cycle duration continues to use the same count of frames and is independent of brightness.

Write time counts toward the scheduled interval. A late write does not generate a backlog of catch-up writes, but can stretch the actual cycle. There is no claim that native bulb fades follow the same interpolation path or complete at the requested deadline. Color and brightness use separate BLE packets; when brightness changes, its packet follows the color packet in the same update.

## Reproducible maximum scheduled rates

Run the actual Rust model without connecting to lights:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --example breathing_model
```

At step 1 and a 459-second cycle, the shortest interval is near saturated green: **RGB (10,255,0) → (9,255,0)**, or **#0aff00 → #09ff00**, approximately 118° hue. A random starting position merely rotates the same step-1 sequence.

| Selected base | Brightness during fastest transition | Shortest interval | Color updates/second | Longest interval |
| --- | --- | --- | --- | --- |
| 10% | 36/255 | 11.328706 ms | 88.271 | 1,583.271732 ms |
| 40% | 142/255 | 14.922004 ms | 67.015 | 657.068667 ms |
| 100% | 255/255 | 15.995375 ms | 62.518 | 590.756829 ms |

These are calculated color-frame intervals, not measured radio throughput. The darkest red-channel changes near green have especially small modeled distances. The numbers vary with base brightness because compensation and quantization change the path and its total perceptual distance. The fastest transition itself needs no extra brightness packet. Elsewhere the full cycle includes 484 brightness changes at 10%, 270 at 40%, and none at 100%, in addition to initial synchronization.

## Live hue display and seeking

After each successful write the backend publishes the target hue, applied brightness, effect generation, frame sequence, and seek request ID. The frontend moves the existing hue handle and swatch without altering the saved static color. Sequence IDs reject late status snapshots. While dragging, the pointer controls the handle; coalesced seek requests wake the running effect, apply the latest origin, and continue without stopping or restoring a preset. The display shows commanded hue, not a sensor measurement of the ongoing physical fade.

If Full cycle was at its minimum before a Color step change, it follows the new minimum in either direction. A longer manually chosen duration is preserved unless it falls below the new minimum.

## Why this is plausible, and how to improve it

Perceptual-distance timing makes `distance / time` constant within the model. The brightness equation solves for constant modeled lightness wherever the cap and quantization permit. Both follow from published transforms and explicit assumptions, so the results are reproducible and testable.

Color-difference models describe perceived differences under assumed viewing conditions; they do not fully predict temporal adaptation or a particular lamp's fading behavior. Oklab L is an approximation to perceived lightness, not a promise of equal subjective brightness for all saturated emitters or observers. The weakest physical assumptions are sRGB-like primaries/transfer and brightness linearity.

For calibration, measure channel responses at multiple drive levels, a reference white, and representative mixed colors using a suitable spectroradiometer or LED-calibrated colorimeter. Fit a device RGB-to-XYZ model and brightness response, verify the prediction on held-out colors, then substitute those values before Oklab conversion. Measure the native fade separately to assess timing. [CIE Colorimetry](https://www.cie.co.at/publications/colorimetry-4th-edition) supplies the framework for standard observers, tristimulus coordinates, viewing conditions, and color differences. No bulb measurements have been performed for this implementation.

Validation covers reference colors, transfer-function endpoints, the maximum-lightness target across the wheel, no-dimming/full-output/off invariants, quantization error, every supported step size's total duration and ordering, packet deduplication/restoration, seek coalescing, stale frame rejection, and live hue UI behavior.
