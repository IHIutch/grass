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
pub(crate) use space::ColorSpace;

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
    fn to_rgb_channels(&self) -> [f64; 3] {
        let c0 = self.channels[0].unwrap_or(0.0);
        let c1 = self.channels[1].unwrap_or(0.0);
        let c2 = self.channels[2].unwrap_or(0.0);

        match self.space {
            ColorSpace::Rgb => [c0, c1, c2],
            ColorSpace::Hsl => {
                let srgb = conversion::hsl_to_srgb(c0, c1, c2);
                [
                    fuzzy_round(srgb[0] * 255.0),
                    fuzzy_round(srgb[1] * 255.0),
                    fuzzy_round(srgb[2] * 255.0),
                ]
            }
            ColorSpace::Hwb => {
                let srgb = conversion::hwb_to_srgb(c0, c1, c2);
                [
                    fuzzy_round(srgb[0] * 255.0),
                    fuzzy_round(srgb[1] * 255.0),
                    fuzzy_round(srgb[2] * 255.0),
                ]
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
            alpha: Some(alpha.0.clamp(0.0, 1.0)),
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
            alpha: Some(alpha.0.clamp(0.0, 1.0)),
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
                Some(saturation.0.clamp(0.0, 1.0)),
                Some(lightness.0.clamp(0.0, 1.0)),
            ],
            alpha: Some(alpha.0.clamp(0.0, 1.0)),
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
        // Convert HWB to RGB immediately (legacy behavior)
        let h = hue.rem_euclid(360.0);
        let w = white.0 / 100.0;
        let b = black.0 / 100.0;

        let srgb = conversion::hwb_to_srgb(h, w, b);
        Color {
            space: ColorSpace::Rgb,
            channels: [
                Some(fuzzy_round(srgb[0] * 255.0)),
                Some(fuzzy_round(srgb[1] * 255.0)),
                Some(fuzzy_round(srgb[2] * 255.0)),
            ],
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

    pub fn adjust_hue(&self, degrees: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        Color::from_hsla(hue + degrees, saturation, luminance, alpha)
    }

    pub fn lighten(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        Color::from_hsla(hue, saturation, luminance + amount, alpha)
    }

    pub fn darken(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        Color::from_hsla(hue, saturation, luminance - amount, alpha)
    }

    pub fn saturate(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        Color::from_hsla(hue, (saturation + amount).clamp(0.0, 1.0), luminance, alpha)
    }

    pub fn desaturate(&self, amount: Number) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        Color::from_hsla(hue, (saturation - amount).clamp(0.0, 1.0), luminance, alpha)
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

    pub fn complement(&self) -> Self {
        let (hue, saturation, luminance, alpha) = self.as_hsla();
        Color::from_hsla(hue + Number(180.0), saturation, luminance, alpha)
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
            return self.clone();
        }

        let c0 = self.channels[0].unwrap_or(0.0);
        let c1 = self.channels[1].unwrap_or(0.0);
        let c2 = self.channels[2].unwrap_or(0.0);

        let converted = conversion::convert([c0, c1, c2], self.space, target);

        // Propagate missing channels: if a channel was None in source,
        // the corresponding channel in target is also None (CSS Color 4 spec)
        let new_channels = [
            if self.channels[0].is_none() && has_analogous_channel(self.space, 0, target, 0) {
                None
            } else {
                Some(converted[0])
            },
            if self.channels[1].is_none() && has_analogous_channel(self.space, 1, target, 1) {
                None
            } else {
                Some(converted[1])
            },
            if self.channels[2].is_none() && has_analogous_channel(self.space, 2, target, 2) {
                None
            } else {
                Some(converted[2])
            },
        ];

        Color {
            space: target,
            channels: new_channels,
            alpha: self.alpha,
            format: ColorFormat::Infer,
        }
    }

    /// Whether a channel is missing (None/`none`).
    pub fn has_missing_channel(&self, index: usize) -> bool {
        self.channels.get(index).map_or(false, |c| c.is_none())
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
        for i in 0..3 {
            if let Some(val) = self.channels[i] {
                if !channel_defs[i].is_polar && (val < channel_defs[i].min || val > channel_defs[i].max) {
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
    /// This maps an out-of-gamut color to the closest in-gamut color using
    /// perceptual uniformity in OKLch. It bisects on chroma while checking
    /// deltaEOK to ensure perceptual accuracy.
    pub fn to_gamut_local_minde(&self) -> Self {
        // If already in gamut, return as-is
        if self.is_in_gamut() {
            return self.clone();
        }

        let origin = self.to_space(ColorSpace::Oklch);
        let l = origin.channels[0].unwrap_or(0.0);
        let c = origin.channels[1].unwrap_or(0.0);
        let h = origin.channels[2].unwrap_or(0.0);

        // Handle edge cases: if lightness is at or beyond bounds
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

        const EPSILON: f64 = 0.02;
        const DELTA_E_THRESHOLD: f64 = 0.02;

        let mut min_chroma = 0.0_f64;
        let mut max_chroma = c;
        let mut current_chroma;

        // Bisect on chroma
        loop {
            current_chroma = (min_chroma + max_chroma) / 2.0;

            // Create color with reduced chroma in OKLch
            let candidate_oklch = Color::for_space(
                ColorSpace::Oklch,
                [Some(l), Some(current_chroma), Some(h)],
                self.alpha,
                ColorFormat::Infer,
            );

            // Convert to target space
            let candidate = candidate_oklch.to_space(self.space);

            // Check if in gamut
            if candidate.is_in_gamut() {
                // Check if we're close enough to the boundary
                if max_chroma - min_chroma < EPSILON {
                    return candidate;
                }
                // Try higher chroma
                min_chroma = current_chroma;
            } else {
                // Clip the candidate
                let clipped = candidate.to_gamut_clip();

                // Calculate deltaEOK between clipped and candidate
                let delta = delta_e_ok(&clipped, &candidate);

                if delta - DELTA_E_THRESHOLD < EPSILON {
                    if max_chroma - min_chroma < EPSILON {
                        return clipped;
                    }
                    // Clipped is close enough perceptually, try higher chroma
                    min_chroma = current_chroma;
                } else {
                    // Too much perceptual difference, reduce chroma
                    max_chroma = current_chroma;
                }
            }

            // Safety: if chroma range is tiny, return clipped
            if max_chroma - min_chroma < EPSILON / 100.0 {
                let final_oklch = Color::for_space(
                    ColorSpace::Oklch,
                    [Some(l), Some(current_chroma), Some(h)],
                    self.alpha,
                    ColorFormat::Infer,
                );
                return final_oklch.to_space(self.space).to_gamut_clip();
            }
        }
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

/// Check if a source channel has an analogous channel in the target space.
/// For simplicity, same-index channels are considered analogous for now.
fn has_analogous_channel(_from: ColorSpace, _from_idx: usize, _to: ColorSpace, _to_idx: usize) -> bool {
    // TODO: implement proper analogous channel mapping per CSS Color 4 spec
    true
}

fn fuzzy_is_zero(v: f64) -> bool {
    v.abs() < 1e-10
}
