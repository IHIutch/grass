//! CSS Color Level 4 color representation.
//!
//! Colors are stored in their native color space with three channels and an alpha.
//! Legacy spaces (RGB, HSL, HWB) use their traditional channel ranges:
//! - RGB: red/green/blue in [0, 255]
//! - HSL: hue in [0, 360], saturation/lightness in [0, 1]
//! - HWB: hue in [0, 360], whiteness/blackness in [0, 1]
//!
//! Modern spaces store channels in their natural ranges per CSS Color Level 4.
//!
//! Channels may be `None` to represent the CSS "missing" component (`none` keyword).
//! This is important for interpolation behavior in CSS Color 4.

use crate::value::{fuzzy_round, Number};
pub(crate) use name::NAMED_COLORS;
pub(crate) use space::{ColorSpace, HueInterpolationMethod};

pub(crate) mod conversion;
mod name;
pub(crate) mod space;

#[derive(Debug, Clone)]
pub struct Color {
    /// The color space this color is stored in.
    space: ColorSpace,
    /// Three channel values in the native space, or None for missing channels.
    channels: [Option<f64>; 3],
    /// Alpha channel (0.0-1.0), or None for missing alpha.
    alpha: Option<f64>,
    /// How this color should be serialized.
    pub(crate) format: ColorFormat,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ColorFormat {
    Rgb,
    Hsl,
    /// Literal string from source text. Either a named color like `red` or a hex color
    Literal(String),
    /// Use the most appropriate format
    Infer,
}

impl PartialEq for Color {
    fn eq(&self, other: &Self) -> bool {
        let self_alpha = self.alpha();
        let other_alpha = other.alpha();
        if self_alpha != other_alpha
            && !(self_alpha >= Number::one() && other_alpha >= Number::one())
        {
            return false;
        }

        // Compare in RGB space for legacy colors
        let self_rgb = self.to_rgb_channels();
        let other_rgb = other.to_rgb_channels();

        let cmp = |a: Number, b: Number| -> bool {
            a == b || (a >= Number(255.0) && b >= Number(255.0))
        };

        cmp(Number(self_rgb[0]).round(), Number(other_rgb[0]).round())
            && cmp(Number(self_rgb[1]).round(), Number(other_rgb[1]).round())
            && cmp(Number(self_rgb[2]).round(), Number(other_rgb[2]).round())
    }
}

impl Eq for Color {}

impl Color {
    /// Get the RGB channel values [red, green, blue] in 0-255 range.
    /// Converts from native space if necessary.
    pub(crate) fn to_rgb_channels(&self) -> [f64; 3] {
        let raw = self.to_rgb_channels_raw();
        match self.space {
            ColorSpace::Rgb => raw,
            ColorSpace::Hsl | ColorSpace::Hwb => [
                fuzzy_round(raw[0]),
                fuzzy_round(raw[1]),
                fuzzy_round(raw[2]),
            ],
            _ => raw,
        }
    }

    /// Raw RGB channel values without rounding, for out-of-gamut/fractional detection.
    pub(crate) fn to_rgb_channels_raw(&self) -> [f64; 3] {
        let c0 = self.channels[0].unwrap_or(0.0);
        let c1 = self.channels[1].unwrap_or(0.0);
        let c2 = self.channels[2].unwrap_or(0.0);

        match self.space {
            ColorSpace::Rgb => [c0, c1, c2],
            ColorSpace::Hsl => {
                let srgb = conversion::hsl_to_srgb(c0, c1, c2);
                [srgb[0] * 255.0, srgb[1] * 255.0, srgb[2] * 255.0]
            }
            ColorSpace::Hwb => {
                let srgb = conversion::hwb_to_srgb(c0, c1, c2);
                [srgb[0] * 255.0, srgb[1] * 255.0, srgb[2] * 255.0]
            }
            _ => {
                // Modern spaces: convert through XYZ to sRGB, scale to 0-255
                let srgb = conversion::convert([c0, c1, c2], self.space, ColorSpace::SRgb);
                [srgb[0] * 255.0, srgb[1] * 255.0, srgb[2] * 255.0]
            }
        }
    }

    /// Get the HSL channel values (hue, saturation, lightness).
    /// Returns (hue 0-360, saturation 0-1, lightness 0-1).
    fn to_hsl_channels(&self) -> [f64; 3] {
        match self.space {
            ColorSpace::Hsl => [
                self.channels[0].unwrap_or(0.0),
                self.channels[1].unwrap_or(0.0),
                self.channels[2].unwrap_or(0.0),
            ],
            _ => {
                let rgb = self.to_rgb_channels();
                conversion::srgb_to_hsl(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0)
            }
        }
    }

    pub fn color_space(&self) -> ColorSpace {
        self.space
    }

    /// Get the raw channel values (with None for missing channels).
    pub fn raw_channels(&self) -> [Option<f64>; 3] {
        self.channels
    }

