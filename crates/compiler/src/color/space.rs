use std::fmt;

/// Hue interpolation method for polar color spaces (CSS Color 4 §12.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HueInterpolationMethod {
    /// Default: take the shorter arc.
    Shorter,
    /// Take the longer arc.
    Longer,
    /// Always go in the increasing (positive) direction.
    Increasing,
    /// Always go in the decreasing (negative) direction.
    Decreasing,
}

/// All CSS Color Level 4 color spaces supported by Sass.
///
/// Legacy spaces (RGB, HSL, HWB) use commas in their function syntax and
/// serialize differently from modern spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// Standard RGB (the default/legacy space). Channels: red [0,255], green [0,255], blue [0,255]
    Rgb,
    /// HSL (legacy). Channels: hue [0,360], saturation [0,100], lightness [0,100]
    Hsl,
    /// HWB (legacy). Channels: hue [0,360], whiteness [0,100], blackness [0,100]
    Hwb,
    /// sRGB with channels in [0,1]. Used in `color(srgb ...)`.
    SRgb,
    /// Linear-light sRGB. Channels in [0,1].
    SRgbLinear,
    /// Display P3 (wide gamut). Channels in [0,1].
    DisplayP3,
    /// Linear-light Display P3. Channels in [0,1].
    DisplayP3Linear,
    /// Adobe RGB 1998. Channels in [0,1].
    A98Rgb,
    /// ProPhoto RGB (ROMM RGB). Channels in [0,1].
    ProphotoRgb,
    /// ITU-R BT.2020. Channels in [0,1].
    Rec2020,
    /// CIE Lab. Channels: lightness [0,100], a [-125,125], b [-125,125]
    Lab,
    /// CIE LCH. Channels: lightness [0,100], chroma [0,150], hue [0,360]
    Lch,
    /// OKLab. Channels: lightness [0,1], a [-0.4,0.4], b [-0.4,0.4]
    Oklab,
    /// OKLch. Channels: lightness [0,1], chroma [0,0.4], hue [0,360]
    Oklch,
    /// CIE XYZ with D50 illuminant. Channels in [0,1].
    XyzD50,
    /// CIE XYZ with D65 illuminant. Channels in [0,1].
    XyzD65,
}

impl ColorSpace {
    /// Whether this is a legacy color space (RGB, HSL, HWB).
    /// Legacy spaces use comma-separated syntax and different serialization rules.
    pub fn is_legacy(self) -> bool {
        matches!(self, Self::Rgb | Self::Hsl | Self::Hwb)
    }

    /// Whether this space has a polar/hue channel (HSL, HWB, LCH, OKLch).
    pub fn is_polar(self) -> bool {
        matches!(self, Self::Hsl | Self::Hwb | Self::Lch | Self::Oklch)
    }

    /// Whether this space is a rectangular (non-polar) space.
    pub fn is_rectangular(self) -> bool {
        !self.is_polar()
    }

    /// Whether this space is an RGB-like space that uses the `color()` function.
    /// These are the "predefined RGB" spaces in the CSS spec.
    pub fn is_predefined_rgb(self) -> bool {
        matches!(
            self,
            Self::SRgb | Self::SRgbLinear | Self::DisplayP3 | Self::DisplayP3Linear | Self::A98Rgb | Self::ProphotoRgb | Self::Rec2020
        )
    }

    /// Whether this is a perceptual color space (Lab, LCH, OKLab, OKLch).
    /// Perceptual spaces can represent colors outside the visible gamut and
    /// require special serialization when out-of-range.
    pub fn is_perceptual(self) -> bool {
        matches!(self, Self::Lab | Self::Lch | Self::Oklab | Self::Oklch)
    }

    /// Whether this space uses XYZ coordinates.
    pub fn is_xyz(self) -> bool {
        matches!(self, Self::XyzD50 | Self::XyzD65)
    }

    /// Whether this color space is unbounded (all values are in gamut).
    /// Perceptual and XYZ spaces have no gamut limits.
    pub fn is_unbounded(self) -> bool {
        self.is_perceptual() || self.is_xyz()
    }

