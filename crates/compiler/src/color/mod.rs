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
                Some(red.0.clamp(0.0, 255.0)),
                Some(green.0.clamp(0.0, 255.0)),
                Some(blue.0.clamp(0.0, 255.0)),
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