    /// Get the raw alpha value (None for missing alpha).
    pub fn raw_alpha(&self) -> Option<f64> {
        self.alpha
    }
}

// ---- Legacy constructors ----
impl Color {
    pub(crate) const fn new_rgba(
        red: Number,
        green: Number,
        blue: Number,
        alpha: Number,
        format: ColorFormat,
    ) -> Color {
        Color {
            space: ColorSpace::Rgb,
            channels: [Some(red.0), Some(green.0), Some(blue.0)],
            alpha: Some(alpha.0),
            format,
        }
    }

    /// Create from named color lookup (rgba bytes).
    pub fn new(red: u8, green: u8, blue: u8, alpha: u8, format: String) -> Self {
        Color {
            space: ColorSpace::Rgb,
            channels: [Some(red as f64), Some(green as f64), Some(blue as f64)],
            alpha: Some(alpha as f64 / 255.0),
            format: ColorFormat::Literal(format),
        }
    }

    /// Create a new `Color` with just RGBA values.
    /// Color representation is created automatically.
    pub fn from_rgba(
        red: Number,
        green: Number,
        blue: Number,
        alpha: Number,
    ) -> Self {
        Color {
            space: ColorSpace::Rgb,
            channels: [
                Some(fuzzy_round(red.0).clamp(0.0, 255.0)),
                Some(fuzzy_round(green.0).clamp(0.0, 255.0)),
                Some(fuzzy_round(blue.0).clamp(0.0, 255.0)),
            ],
            alpha: Some(alpha.clamp(0.0, 1.0).0),
            format: ColorFormat::Infer,
        }
    }

    pub fn from_rgba_fn(
        red: Number,
        green: Number,
        blue: Number,
        alpha: Number,
    ) -> Self {
        Color {
            space: ColorSpace::Rgb,
            channels: [
                Some(red.0.clamp(0.0, 255.0)),
                Some(green.0.clamp(0.0, 255.0)),
                Some(blue.0.clamp(0.0, 255.0)),
            ],
            alpha: Some(alpha.clamp(0.0, 1.0).0),
            format: ColorFormat::Rgb,
        }
    }

    /// Create RGBA representation from HSLA values.
    /// hue in degrees, saturation and lightness in [0, 1].
    pub fn from_hsla(hue: Number, saturation: Number, lightness: Number, alpha: Number) -> Self {
        let hue = hue % Number(360.0);
        Color {
            space: ColorSpace::Hsl,
            channels: [
                Some(hue.0),
                Some(saturation.0.max(0.0)), // saturation clamped to non-negative
                Some(lightness.0),
            ],
            alpha: Some(alpha.clamp(0.0, 1.0).0),
            format: ColorFormat::Infer,
        }
    }

    pub fn from_hsla_fn(hue: Number, saturation: Number, luminance: Number, alpha: Number) -> Self {
        let mut color = Self::from_hsla(hue, saturation, luminance, alpha);
        color.format = ColorFormat::Hsl;
        color
    }

    /// Create a color in any color space with explicit channels and alpha.
    /// Channels and alpha can be None to represent CSS "none" (missing).
    pub(crate) fn for_space(
        space: ColorSpace,
        channels: [Option<f64>; 3],
        alpha: Option<f64>,
        format: ColorFormat,
    ) -> Self {
        Color {
            space,
            channels,
            alpha,
            format,
        }
    }

    pub fn from_hwb(hue: Number, white: Number, black: Number, alpha: Number) -> Color {
        let h = hue.rem_euclid(360.0);
        let mut w = white.0 / 100.0;
        let mut b = black.0 / 100.0;

        // When whiteness + blackness > 1, normalize proportionally (CSS Color 4 spec)
        let sum = w + b;
        if sum > 1.0 {
            w /= sum;
            b /= sum;
        }

        Color {
            space: ColorSpace::Hwb,
            channels: [Some(h), Some(w), Some(b)],
            alpha: Some(alpha.0.clamp(0.0, 1.0)),
            format: ColorFormat::Infer,
        }
    }
}

// ---- RGBA getters ----
impl Color {
    pub fn red(&self) -> Number {
        Number(self.to_rgb_channels()[0]).round()
    }

    pub fn green(&self) -> Number {
        Number(self.to_rgb_channels()[1]).round()
    }

    pub fn blue(&self) -> Number {
        Number(self.to_rgb_channels()[2]).round()
    }

    /// Mix two colors together with weight.
    /// Algorithm adapted from dart-sass.
    pub fn mix(&self, other: &Color, weight: Number) -> Self {
        let weight = weight.clamp(0.0, 100.0);
        let normalized_weight = weight * Number(2.0) - Number::one();
        let alpha_distance = self.alpha() - other.alpha();

        let combined_weight1 = if normalized_weight * alpha_distance == Number(-1.0) {
            normalized_weight
        } else {
            (normalized_weight + alpha_distance)
                / (Number::one() + normalized_weight * alpha_distance)
        };
        let weight1 = (combined_weight1 + Number::one()) / Number(2.0);
        let weight2 = Number::one() - weight1;

        Color::from_rgba(
            self.red() * weight1 + other.red() * weight2,
            self.green() * weight1 + other.green() * weight2,
            self.blue() * weight1 + other.blue() * weight2,
            self.alpha() * weight + other.alpha() * (Number::one() - weight),
        )
    }