    /// Get channel definitions for this color space.
    pub fn channels(self) -> [ChannelDef; 3] {
        match self {
            Self::Rgb => [
                ChannelDef::new("red", 0.0, 255.0, false, Some(255.0)),
                ChannelDef::new("green", 0.0, 255.0, false, Some(255.0)),
                ChannelDef::new("blue", 0.0, 255.0, false, Some(255.0)),
            ],
            Self::Hsl => [
                ChannelDef::new("hue", 0.0, 360.0, true, None),
                // Internal storage is [0, 1], CSS display is [0%, 100%]
                ChannelDef::new("saturation", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("lightness", 0.0, 1.0, false, Some(1.0)),
            ],
            Self::Hwb => [
                ChannelDef::new("hue", 0.0, 360.0, true, None),
                // Internal storage is [0, 1], CSS display is [0%, 100%]
                ChannelDef::new("whiteness", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("blackness", 0.0, 1.0, false, Some(1.0)),
            ],
            Self::SRgb | Self::SRgbLinear | Self::DisplayP3 | Self::DisplayP3Linear | Self::A98Rgb | Self::ProphotoRgb | Self::Rec2020 => [
                ChannelDef::new("red", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("green", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("blue", 0.0, 1.0, false, Some(1.0)),
            ],
            Self::Lab => [
                ChannelDef::new("lightness", 0.0, 100.0, false, Some(100.0)),
                ChannelDef::new("a", -125.0, 125.0, false, Some(125.0)),
                ChannelDef::new("b", -125.0, 125.0, false, Some(125.0)),
            ],
            Self::Lch => [
                ChannelDef::new("lightness", 0.0, 100.0, false, Some(100.0)),
                ChannelDef::new("chroma", 0.0, 150.0, false, Some(150.0)),
                ChannelDef::new("hue", 0.0, 360.0, true, None),
            ],
            Self::Oklab => [
                ChannelDef::new("lightness", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("a", -0.4, 0.4, false, Some(0.4)),
                ChannelDef::new("b", -0.4, 0.4, false, Some(0.4)),
            ],
            Self::Oklch => [
                ChannelDef::new("lightness", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("chroma", 0.0, 0.4, false, Some(0.4)),
                ChannelDef::new("hue", 0.0, 360.0, true, None),
            ],
            Self::XyzD50 | Self::XyzD65 => [
                ChannelDef::new("x", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("y", 0.0, 1.0, false, Some(1.0)),
                ChannelDef::new("z", 0.0, 1.0, false, Some(1.0)),
            ],
        }
    }

    /// Get the name of this color space as used in CSS/Sass.
    pub fn name(self) -> &'static str {
        match self {
            Self::Rgb => "rgb",
            Self::Hsl => "hsl",
            Self::Hwb => "hwb",
            Self::SRgb => "srgb",
            Self::SRgbLinear => "srgb-linear",
            Self::DisplayP3 => "display-p3",
            Self::DisplayP3Linear => "display-p3-linear",
            Self::A98Rgb => "a98-rgb",
            Self::ProphotoRgb => "prophoto-rgb",
            Self::Rec2020 => "rec2020",
            Self::Lab => "lab",
            Self::Lch => "lch",
            Self::Oklab => "oklab",
            Self::Oklch => "oklch",
            Self::XyzD50 => "xyz-d50",
            Self::XyzD65 => "xyz",
        }
    }

    /// Parse a color space name from a CSS/Sass string.
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "rgb" => Some(Self::Rgb),
            "hsl" => Some(Self::Hsl),
            "hwb" => Some(Self::Hwb),
            "srgb" => Some(Self::SRgb),
            "srgb-linear" => Some(Self::SRgbLinear),
            "display-p3" => Some(Self::DisplayP3),
            "display-p3-linear" => Some(Self::DisplayP3Linear),
            "a98-rgb" => Some(Self::A98Rgb),
            "prophoto-rgb" => Some(Self::ProphotoRgb),
            "rec2020" => Some(Self::Rec2020),
            "lab" => Some(Self::Lab),
            "lch" => Some(Self::Lch),
            "oklab" => Some(Self::Oklab),
            "oklch" => Some(Self::Oklch),
            "xyz-d50" => Some(Self::XyzD50),
            "xyz" | "xyz-d65" => Some(Self::XyzD65),
            _ => None,
        }
    }

    /// Channel values representing white in this color space.
    pub fn white_channels(self) -> [Option<f64>; 3] {
        match self {
            Self::Rgb => [Some(255.0), Some(255.0), Some(255.0)],
            Self::Hsl => [Some(0.0), Some(0.0), Some(1.0)], // hue=0, sat=0, lightness=1.0 (100%)
            Self::Hwb => [Some(0.0), Some(1.0), Some(0.0)], // hue=0, whiteness=1.0, blackness=0
            Self::SRgb | Self::DisplayP3 | Self::DisplayP3Linear | Self::A98Rgb | Self::ProphotoRgb | Self::Rec2020 => {
                [Some(1.0), Some(1.0), Some(1.0)]
            }
            Self::SRgbLinear => [Some(1.0), Some(1.0), Some(1.0)],
            Self::Lab => [Some(100.0), Some(0.0), Some(0.0)],
            Self::Lch => [Some(100.0), Some(0.0), Some(0.0)],
            Self::Oklab => [Some(1.0), Some(0.0), Some(0.0)],
            Self::Oklch => [Some(1.0), Some(0.0), Some(0.0)],
            Self::XyzD50 => [Some(0.9642), Some(1.0), Some(0.8252)],
            Self::XyzD65 => [Some(0.9505), Some(1.0), Some(1.0890)],
        }
    }

    /// Channel values representing black in this color space.
    pub fn black_channels(self) -> [Option<f64>; 3] {
        match self {
            Self::Rgb => [Some(0.0), Some(0.0), Some(0.0)],
            Self::Hsl => [Some(0.0), Some(0.0), Some(0.0)],
            Self::Hwb => [Some(0.0), Some(0.0), Some(1.0)],
            Self::SRgb | Self::SRgbLinear | Self::DisplayP3 | Self::DisplayP3Linear | Self::A98Rgb
            | Self::ProphotoRgb | Self::Rec2020 => [Some(0.0), Some(0.0), Some(0.0)],
            Self::Lab => [Some(0.0), Some(0.0), Some(0.0)],
            Self::Lch => [Some(0.0), Some(0.0), Some(0.0)],
            Self::Oklab => [Some(0.0), Some(0.0), Some(0.0)],
            Self::Oklch => [Some(0.0), Some(0.0), Some(0.0)],
            Self::XyzD50 | Self::XyzD65 => [Some(0.0), Some(0.0), Some(0.0)],
        }
    }

    /// The index of the hue channel in this space, if any.
    pub fn hue_channel_index(self) -> Option<usize> {
        match self {
            Self::Hsl | Self::Hwb => Some(0),
            Self::Lch | Self::Oklch => Some(2),
            _ => None,
        }
    }
}

impl fmt::Display for ColorSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Metadata for a single color channel.
#[derive(Debug, Clone, Copy)]
pub struct ChannelDef {
    /// Channel name (e.g. "red", "hue", "lightness")
    pub name: &'static str,
    /// Minimum value (for gamut checking)
    pub min: f64,
    /// Maximum value (for gamut checking)
    pub max: f64,
    /// Whether this is a polar/hue channel (wraps at 360)
    pub is_polar: bool,
    /// The reference value for percentage conversion (100% = this value).
    /// None for hue channels where percentages aren't meaningful.
    pub percentage_ref: Option<f64>,
}

impl ChannelDef {
    const fn new(name: &'static str, min: f64, max: f64, is_polar: bool, percentage_ref: Option<f64>) -> Self {
        Self {
            name,
            min,
            max,
            is_polar,
            percentage_ref,
        }
    }
}