    /// Simple linear interpolation mix in a given color space.
    /// Both colors must already be in the target space.
    /// weight is the proportion of self (1.0 = 100% self, 0.0 = 100% other).
    fn mix_in_space(&self, other: &Color, weight: Number, space: ColorSpace) -> Color {
        let w1 = weight.0;
        let w2 = 1.0 - w1;

        let channels: [Option<f64>; 3] = std::array::from_fn(|i| {
            let v1 = self.channels[i].unwrap_or(0.0);
            let v2 = other.channels[i].unwrap_or(0.0);
            Some(v1 * w1 + v2 * w2)
        });

        let a1 = self.alpha.unwrap_or(1.0);
        let a2 = other.alpha.unwrap_or(1.0);

        Color {
            space,
            channels,
            alpha: Some(a1 * w1 + a2 * w2),
            format: ColorFormat::Infer,
        }
    }

    /// CSS Color 4 interpolation with full support for:
    /// - Hue interpolation methods (polar spaces)
    /// - Missing channel handling
    /// - Powerless hue detection
    /// - Premultiplied alpha interpolation
    ///
    /// `weight` is the proportion of `self` (0.0–1.0).
    /// Result is converted back to `self`'s original color space.
    pub fn mix_with_method(
        &self,
        other: &Color,
        weight: f64,
        space: ColorSpace,
        hue_method: HueInterpolationMethod,
    ) -> Color {
        let c1 = self.to_space(space);
        let c2 = other.to_space(space);

        let hue_idx = space.hue_channel_index();

        // Step 1: Resolve missing channels.
        // If one color has `none` for a channel, use the other color's value.
        // If both are `none`, result is `none`.
        let mut v1 = [0.0_f64; 3];
        let mut v2 = [0.0_f64; 3];
        let mut both_none = [false; 3];

        for i in 0..3 {
            let ch1 = c1.channels[i];
            let ch2 = c2.channels[i];

            match (ch1, ch2) {
                (None, None) => {
                    both_none[i] = true;
                    v1[i] = 0.0;
                    v2[i] = 0.0;
                }
                (None, Some(val)) => {
                    v1[i] = val;
                    v2[i] = val;
                }
                (Some(val), None) => {
                    v1[i] = val;
                    v2[i] = val;
                }
                (Some(a), Some(b)) => {
                    v1[i] = a;
                    v2[i] = b;
                }
            }
        }

        // Step 2: Hue interpolation adjustment (only for polar spaces with a hue channel)
        if let Some(hi) = hue_idx {
            if !both_none[hi] {
                let h1 = v1[hi].rem_euclid(360.0);
                let h2 = v2[hi].rem_euclid(360.0);
                let (adj1, adj2) = adjust_hue(h1, h2, hue_method);
                v1[hi] = adj1;
                v2[hi] = adj2;
            }
        }

        // Step 3: Premultiplied alpha interpolation
        let a1 = c1.alpha.unwrap_or(1.0);
        let a2 = c2.alpha.unwrap_or(1.0);

        let w1 = weight;
        let w2 = 1.0 - weight;

        let result_alpha = a1 * w1 + a2 * w2;

        // Premultiply non-hue, non-both-none channels by alpha
        let mut result_channels: [Option<f64>; 3] = [None; 3];

        for i in 0..3 {
            if both_none[i] {
                result_channels[i] = None;
                continue;
            }

            let is_hue = hue_idx == Some(i);

            if is_hue {
                // Hue is not premultiplied
                result_channels[i] = Some((v1[i] * w1 + v2[i] * w2).rem_euclid(360.0));
            } else {
                // Premultiplied alpha interpolation for non-hue channels
                let pm1 = v1[i] * a1;
                let pm2 = v2[i] * a2;
                let pm_result = pm1 * w1 + pm2 * w2;

                if result_alpha == 0.0 {
                    result_channels[i] = Some(0.0);
                } else {
                    result_channels[i] = Some(pm_result / result_alpha);
                }
            }
        }

        let result = Color {
            space,
            channels: result_channels,
            alpha: Some(result_alpha),
            format: ColorFormat::Infer,
        };

        // Convert back to original space
        if self.space != space {
            result.to_space(self.space)
        } else {
            result
        }
    }
}

// ---- HSLA getters ----
impl Color {
    /// Calculate hue (0-360 degrees)
    pub fn hue(&self) -> Number {
        if self.space == ColorSpace::Hsl {
            return Number(self.channels[0].unwrap_or(0.0));
        }

        let hsl = self.to_hsl_channels();
        Number(hsl[0]) % Number(360.0)
    }

    /// Calculate saturation (0-100%)
    pub fn saturation(&self) -> Number {
        if self.space == ColorSpace::Hsl {
            return Number(self.channels[1].unwrap_or(0.0)) * Number(100.0);
        }

        let rgb = self.to_rgb_channels();
        let red = Number(rgb[0]) / Number(255.0);
        let green = Number(rgb[1]) / Number(255.0);
        let blue = Number(rgb[2]) / Number(255.0);

        let min = red.min(green.min(blue));
        let max = red.max(green.max(blue));

        if min == max {
            return Number::zero();
        }

        let delta = max - min;
        let sum = max + min;

        let s = delta
            / if sum > Number::one() {
                Number(2.0) - sum
            } else {
                sum
            };

        s * Number(100.0)
    }

    /// Calculate lightness (0-100%)
    pub fn lightness(&self) -> Number {
        if self.space == ColorSpace::Hsl {
            return Number(self.channels[2].unwrap_or(0.0)) * Number(100.0);
        }

        let rgb = self.to_rgb_channels();
        let red = Number(rgb[0]) / Number(255.0);
        let green = Number(rgb[1]) / Number(255.0);
        let blue = Number(rgb[2]) / Number(255.0);
        let min = red.min(green.min(blue));
        let max = red.max(green.max(blue));
        (((min + max) / Number(2.0)) * Number(100.0)).round()
    }

    pub fn as_hsla(&self) -> (Number, Number, Number, Number) {
        if self.space == ColorSpace::Hsl {
            return (
                Number(self.channels[0].unwrap_or(0.0)),
                Number(self.channels[1].unwrap_or(0.0)),
                Number(self.channels[2].unwrap_or(0.0)),
                self.alpha(),
            );
        }

        let rgb = self.to_rgb_channels();
        let red = Number(rgb[0]) / Number(255.0);
        let green = Number(rgb[1]) / Number(255.0);
        let blue = Number(rgb[2]) / Number(255.0);
        let min = red.min(green.min(blue));
        let max = red.max(green.max(blue));

        let lightness = (min + max) / Number(2.0);

        let saturation = if min == max {
            Number::zero()
        } else {
            let d = max - min;
            let mm = max + min;
            d / if mm > Number::one() {
                Number(2.0) - mm
            } else {
                mm
            }
        };

        let mut hue = if min == max {
            Number::zero()
        } else if blue == max {
            Number(4.0) + (red - green) / (max - min)
        } else if green == max {
            Number(2.0) + (blue - red) / (max - min)
        } else {
            (green - blue) / (max - min)
        };

        if hue.is_negative() {
            hue += Number(360.0);
        }

        hue *= Number(60.0);

        (hue % Number(360.0), saturation, lightness, self.alpha())
    }

    /// If this color was created from an HSL function call, preserve that format.
    fn hsl_format_if_preserved(&self) -> ColorFormat {
        match &self.format {
            ColorFormat::Hsl => ColorFormat::Hsl,
            _ => ColorFormat::Infer,
        }
    }

    pub fn adjust_hue(&self, degrees: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        let mut c = Color::from_hsla(hue + degrees, saturation, luminance, alpha);
        c.format = self.hsl_format_if_preserved();
        c
    }

    pub fn lighten(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        let mut c = Color::from_hsla(hue, saturation, luminance + amount, alpha);
        c.format = self.hsl_format_if_preserved();
        c
    }

    pub fn darken(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        let mut c = Color::from_hsla(hue, saturation, luminance - amount, alpha);
        c.format = self.hsl_format_if_preserved();
        c
    }

    pub fn saturate(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        let mut c = Color::from_hsla(hue, (saturation + amount).clamp(0.0, 1.0), luminance, alpha);
        c.format = self.hsl_format_if_preserved();
        c
    }

    pub fn desaturate(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        let mut c = Color::from_hsla(hue, (saturation - amount).clamp(0.0, 1.0), luminance, alpha);
        c.format = self.hsl_format_if_preserved();
        c
    }

    pub fn invert(&self, weight: Number) -> Self {
        if weight.is_zero() {
            return self.clone();
        }

        let rgb = self.to_rgb_channels();
        let red = Number(255.0) - Number(rgb[0]).round();
        let green = Number(255.0) - Number(rgb[1]).round();
        let blue = Number(255.0) - Number(rgb[2]).round();

        let inverse = Color {
            space: ColorSpace::Rgb,
            channels: [Some(red.0), Some(green.0), Some(blue.0)],
            alpha: self.alpha,
            format: ColorFormat::Infer,
        };

        inverse.mix(self, weight)
    }

    /// Invert a color in a given color space.
    ///
    /// Channel inversion rules:
    /// - Hue (polar): rotate by 180°
    /// - Chroma/saturation: unchanged (magnitude, not position)
    /// - HWB whiteness/blackness: swap with each other
    /// - All other channels: reflect around midpoint (max + min - value)
    pub fn invert_in_space(&self, space: ColorSpace, weight: Number) -> Self {
        let in_space = self.to_space(space);

        let channels_def = space.channels();
        let mut inverted_channels: [Option<f64>; 3] = [None; 3];

        // HWB is special: whiteness and blackness swap
        if space == ColorSpace::Hwb {
            inverted_channels[0] = in_space.channels[0].map(|h| (h + 180.0) % 360.0);
            inverted_channels[1] = in_space.channels[2]; // w' = old b
            inverted_channels[2] = in_space.channels[1]; // b' = old w
        } else {
            for (i, ch_def) in channels_def.iter().enumerate() {
                let val = in_space.channels[i];
                inverted_channels[i] = val.map(|v| {
                    if ch_def.is_polar {
                        (v + 180.0) % 360.0
                    } else if ch_def.name == "chroma" || ch_def.name == "saturation" {
                        v
                    } else {
                        ch_def.max + ch_def.min - v
                    }
                });
            }
        }

        let inverse = Color {
            space,
            channels: inverted_channels,
            alpha: in_space.alpha,
            format: ColorFormat::Infer,
        };

        if weight == Number::one() {
            // Convert back to original space
            if space != self.color_space() {
                inverse.to_space(self.color_space())
            } else {
                inverse
            }
        } else {
            // Mix the inverse with the original, then convert back
            let mixed = inverse.mix_in_space(&in_space, weight, space);
            if space != self.color_space() {
                mixed.to_space(self.color_space())
            } else {
                mixed
            }
        }
    }

    pub fn complement(&self) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        let mut c = Color::from_hsla(hue + Number(180.0), saturation, luminance, alpha);
        c.format = self.hsl_format_if_preserved();
        c
    }

    /// Complement a color in a given color space (rotate hue by 180 degrees).
    pub fn complement_in_space(&self, space: ColorSpace) -> Self {
        let in_space = self.to_space(space);
        let hue_idx = space.hue_channel_index();

        if let Some(idx) = hue_idx {
            let mut channels = in_space.channels;
            channels[idx] = channels[idx].map(|h| (h + 180.0) % 360.0);
            let result = Color {
                space,
                channels,
                alpha: in_space.alpha,
                format: ColorFormat::Infer,
            };
            if space != self.color_space() {
                result.to_space(self.color_space())
            } else {
                result
            }
        } else {
            // Non-polar space: no hue to rotate. Return unchanged.
            self.clone()
        }
    }
}

// ---- Alpha/opacity ----
impl Color {
    pub fn alpha(&self) -> Number {
        Number(self.alpha.unwrap_or(1.0))
    }

    /// Change `alpha` to value given
    pub fn with_alpha(&self, alpha: Number) -> Self {
        let rgb = self.to_rgb_channels();
        Color::from_rgba(
            Number(rgb[0]).round(),
            Number(rgb[1]).round(),
            Number(rgb[2]).round(),
            alpha,
        )
    }

    pub fn fade_in(&self, amount: Number) -> Self {
        let rgb = self.to_rgb_channels();
        Color::from_rgba(
            Number(rgb[0]).round(),
            Number(rgb[1]).round(),
            Number(rgb[2]).round(),
            self.alpha() + amount,
        )
    }

    pub fn fade_out(&self, amount: Number) -> Self {
        let rgb = self.to_rgb_channels();
        Color::from_rgba(
            Number(rgb[0]).round(),
            Number(rgb[1]).round(),
            Number(rgb[2]).round(),
            self.alpha() - amount,
        )
    }
}

// ---- Other ----
impl Color {
    pub fn to_ie_hex_str(&self) -> String {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            fuzzy_round(self.alpha().0 * 255.0) as u8,
            self.red().0 as u8,
            self.green().0 as u8,
            self.blue().0 as u8
        )
    }
}

// ---- HWB getters ----
impl Color {
    pub fn whiteness(&self) -> Number {
        self.red().min(self.green()).min(self.blue()) / Number(255.0)
    }

    pub fn blackness(&self) -> Number {
        Number(1.0) - (self.red().max(self.green()).max(self.blue()) / Number(255.0))
    }
}

// ---- Color space conversion and query methods ----
impl Color {
    /// Convert this color to a different color space.
    pub fn to_space(&self, target: ColorSpace) -> Self {
        if self.space == target {
            let mut result = self.clone();
            // Ensure correct serialization format for to-space() results
            if target == ColorSpace::Hsl || target == ColorSpace::Hwb {
                result.format = ColorFormat::Hsl;
            }
            return result;
        }

        let c0 = self.channels[0].unwrap_or(0.0);
        let c1 = self.channels[1].unwrap_or(0.0);
        let c2 = self.channels[2].unwrap_or(0.0);

        let converted = conversion::convert([c0, c1, c2], self.space, target);

        // Propagate missing channels: if a source channel was None and the
        // target has an analogous channel, that target channel becomes None too.
        // Analogous channels can be at different indices (e.g. HSL hue=0, LCH hue=2).
        let new_channels = [
            if should_propagate_none(self.space, &self.channels, target, 0) {
                None
            } else {
                Some(converted[0])
            },
            if should_propagate_none(self.space, &self.channels, target, 1) {
                None
            } else {
                Some(converted[1])
            },
            if should_propagate_none(self.space, &self.channels, target, 2) {
                None
            } else {
                Some(converted[2])
            },
        ];

        let mut new_channels = new_channels;

        if target.is_legacy() {
            // For legacy targets, `none` from analogous source channels
            // is replaced with 0 instead of being propagated as `none`.
            for (i, ch) in new_channels.iter_mut().enumerate() {
                if should_replace_with_zero(self.space, &self.channels, target, i) {
                    *ch = Some(0.0);
                }
            }
        }

        let mut result = Color {
            space: target,
            channels: new_channels,
            alpha: self.alpha,
            format: ColorFormat::Infer,
        };

        // In modern (non-legacy) spaces, set powerless channels to none.
        // E.g. hue is powerless when chroma/saturation is 0.
        if !target.is_legacy() {
            for i in 0..3 {
                if result.is_channel_powerless(i) && result.channels[i].is_some() {
                    result.channels[i] = None;
                }
            }
        }

        // For legacy target spaces, always use HSL format for HSL/HWB targets.
        // For RGB targets, use HSL only when out of gamut.
        if target == ColorSpace::Hsl || target == ColorSpace::Hwb {
            result.format = ColorFormat::Hsl;
        } else if target == ColorSpace::Rgb {
            // Check if underlying sRGB values are significantly out of gamut.
            let srgb = if target == ColorSpace::Rgb {
                let ch = result.raw_channels();
                [
                    ch[0].unwrap_or(0.0) / 255.0,
                    ch[1].unwrap_or(0.0) / 255.0,
                    ch[2].unwrap_or(0.0) / 255.0,
                ]
            } else {
                // HWB → sRGB
                let ch = result.raw_channels();
                conversion::hwb_to_srgb(
                    ch[0].unwrap_or(0.0),
                    ch[1].unwrap_or(0.0),
                    ch[2].unwrap_or(0.0),
                )
            };

            let out_of_gamut = srgb.iter().any(|v| *v < -0.0001 || *v > 1.0001);

            if out_of_gamut {
                // Convert to HSL and store in HSL space for hsl() format.
                let hsl = conversion::srgb_to_hsl(srgb[0], srgb[1], srgb[2]);
                result.space = ColorSpace::Hsl;
                result.channels = [Some(hsl[0]), Some(hsl[1]), Some(hsl[2])];
                result.format = ColorFormat::Hsl;
            }
        }

        result
    }

    /// Whether a channel is missing (None/`none`).
    pub fn has_missing_channel(&self, index: usize) -> bool {
        self.channels.get(index).is_some_and(|c| c.is_none())
    }

    /// Whether alpha is missing.
    pub fn has_missing_alpha(&self) -> bool {
        self.alpha.is_none()
    }

    /// Get a channel value, treating None as 0.
    pub fn channel_value(&self, index: usize) -> Number {
        Number(self.channels.get(index).and_then(|c| *c).unwrap_or(0.0))
    }

    /// Check if all channels are within the gamut bounds for this space.
    pub fn is_in_gamut(&self) -> bool {
        let channel_defs = self.space.channels();
        for (i, def) in channel_defs.iter().enumerate() {
            if let Some(val) = self.channels[i] {
                if !def.is_polar && (val < def.min || val > def.max) {
                    return false;
                }
            }
        }
        true
    }

    /// Check if a specific channel is "powerless" in this color.
    /// A channel is powerless when its value has no effect on the color's appearance.
    /// Examples: hue when saturation=0 in HSL, hue when chroma=0 in LCH/OKLch.
    pub fn is_channel_powerless(&self, index: usize) -> bool {
        let channel_defs = self.space.channels();
        if !channel_defs[index].is_polar {
            return false;
        }

        // Hue is powerless when the associated chroma/saturation is 0
        match self.space {
            ColorSpace::Hsl => {
                // hue is channel 0, powerless when saturation (channel 1) is 0
                index == 0 && fuzzy_is_zero(self.channels[1].unwrap_or(0.0))
            }
            ColorSpace::Hwb => {
                // hue is channel 0, powerless when whiteness + blackness >= 1
                index == 0 && {
                    let w = self.channels[1].unwrap_or(0.0);
                    let b = self.channels[2].unwrap_or(0.0);
                    w + b >= 1.0
                }
            }
            ColorSpace::Lch => {
                // hue is channel 2, powerless when chroma (channel 1) is 0
                index == 2 && fuzzy_is_zero(self.channels[1].unwrap_or(0.0))
            }
            ColorSpace::Oklch => {
                // hue is channel 2, powerless when chroma (channel 1) is 0
                index == 2 && fuzzy_is_zero(self.channels[1].unwrap_or(0.0))
            }
            _ => false,
        }
    }
}

// ---- Gamut mapping ----
impl Color {
    /// Clamp all channels to the gamut bounds of this color's space.
    pub fn to_gamut_clip(&self) -> Self {
        let channel_defs = self.space.channels();
        let mut channels = self.channels;
        for i in 0..3 {
            if let Some(val) = channels[i] {
                if !channel_defs[i].is_polar {
                    channels[i] = Some(val.clamp(channel_defs[i].min, channel_defs[i].max));
                }
            }
        }
        Color {
            space: self.space,
            channels,
            alpha: self.alpha,
            format: ColorFormat::Infer,
        }
    }

    /// CSS Color 4 local-MINDE gamut mapping algorithm.
    ///
    /// Ported from dart-sass `local_minde.dart`. Bisects on OKLch chroma using
    /// deltaEOK to find the closest perceptually-accurate in-gamut color.
    pub fn to_gamut_local_minde(&self) -> Self {
        if self.is_in_gamut() {
            return self.clone();
        }

        let origin = self.to_space(ColorSpace::Oklch);
        let l = origin.channels[0].unwrap_or(0.0);
        let h = origin.channels[2];
        let alpha = origin.alpha;

        const JND: f64 = 0.02;
        const EPSILON: f64 = 0.0001;

        if l >= 1.0 {
            let white = Color::for_space(
                self.space,
                self.space.white_channels(),
                self.alpha,
                ColorFormat::Infer,
            );
            return white.to_gamut_clip();
        }
        if l <= 0.0 {
            let black = Color::for_space(
                self.space,
                self.space.black_channels(),
                self.alpha,
                ColorFormat::Infer,
            );
            return black.to_gamut_clip();
        }

        // Early check: if clipping is close enough perceptually, just clip
        let mut clipped = self.to_gamut_clip();
        if delta_e_ok(&clipped, self) < JND {
            return clipped;
        }

        let mut min = 0.0_f64;
        let mut max = origin.channels[1].unwrap_or(0.0);
        let mut min_in_gamut = true;

        while max - min > EPSILON {
            let chroma = (min + max) / 2.0;
            let current = Color::for_space(
                ColorSpace::Oklch,
                [Some(l), Some(chroma), h],
                alpha,
                ColorFormat::Infer,
            )
            .to_space(self.space);

            if min_in_gamut && current.is_in_gamut() {
                min = chroma;
                continue;
            }

            clipped = current.to_gamut_clip();
            let e = delta_e_ok(&clipped, &current);

            if e < JND {
                if JND - e < EPSILON {
                    return clipped;
                }
                min_in_gamut = false;
                min = chroma;
            } else {
                max = chroma;
            }
        }
        clipped
    }
}

/// Calculate deltaEOK (Euclidean distance in OKLab space).
fn delta_e_ok(a: &Color, b: &Color) -> f64 {
    let a_oklab = a.to_space(ColorSpace::Oklab);
    let b_oklab = b.to_space(ColorSpace::Oklab);

    let dl = a_oklab.channels[0].unwrap_or(0.0) - b_oklab.channels[0].unwrap_or(0.0);
    let da = a_oklab.channels[1].unwrap_or(0.0) - b_oklab.channels[1].unwrap_or(0.0);
    let db = a_oklab.channels[2].unwrap_or(0.0) - b_oklab.channels[2].unwrap_or(0.0);

    (dl * dl + da * da + db * db).sqrt()
}

/// Channel analogy groups per CSS Color Level 4.
///
/// Two channels are "analogous" if they represent the same perceptual attribute.
/// When converting between spaces, a `none` (missing) value propagates from a
/// source channel to the analogous target channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelAnalogy {
    RedX,         // RGB red ↔ XYZ x (index 0 in both families)
    GreenY,       // RGB green ↔ XYZ y (index 1 in both families)
    BlueZ,        // RGB blue ↔ XYZ z (index 2 in both families)
    Hue,
    Lightness,
    Colorfulness, // chroma and saturation
    OpponentA,    // Lab a ↔ OKLab a
    OpponentB,    // Lab b ↔ OKLab b
}

/// Get the analogy group for channel `index` in `space`, if any.
///
/// Returns `None` for channels that have no analogous counterpart in other
/// spaces (e.g. HWB whiteness/blackness).
fn channel_analogy(space: ColorSpace, index: usize) -> Option<ChannelAnalogy> {
    use ChannelAnalogy::*;
    match space {
        // RGB-family: red=0, green=1, blue=2
        ColorSpace::Rgb
        | ColorSpace::SRgb
        | ColorSpace::SRgbLinear
        | ColorSpace::DisplayP3
        | ColorSpace::DisplayP3Linear
        | ColorSpace::A98Rgb
        | ColorSpace::ProphotoRgb
        | ColorSpace::Rec2020 => match index {
            0 => Some(RedX),
            1 => Some(GreenY),
            2 => Some(BlueZ),
            _ => None,
        },
        // HSL: hue=0, saturation=1, lightness=2
        ColorSpace::Hsl => match index {
            0 => Some(Hue),
            1 => Some(Colorfulness),
            2 => Some(Lightness),
            _ => None,
        },
        // HWB: hue=0, whiteness=1, blackness=2
        // whiteness and blackness have no analogous channels
        ColorSpace::Hwb => match index {
            0 => Some(Hue),
            _ => None,
        },
        // Lab: lightness=0, a=1, b=2
        ColorSpace::Lab => match index {
            0 => Some(Lightness),
            1 => Some(OpponentA),
            2 => Some(OpponentB),
            _ => None,
        },
        // LCH: lightness=0, chroma=1, hue=2
        ColorSpace::Lch => match index {
            0 => Some(Lightness),
            1 => Some(Colorfulness),
            2 => Some(Hue),
            _ => None,
        },
        // OKLab: lightness=0, a=1, b=2
        ColorSpace::Oklab => match index {
            0 => Some(Lightness),
            1 => Some(OpponentA),
            2 => Some(OpponentB),
            _ => None,
        },
        // OKLch: lightness=0, chroma=1, hue=2
        ColorSpace::Oklch => match index {
            0 => Some(Lightness),
            1 => Some(Colorfulness),
            2 => Some(Hue),
            _ => None,
        },
        // XYZ: x=0, y=1, z=2 — analogous to RGB red/green/blue
        ColorSpace::XyzD50 | ColorSpace::XyzD65 => match index {
            0 => Some(RedX),
            1 => Some(GreenY),
            2 => Some(BlueZ),
            _ => None,
        },
    }
}

/// Check whether any missing (None) channel in the source has an analogous
/// channel at `target_idx` in the target space.
///
/// Per dart-sass behavior, `none` is never propagated to legacy spaces
/// (RGB, HSL, HWB). This matches the Sass spec's treatment of legacy colors.
fn should_propagate_none(
    from: ColorSpace,
    from_channels: &[Option<f64>; 3],
    to: ColorSpace,
    to_idx: usize,
) -> bool {
    // Never propagate none to legacy color spaces
    if to.is_legacy() {
        return false;
    }

    let target_type = match channel_analogy(to, to_idx) {
        Some(t) => t,
        None => return false,
    };

    for (i, ch) in from_channels.iter().enumerate() {
        if ch.is_none() {
            if let Some(source_type) = channel_analogy(from, i) {
                if source_type == target_type {
                    return true;
                }
            }
        }
    }
    false
}

/// For legacy target spaces: when a source channel is `none` and has an
/// analogous channel in the target, replace the target channel with 0.
/// This is dart-sass's behavior — legacy spaces don't support `none` but
/// still respect the semantic of "missing" by using 0.
fn should_replace_with_zero(
    from: ColorSpace,
    from_channels: &[Option<f64>; 3],
    to: ColorSpace,
    to_idx: usize,
) -> bool {
    let target_type = match channel_analogy(to, to_idx) {
        Some(t) => t,
        None => return false,
    };

    for (i, ch) in from_channels.iter().enumerate() {
        if ch.is_none() {
            if let Some(source_type) = channel_analogy(from, i) {
                if source_type == target_type {
                    return true;
                }
            }
        }
    }
    false
}

fn fuzzy_is_zero(v: f64) -> bool {
    v.abs() < 1e-10
}

/// Adjust two hue values for the given interpolation method.
/// Returns (h1, h2) adjusted so that linear interpolation produces the correct arc.
fn adjust_hue(h1: f64, h2: f64, method: HueInterpolationMethod) -> (f64, f64) {
    let diff = h2 - h1;
    match method {
        HueInterpolationMethod::Shorter => {
            if diff > 180.0 {
                (h1 + 360.0, h2)
            } else if diff < -180.0 {
                (h1, h2 + 360.0)
            } else {
                (h1, h2)
            }
        }
        HueInterpolationMethod::Longer => {
            if diff > 0.0 && diff < 180.0 {
                (h1 + 360.0, h2)
            } else if diff > -180.0 && diff < 0.0 {
                (h1, h2 + 360.0)
            } else {
                (h1, h2)
            }
        }
        HueInterpolationMethod::Increasing => {
            if diff < 0.0 {
                (h1, h2 + 360.0)
            } else {
                (h1, h2)
            }
        }
        HueInterpolationMethod::Decreasing => {
            if diff > 0.0 {
                (h1 + 360.0, h2)
            } else {
                (h1, h2)
            }
        }
    }
}
