use std::io::Write;

use codemap::{CodeMap, Span};

use crate::{
    ast::{CssStmt, MediaQuery, SassMixin, Style, SupportsRule},
    color::{Color, ColorFormat, ColorSpace, NAMED_COLORS},
    common::{BinaryOp, Brackets, ListSeparator, QuoteKind},
    error::SassResult,
    selector::{
        Combinator, ComplexSelector, ComplexSelectorComponent, CompoundSelector, Namespace, Pseudo,
        SelectorList, SimpleSelector,
    },
    unit::Unit,
    utils::hex_char_for,
    value::{
        fuzzy_equals, ArgList, CalculationArg, CalculationName, SassCalculation, SassFunction,
        SassMap, SassNumber, Value,
    },
    Options,
};

pub(crate) fn serialize_selector_list(
    list: &SelectorList,
    options: &Options,
    span: Span,
) -> String {
    let map = CodeMap::new();
    let mut serializer = Serializer::new(options, &map, false, span);

    serializer.write_selector_list(list);

    serializer.finish_for_expr()
}

pub(crate) fn serialize_calculation_arg(
    arg: &CalculationArg,
    options: &Options,
    span: Span,
) -> SassResult<String> {
    let map = CodeMap::new();
    let mut serializer = Serializer::new(options, &map, false, span);

    serializer.write_calculation_arg(arg)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) fn serialize_number(
    number: &SassNumber,
    options: &Options,
    span: Span,
) -> SassResult<String> {
    let map = CodeMap::new();
    let mut serializer = Serializer::new(options, &map, false, span);

    serializer.visit_number(number)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) fn serialize_value(val: &Value, options: &Options, span: Span) -> SassResult<String> {
    let map = CodeMap::new();
    let mut serializer = Serializer::new(options, &map, false, span);

    serializer.visit_value(val, span)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) fn inspect_value(val: &Value, options: &Options, span: Span) -> SassResult<String> {
    let map = CodeMap::new();
    let mut serializer = Serializer::new(options, &map, true, span);

    serializer.visit_value(val, span)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) fn inspect_float(number: f64, options: &Options, span: Span) -> String {
    let map = CodeMap::new();
    let mut serializer = Serializer::new(options, &map, true, span);

    serializer.write_float(number);

    serializer.finish_for_expr()
}

pub(crate) fn inspect_map(map: &SassMap, options: &Options, span: Span) -> SassResult<String> {
    let code_map = CodeMap::new();
    let mut serializer = Serializer::new(options, &code_map, true, span);

    serializer.visit_map(map, span)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) fn inspect_function_ref(
    func: &SassFunction,
    options: &Options,
    span: Span,
) -> SassResult<String> {
    let code_map = CodeMap::new();
    let mut serializer = Serializer::new(options, &code_map, true, span);

    serializer.visit_function_ref(func, span)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) fn inspect_mixin_ref(
    mixin: &SassMixin,
    options: &Options,
    span: Span,
) -> SassResult<String> {
    let code_map = CodeMap::new();
    let mut serializer = Serializer::new(options, &code_map, true, span);

    serializer.visit_mixin_ref(mixin, span)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) fn inspect_number(
    number: &SassNumber,
    options: &Options,
    span: Span,
) -> SassResult<String> {
    let map = CodeMap::new();
    let mut serializer = Serializer::new(options, &map, true, span);

    serializer.visit_number(number)?;

    Ok(serializer.finish_for_expr())
}

pub(crate) struct Serializer<'a> {
    indentation: usize,
    options: &'a Options<'a>,
    inspect: bool,
    indent_width: usize,
    // todo: use this field
    _quote: bool,
    buffer: Vec<u8>,
    map: &'a CodeMap,
    _span: Span,
    in_calculation: bool,
    in_custom_property: bool,
}

impl<'a> Serializer<'a> {
    pub fn new(options: &'a Options<'a>, map: &'a CodeMap, inspect: bool, span: Span) -> Self {
        Self {
            inspect,
            _quote: true,
            indentation: 0,
            indent_width: 2,
            options,
            buffer: Vec::new(),
            map,
            _span: span,
            in_calculation: false,
            in_custom_property: false,
        }
    }

    pub fn with_capacity(options: &'a Options<'a>, map: &'a CodeMap, inspect: bool, span: Span, capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            ..Self::new(options, map, inspect, span)
        }
    }

    fn omit_spaces_around_complex_component(&self, component: &ComplexSelectorComponent) -> bool {
        self.options.is_compressed()
            && matches!(component, ComplexSelectorComponent::Combinator(..))
    }

    fn write_pseudo_selector(&mut self, pseudo: &Pseudo) {
        if let Some(sel) = &pseudo.selector {
            if pseudo.name == "not" && sel.is_invisible() {
                return;
            }
        }

        self.buffer.push(b':');

        if !pseudo.is_syntactic_class {
            self.buffer.push(b':');
        }

        self.buffer.extend_from_slice(pseudo.name.as_bytes());

        if pseudo.argument.is_none() && pseudo.selector.is_none() {
            return;
        }

        self.buffer.push(b'(');
        if let Some(arg) = &pseudo.argument {
            self.buffer.extend_from_slice(arg.as_bytes());
            if pseudo.selector.is_some() {
                self.buffer.push(b' ');
            }
        }

        if let Some(sel) = &pseudo.selector {
            self.write_selector_list(sel);
        }

        self.buffer.push(b')');
    }

    fn write_namespace(&mut self, namespace: &Namespace) {
        match namespace {
            Namespace::Empty => self.buffer.push(b'|'),
            Namespace::Asterisk => self.buffer.extend_from_slice(b"*|"),
            Namespace::Other(namespace) => {
                self.buffer.extend_from_slice(namespace.as_bytes());
                self.buffer.push(b'|');
            }
            Namespace::None => {}
        }
    }

    fn write_simple_selector(&mut self, simple: &SimpleSelector) {
        match simple {
            SimpleSelector::Id(name) => {
                self.buffer.push(b'#');
                self.buffer.extend_from_slice(name.as_bytes());
            }
            SimpleSelector::Class(name) => {
                self.buffer.push(b'.');
                self.buffer.extend_from_slice(name.as_bytes());
            }
            SimpleSelector::Placeholder(name) => {
                self.buffer.push(b'%');
                self.buffer.extend_from_slice(name.as_bytes());
            }
            SimpleSelector::Universal(namespace) => {
                self.write_namespace(namespace);
                self.buffer.push(b'*');
            }
            SimpleSelector::Pseudo(pseudo) => self.write_pseudo_selector(pseudo),
            SimpleSelector::Type(name) => {
                self.write_namespace(&name.namespace);
                self.buffer.extend_from_slice(name.ident.as_bytes());
            }
            SimpleSelector::Attribute(attr) => write!(&mut self.buffer, "{}", attr).unwrap(),
            SimpleSelector::Parent(suffix) => {
                self.buffer.push(b'&');
                if let Some(s) = suffix {
                    self.buffer.extend_from_slice(s.as_bytes());
                }
            }
        }
    }

    fn write_compound_selector(&mut self, compound: &CompoundSelector) {
        let mut did_write = false;
        for simple in &compound.components {
            if did_write {
                self.write_simple_selector(simple);
            } else {
                let len = self.buffer.len();
                self.write_simple_selector(simple);
                if self.buffer.len() != len {
                    did_write = true;
                }
            }
        }

        // If we emit an empty compound, it's because all of the components got
        // optimized out because they match all selectors, so we just emit the
        // universal selector.
        if !did_write {
            self.buffer.push(b'*');
        }
    }

    fn write_complex_selector_component(&mut self, component: &ComplexSelectorComponent) {
        match component {
            ComplexSelectorComponent::Combinator(Combinator::NextSibling) => self.buffer.push(b'+'),
            ComplexSelectorComponent::Combinator(Combinator::Child) => self.buffer.push(b'>'),
            ComplexSelectorComponent::Combinator(Combinator::FollowingSibling) => {
                self.buffer.push(b'~')
            }
            ComplexSelectorComponent::Compound(compound) => self.write_compound_selector(compound),
        }
    }

    fn write_complex_selector(&mut self, complex: &ComplexSelector) {
        let mut last_component = None;

        for component in &complex.components {
            if let Some(c) = last_component {
                if !self.omit_spaces_around_complex_component(c)
                    && !self.omit_spaces_around_complex_component(component)
                {
                    self.buffer.push(b' ');
                }
            }
            self.write_complex_selector_component(component);
            last_component = Some(component);
        }
    }

    fn write_selector_list(&mut self, list: &SelectorList) {
        self.write_selector_list_filtered(list, false);
    }

    /// Write a top-level selector list, filtering out bogus selectors
    fn write_top_level_selector_list(&mut self, list: &SelectorList) {
        self.write_selector_list_filtered(list, true);
    }

    fn write_selector_list_filtered(&mut self, list: &SelectorList, filter_bogus: bool) {
        let complexes: Vec<_> = list
            .components
            .iter()
            .filter(|c| {
                !c.is_invisible() && (!filter_bogus || !c.is_bogus(false))
            })
            .collect();

        let mut first = true;

        for complex in complexes {
            if first {
                first = false;
            } else {
                self.buffer.push(b',');
                if complex.line_break {
                    self.write_newline();
                    self.write_indentation();
                } else {
                    self.write_optional_space();
                }
            }
            self.write_complex_selector(complex);
        }
    }

    fn write_newline(&mut self) {
        if !self.options.is_compressed() {
            self.buffer.push(b'\n');
        }
    }

    fn write_comma_separator(&mut self) {
        self.buffer.push(b',');
        self.write_optional_space();
    }

    fn write_calculation_name(&mut self, name: CalculationName) {
        let s = match name {
            CalculationName::Calc => b"calc" as &[u8],
            CalculationName::Min => b"min",
            CalculationName::Max => b"max",
            CalculationName::Clamp => b"clamp",
            CalculationName::Abs => b"abs",
            CalculationName::Acos => b"acos",
            CalculationName::Asin => b"asin",
            CalculationName::Atan => b"atan",
            CalculationName::Atan2 => b"atan2",
            CalculationName::Cos => b"cos",
            CalculationName::Exp => b"exp",
            CalculationName::Hypot => b"hypot",
            CalculationName::Log => b"log",
            CalculationName::Mod => b"mod",
            CalculationName::Pow => b"pow",
            CalculationName::Rem => b"rem",
            CalculationName::Round => b"round",
            CalculationName::Sign => b"sign",
            CalculationName::Sin => b"sin",
            CalculationName::Sqrt => b"sqrt",
            CalculationName::Tan => b"tan",
            CalculationName::CalcSize => b"calc-size",
        };
        self.buffer.extend_from_slice(s);
    }

    fn visit_calculation(&mut self, calculation: &SassCalculation) -> SassResult<()> {
        self.write_calculation_name(calculation.name);
        self.buffer.push(b'(');

        let was_in_calculation = self.in_calculation;
        self.in_calculation = true;

        if let Some((last, slice)) = calculation.args.split_last() {
            for arg in slice {
                self.write_calculation_arg(arg)?;
                self.write_comma_separator();
            }

            self.write_calculation_arg(last)?;
        }

        self.in_calculation = was_in_calculation;
        self.buffer.push(b')');

        Ok(())
    }

    fn write_calculation_arg(&mut self, arg: &CalculationArg) -> SassResult<()> {
        match arg {
            CalculationArg::Number(num) => self.visit_number(num)?,
            CalculationArg::Calculation(calc) => {
                self.visit_calculation(calc)?;
            }
            CalculationArg::String(s) | CalculationArg::Interpolation(s) => {
                self.buffer.extend_from_slice(s.as_bytes());
            }
            CalculationArg::Operation { lhs, op, rhs } => {
                let paren_left = match &**lhs {
                    CalculationArg::Operation { op: op2, .. } => op2.precedence() < op.precedence(),
                    _ => false,
                };

                if paren_left {
                    self.buffer.push(b'(');
                }

                self.write_calculation_arg(lhs)?;

                if paren_left {
                    self.buffer.push(b')');
                }

                let operator_whitespace =
                    !self.options.is_compressed() || matches!(op, BinaryOp::Plus | BinaryOp::Minus);

                if operator_whitespace {
                    self.buffer.push(b' ');
                }

                // todo: avoid allocation with `write_binary_operator` method
                self.buffer.extend_from_slice(op.to_string().as_bytes());

                if operator_whitespace {
                    self.buffer.push(b' ');
                }

                let paren_right = match &**rhs {
                    CalculationArg::Operation { op: op2, .. } => {
                        CalculationArg::parenthesize_calculation_rhs(*op, *op2)
                    }
                    // Degenerate numbers (NaN/infinity) with units serialize as
                    // multi-token expressions (e.g. "infinity * 1px"), which need
                    // parens to preserve precedence in division context
                    CalculationArg::Number(n)
                        if *op == BinaryOp::Div
                            && (n.num.0.is_infinite() || n.num.0.is_nan())
                            && n.unit != Unit::None =>
                    {
                        true
                    }
                    _ => false,
                };

                if paren_right {
                    self.buffer.push(b'(');
                }

                self.write_calculation_arg(rhs)?;

                if paren_right {
                    self.buffer.push(b')');
                }
            }
        }

        Ok(())
    }

    /// Write a potentially degenerate (NaN/Infinity) value in legacy color syntax.
    /// Wraps NaN/Infinity in calc() wrappers per CSS spec.
    /// `has_percent`: if true, uses `calc(NaN * 1%)` instead of `calc(NaN)`.
    fn write_legacy_degenerate_or_float(&mut self, val: f64, has_percent: bool) {
        if val.is_nan() {
            if has_percent {
                self.buffer.extend_from_slice(b"calc(NaN * 1%)");
            } else {
                self.buffer.extend_from_slice(b"calc(NaN)");
            }
        } else if val.is_infinite() {
            let sign = if val.is_sign_negative() { "-" } else { "" };
            if has_percent {
                write!(&mut self.buffer, "calc({}infinity * 1%)", sign).unwrap();
            } else {
                write!(&mut self.buffer, "calc({}infinity)", sign).unwrap();
            }
        } else {
            self.write_float(val);
            if has_percent {
                self.buffer.push(b'%');
            }
        }
    }

    fn write_rgb(&mut self, color: &Color) {
        let is_opaque = fuzzy_equals(color.alpha().0, 1.0);

        if is_opaque {
            self.buffer.extend_from_slice(b"rgb(");
        } else {
            self.buffer.extend_from_slice(b"rgba(");
        }

        self.write_float(color.red().0);
        self.buffer.extend_from_slice(b",");
        self.write_optional_space();
        self.write_float(color.green().0);
        self.buffer.extend_from_slice(b",");
        self.write_optional_space();
        self.write_float(color.blue().0);

        if !is_opaque {
            self.buffer.extend_from_slice(b",");
            self.write_optional_space();
            self.write_float(color.alpha().0);
        }

        self.buffer.push(b')');
    }

    /// Write RGB color with fractional channel values (from space conversions).
    fn write_rgb_fractional(&mut self, color: &Color, rgb: &[f64; 3]) {
        let is_opaque = fuzzy_equals(color.alpha().0, 1.0);

        if is_opaque {
            self.buffer.extend_from_slice(b"rgb(");
        } else {
            self.buffer.extend_from_slice(b"rgba(");
        }

        self.write_float(rgb[0]);
        self.buffer.extend_from_slice(b", ");
        self.write_float(rgb[1]);
        self.buffer.extend_from_slice(b", ");
        self.write_float(rgb[2]);

        if !is_opaque {
            self.buffer.extend_from_slice(b", ");
            self.write_float(color.alpha().0);
        }

        self.buffer.push(b')');
    }

    fn write_hsl(&mut self, color: &Color) {
        let is_opaque = fuzzy_equals(color.alpha().0, 1.0);

        if is_opaque {
            self.buffer.extend_from_slice(b"hsl(");
        } else {
            self.buffer.extend_from_slice(b"hsla(");
        }

        // For HSL-stored colors, read raw channels to preserve out-of-gamut values.
        // For HWB-stored colors, convert through sRGB to get HSL values.
        let (hue, sat, light) = if color.color_space() == ColorSpace::Hsl {
            let raw = color.raw_channels();
            (
                raw[0].unwrap_or(0.0),
                raw[1].unwrap_or(0.0) * 100.0,
                raw[2].unwrap_or(0.0) * 100.0,
            )
        } else if color.color_space() == ColorSpace::Hwb {
            let raw = color.raw_channels();
            let srgb = crate::color::conversion::hwb_to_srgb(
                raw[0].unwrap_or(0.0),
                raw[1].unwrap_or(0.0),
                raw[2].unwrap_or(0.0),
            );
            let hsl = crate::color::conversion::srgb_to_hsl(srgb[0], srgb[1], srgb[2]);
            (hsl[0], hsl[1] * 100.0, hsl[2] * 100.0)
        } else {
            (color.hue().0, color.saturation().0, color.lightness().0)
        };

        self.write_legacy_degenerate_or_float(hue, false);
        self.buffer.extend_from_slice(b", ");
        self.write_legacy_degenerate_or_float(sat, true);
        self.buffer.extend_from_slice(b", ");
        self.write_legacy_degenerate_or_float(light, true);

        if !is_opaque {
            self.buffer.extend_from_slice(b", ");
            let alpha = color.alpha().0;
            // NaN alpha clamps to 0 in legacy colors
            self.write_float(if alpha.is_nan() { 0.0 } else { alpha });
        }

        self.buffer.push(b')');
    }

    /// Serialize a legacy color (RGB, HSL, HWB) that has missing (none) channels.
    /// Uses modern space-separated syntax: `hsl(none 100% 50%)`, `rgb(none 100 200)`.
    fn write_legacy_with_none(&mut self, color: &Color) {
        let space = color.color_space();
        let is_opaque = fuzzy_equals(color.alpha().0, 1.0);
        let has_missing_alpha = color.has_missing_alpha();

        // Write function name
        self.buffer.extend_from_slice(space.name().as_bytes());
        self.buffer.push(b'(');

        // Write channels with none support
        for i in 0..3 {
            if i > 0 {
                self.buffer.push(b' ');
            }
            if color.has_missing_channel(i) {
                self.buffer.extend_from_slice(b"none");
            } else {
                let val = color.channel_value(i).0;
                match space {
                    ColorSpace::Rgb => {
                        self.write_float(val);
                    }
                    ColorSpace::Hsl => {
                        if i == 0 {
                            // hue
                            self.write_float(val);
                            self.buffer.extend_from_slice(b"deg");
                        } else {
                            // saturation, lightness (stored as [0,1], display as %)
                            self.write_float(val * 100.0);
                            self.buffer.push(b'%');
                        }
                    }
                    ColorSpace::Hwb => {
                        if i == 0 {
                            // hue
                            self.write_float(val);
                            self.buffer.extend_from_slice(b"deg");
                        } else {
                            // whiteness, blackness (stored as [0,1], display as %)
                            self.write_float(val * 100.0);
                            self.buffer.push(b'%');
                        }
                    }
                    _ => {
                        self.write_float(val);
                    }
                }
            }
        }

        // Alpha
        if !is_opaque || has_missing_alpha {
            self.buffer.extend_from_slice(b" / ");
            if has_missing_alpha {
                self.buffer.extend_from_slice(b"none");
            } else {
                self.write_float(color.alpha().0);
            }
        }

        self.buffer.push(b')');
    }

    fn write_hex_component(&mut self, channel: u32) {
        debug_assert!(channel < 256);

        self.buffer.push(hex_char_for(channel >> 4) as u8);
        self.buffer.push(hex_char_for(channel & 0xF) as u8);
    }

    fn is_symmetrical_hex(channel: u32) -> bool {
        channel & 0xF == channel >> 4
    }

    fn can_use_short_hex(color: &Color) -> bool {
        Self::is_symmetrical_hex(color.red().0.round() as u32)
            && Self::is_symmetrical_hex(color.green().0.round() as u32)
            && Self::is_symmetrical_hex(color.blue().0.round() as u32)
    }

    pub fn visit_color(&mut self, color: &Color) {
        // Modern (non-legacy) color spaces get their own serialization
        if !color.color_space().is_legacy() {
            self.write_modern_color(color);
            return;
        }

        // Legacy colors with missing channels use modern space-separated syntax
        let has_missing = color.has_missing_channel(0) || color.has_missing_channel(1)
            || color.has_missing_channel(2) || color.has_missing_alpha();
        if has_missing {
            self.write_legacy_with_none(color);
            return;
        }

        // Check for degenerate (NaN/Infinity) channel values in HSL/HWB colors.
        // These must be serialized via write_hsl to get calc() wrappers.
        if matches!(color.color_space(), ColorSpace::Hsl | ColorSpace::Hwb) {
            let raw = color.raw_channels();
            let has_degenerate = raw.iter().any(|ch| {
                ch.is_some_and(|v| !v.is_finite())
            });
            if has_degenerate {
                self.write_hsl(color);
                return;
            }
        }

        // Check raw RGB channels for out-of-gamut or fractional values.
        {
            let rgb = color.to_rgb_channels_raw();
            let out_of_gamut = rgb.iter().any(|v| *v < -0.0001 || *v > 255.0001);
            if out_of_gamut {
                self.write_hsl(color);
                return;
            }
            let has_fractional = rgb.iter().any(|v| {
                let rounded = v.round();
                (v - rounded).abs() > 1e-10
            });
            if has_fractional {
                match color.color_space() {
                    // HWB always serializes fractional RGB as hsl()
                    ColorSpace::Hwb => self.write_hsl(color),
                    // HSL with explicit Hsl format (from hsl() literal) → hsl()
                    // HSL with Infer format (from adjust/change) → rgb() with fractional values
                    ColorSpace::Hsl if color.format == ColorFormat::Hsl => self.write_hsl(color),
                    _ => self.write_rgb_fractional(color, &rgb),
                }
                return;
            }
            // HWB-stored colors with non-opaque alpha always serialize as hsla()
            if color.color_space() == ColorSpace::Hwb && !fuzzy_equals(color.alpha().0, 1.0) {
                self.write_hsl(color);
                return;
            }
        }

        let red = color.red().0.round() as u8;
        let green = color.green().0.round() as u8;
        let blue = color.blue().0.round() as u8;

        let name = if fuzzy_equals(color.alpha().0, 1.0) {
            NAMED_COLORS.get_by_rgba([red, green, blue])
        } else {
            None
        };

        #[allow(clippy::unnecessary_unwrap)]
        if self.options.is_compressed() {
            if fuzzy_equals(color.alpha().0, 1.0) {
                let hex_length = if Self::can_use_short_hex(color) { 4 } else { 7 };
                if name.is_some() && name.unwrap().len() <= hex_length {
                    self.buffer.extend_from_slice(name.unwrap().as_bytes());
                } else if Self::can_use_short_hex(color) {
                    self.buffer.push(b'#');
                    self.buffer.push(hex_char_for(red as u32 & 0xF) as u8);
                    self.buffer.push(hex_char_for(green as u32 & 0xF) as u8);
                    self.buffer.push(hex_char_for(blue as u32 & 0xF) as u8);
                } else {
                    self.buffer.push(b'#');
                    self.write_hex_component(red as u32);
                    self.write_hex_component(green as u32);
                    self.write_hex_component(blue as u32);
                }
            } else {
                self.write_rgb(color);
            }
        } else if color.format != ColorFormat::Infer {
            match &color.format {
                ColorFormat::Rgb => self.write_rgb(color),
                ColorFormat::Hsl => {
                    // For HWB-stored colors from to-space(), serialize as hex
                    // (like any other legacy color) rather than hsl().
                    // HSL-stored colors always use hsl() format (matching dart-sass).
                    if color.color_space() == ColorSpace::Hwb && fuzzy_equals(color.alpha().0, 1.0) {
                        if let Some(name) = name {
                            self.buffer.extend_from_slice(name.as_bytes());
                        } else {
                            self.buffer.push(b'#');
                            self.write_hex_component(red as u32);
                            self.write_hex_component(green as u32);
                            self.write_hex_component(blue as u32);
                        }
                    } else {
                        self.write_hsl(color);
                    }
                }
                ColorFormat::Literal(text) => self.buffer.extend_from_slice(text.as_bytes()),
                ColorFormat::Infer => unreachable!(),
            }
            // Always emit generated transparent colors in rgba format. This works
            // around an IE bug. See sass/sass#1782.
        } else if name.is_some() && !fuzzy_equals(color.alpha().0, 0.0) {
            self.buffer.extend_from_slice(name.unwrap().as_bytes());
        } else if fuzzy_equals(color.alpha().0, 1.0) {
            self.buffer.push(b'#');
            self.write_hex_component(red as u32);
            self.write_hex_component(green as u32);
            self.write_hex_component(blue as u32);
        } else {
            self.write_rgb(color);
        }
    }

    /// Serialize a color in a modern (non-legacy) color space.
    fn write_modern_color(&mut self, color: &Color) {
        let space = color.color_space();
        let alpha = color.alpha();
        let is_opaque = fuzzy_equals(alpha.0, 1.0);
        let has_missing_alpha = color.has_missing_alpha();

        // Out-of-range perceptual colors serialize as color-mix() when
        // lightness (channel 0) is out of range, unless they have missing channels.
        // Other channels (a/b, chroma) are unbounded and don't trigger fallback.
        if space.is_perceptual() {
            let has_any_missing = color.has_missing_channel(0)
                || color.has_missing_channel(1)
                || color.has_missing_channel(2)
                || color.has_missing_alpha();

            if !has_any_missing {
                let channel_defs = space.channels();
                let lightness = color.channel_value(0).0;
                // Use fuzzy comparison: lightness < min or lightness > max
                // but not when it's within epsilon of the boundary.
                let epsilon = 1e-11;
                let lightness_oor = lightness < channel_defs[0].min - epsilon
                    || lightness > channel_defs[0].max + epsilon;
                if lightness_oor {
                    self.write_color_mix_fallback(color);
                    return;
                }
            }
        }

        match space {
            // Lab-family: lab(L a b) / lch(L C H)
            ColorSpace::Lab | ColorSpace::Lch | ColorSpace::Oklab | ColorSpace::Oklch => {
                self.buffer.extend_from_slice(space.name().as_bytes());
                self.buffer.push(b'(');
                self.write_channel(color, 0);
                self.buffer.push(b' ');
                self.write_channel(color, 1);
                self.buffer.push(b' ');
                self.write_channel(color, 2);
                if !is_opaque || has_missing_alpha {
                    self.buffer.extend_from_slice(b" / ");
                    if has_missing_alpha {
                        self.buffer.extend_from_slice(b"none");
                    } else {
                        self.write_float(alpha.0);
                    }
                }
                self.buffer.push(b')');
            }
            // Predefined RGB spaces + XYZ: color(space r g b)
            _ => {
                self.buffer.extend_from_slice(b"color(");
                self.buffer.extend_from_slice(space.name().as_bytes());
                self.buffer.push(b' ');
                self.write_channel(color, 0);
                self.buffer.push(b' ');
                self.write_channel(color, 1);
                self.buffer.push(b' ');
                self.write_channel(color, 2);
                if !is_opaque || has_missing_alpha {
                    self.buffer.extend_from_slice(b" / ");
                    if has_missing_alpha {
                        self.buffer.extend_from_slice(b"none");
                    } else {
                        self.write_float(alpha.0);
                    }
                }
                self.buffer.push(b')');
            }
        }
    }

    /// Serialize an out-of-range perceptual color as `color-mix(in <space>, color(xyz ...) 100%, black)`.
    fn write_color_mix_fallback(&mut self, color: &Color) {
        let space = color.color_space();
        let alpha = color.alpha();
        let is_opaque = fuzzy_equals(alpha.0, 1.0);

        let xyz = color.to_space(ColorSpace::XyzD65);

        self.buffer.extend_from_slice(b"color-mix(in ");
        self.buffer.extend_from_slice(space.name().as_bytes());
        if self.options.is_compressed() {
            self.buffer.extend_from_slice(b",color(xyz ");
        } else {
            self.buffer.extend_from_slice(b", color(xyz ");
        }
        self.write_float(xyz.channel_value(0).0);
        self.buffer.push(b' ');
        self.write_float(xyz.channel_value(1).0);
        self.buffer.push(b' ');
        self.write_float(xyz.channel_value(2).0);
        if !is_opaque {
            self.buffer.extend_from_slice(b" / ");
            self.write_float(alpha.0);
        }
        if self.options.is_compressed() {
            self.buffer.extend_from_slice(b")100%,red)");
        } else {
            self.buffer.extend_from_slice(b") 100%, black)");
        }
    }

    /// Write a single channel value, or "none" if missing.
    /// Adds appropriate units: `%` for lightness, `deg` for hue.
    fn write_channel(&mut self, color: &Color, index: usize) {
        if color.has_missing_channel(index) {
            self.buffer.extend_from_slice(b"none");
        } else {
            let space = color.color_space();
            let channel_defs = space.channels();
            let val = color.channel_value(index).0;

            // Handle NaN and infinity with calc() wrapper
            if val.is_nan() {
                if channel_defs[index].is_polar {
                    self.buffer
                        .extend_from_slice(b"calc(NaN * 1deg)");
                } else if channel_defs[index].name == "lightness" {
                    self.buffer
                        .extend_from_slice(b"calc(NaN * 1%)");
                } else {
                    self.buffer.extend_from_slice(b"calc(NaN)");
                }
                return;
            }
            if val.is_infinite() {
                let sign = if val.is_sign_negative() { "-" } else { "" };
                if channel_defs[index].is_polar {
                    write!(&mut self.buffer, "calc({}infinity * 1deg)", sign)
                        .unwrap();
                } else if channel_defs[index].name == "lightness" {
                    write!(&mut self.buffer, "calc({}infinity * 1%)", sign)
                        .unwrap();
                } else {
                    write!(&mut self.buffer, "calc({}infinity)", sign).unwrap();
                }
                return;
            }

            // Lightness channels in perceptual spaces serialize with %
            if channel_defs[index].name == "lightness" {
                // OKLab/OKLCh: lightness is in [0, 1], serialize as percentage
                if matches!(space, ColorSpace::Oklab | ColorSpace::Oklch) {
                    self.write_float(val * 100.0);
                } else {
                    // Lab/LCH: lightness is already in [0, 100]
                    self.write_float(val);
                }
                self.buffer.push(b'%');
            } else if channel_defs[index].is_polar {
                // Hue channels serialize with deg
                self.write_float(val);
                self.buffer.extend_from_slice(b"deg");
            } else {
                self.write_float(val);
            }
        }
    }

    fn write_media_query(&mut self, query: &MediaQuery) {
        if let Some(modifier) = &query.modifier {
            self.buffer.extend_from_slice(modifier.as_bytes());
            self.buffer.push(b' ');
        }

        if let Some(media_type) = &query.media_type {
            self.buffer.extend_from_slice(media_type.as_bytes());

            if !query.conditions.is_empty() {
                self.buffer.extend_from_slice(b" and ");
            }
        }

        if query.conditions.len() == 1 && query.conditions.first().unwrap().starts_with("(not ") {
            self.buffer.extend_from_slice(b"not ");
            let condition = query.conditions.first().unwrap();
            self.buffer
                .extend_from_slice(&condition.as_bytes()["(not ".len()..condition.len() - 1]);
        } else {
            let operator = if query.conjunction { " and " } else { " or " };
            self.buffer
                .extend_from_slice(query.conditions.join(operator).as_bytes());
        }
    }

    /// Write the unit suffix for complex units in calc() expressions.
    /// For numerators [a, b] and denominators [c, d], writes:
    ///   ` * 1a * 1b / 1c / 1d`
    fn write_complex_unit_suffix(&mut self, numer: &[Unit], denom: &[Unit]) {
        for unit in numer {
            let _ = write!(&mut self.buffer, " * 1{}", unit);
        }
        for unit in denom {
            let _ = write!(&mut self.buffer, " / 1{}", unit);
        }
    }

    pub fn visit_number(&mut self, number: &SassNumber) -> SassResult<()> {
        if let Some(as_slash) = &number.as_slash {
            self.visit_number(&as_slash.0)?;
            self.buffer.push(b'/');
            self.visit_number(&as_slash.1)?;
            return Ok(());
        }

        let is_complex = number.unit.is_complex();

        {
            let f = number.num.0;
            if f.is_nan() {
                let (numer, denom) = number.unit.clone().numer_and_denom();
                if self.in_calculation {
                    self.buffer.extend_from_slice(b"NaN");
                    self.write_complex_unit_suffix(&numer, &denom);
                } else {
                    self.buffer.extend_from_slice(b"calc(NaN");
                    self.write_complex_unit_suffix(&numer, &denom);
                    self.buffer.push(b')');
                }
                return Ok(());
            }
            if f.is_infinite() {
                let sign = if f.is_sign_negative() { "-" } else { "" };
                let (numer, denom) = number.unit.clone().numer_and_denom();
                if self.in_calculation {
                    write!(&mut self.buffer, "{}infinity", sign)?;
                    self.write_complex_unit_suffix(&numer, &denom);
                } else {
                    write!(&mut self.buffer, "calc({}infinity", sign)?;
                    self.write_complex_unit_suffix(&numer, &denom);
                    self.buffer.push(b')');
                }
                return Ok(());
            }
        }

        if !self.inspect && is_complex {
            // Wrap finite complex-unit numbers in calc()
            let (numer, denom) = number.unit.clone().numer_and_denom();
            if self.in_calculation {
                self.write_float(number.num.0);
                if let Some(first) = numer.first() {
                    write!(&mut self.buffer, "{}", first)?;
                }
                for unit in numer.iter().skip(1) {
                    write!(&mut self.buffer, " * 1{}", unit)?;
                }
                for unit in &denom {
                    write!(&mut self.buffer, " / 1{}", unit)?;
                }
            } else {
                self.buffer.extend_from_slice(b"calc(");
                self.write_float(number.num.0);
                if let Some(first) = numer.first() {
                    write!(&mut self.buffer, "{}", first)?;
                }
                for unit in numer.iter().skip(1) {
                    write!(&mut self.buffer, " * 1{}", unit)?;
                }
                for unit in &denom {
                    write!(&mut self.buffer, " / 1{}", unit)?;
                }
                self.buffer.push(b')');
            }
            return Ok(());
        }

        self.write_float(number.num.0);
        write!(&mut self.buffer, "{}", number.unit)?;

        Ok(())
    }

    fn write_float(&mut self, float: f64) {
        if float.is_infinite() && float.is_sign_negative() {
            self.buffer.extend_from_slice(b"-Infinity");
            return;
        } else if float.is_infinite() {
            self.buffer.extend_from_slice(b"Infinity");
            return;
        }

        let start = self.buffer.len();

        if float < 0.0 {
            self.buffer.push(b'-');
        }

        let num = float.abs();

        // For very large numbers that exceed f64's integer precision,
        // format via scientific notation to avoid precision artifacts.
        // f64 has ~15-17 significant decimal digits; beyond that, the
        // decimal representation includes spurious non-zero digits.
        // This matches dart-sass behavior where 1e100 outputs as 1 followed
        // by 100 zeros.
        if num >= 1e15 && num.fract() == 0.0 {
            let s = format!("{:e}", num);
            if let Some(e_pos) = s.find('e') {
                let mantissa = &s[..e_pos];
                let exp: usize = s[e_pos + 1..].parse().unwrap_or(0);
                let digits: String = mantissa
                    .replace('.', "")
                    .trim_end_matches('0')
                    .to_string();
                let num_digits = digits.len();
                if exp + 1 > num_digits {
                    self.buffer.extend_from_slice(digits.as_bytes());
                    for _ in 0..(exp + 1 - num_digits) {
                        self.buffer.push(b'0');
                    }
                } else {
                    self.buffer.extend_from_slice(&digits.as_bytes()[..exp + 1]);
                }
            } else {
                let formatted = format!("{:.10}", num);
                let trimmed = formatted
                    .trim_end_matches('0')
                    .trim_end_matches('.');
                self.buffer.extend_from_slice(trimmed.as_bytes());
            }
        } else {
            // Use ryu for fast shortest-representation formatting, falling
            // back to {:.10} when ryu uses scientific notation or exceeds
            // 10 decimal places.
            let mut ryu_buf = ryu::Buffer::new();
            let short = ryu_buf.format(num);

            let fixed;
            let trimmed = if short.contains('e') || short.contains('E') {
                // ryu used scientific notation — CSS needs decimal form
                fixed = format!("{:.10}", num);
                fixed.trim_end_matches('0').trim_end_matches('.')
            } else if let Some(dot_pos) = short.find('.') {
                let short_decimals = short.len() - dot_pos - 1;
                if short_decimals <= 10 {
                    short.trim_end_matches('0').trim_end_matches('.')
                } else {
                    fixed = format!("{:.10}", num);
                    fixed.trim_end_matches('0').trim_end_matches('.')
                }
            } else {
                // No decimal point — integer
                short
            };

            if self.options.is_compressed() && num < 1.0 && trimmed.starts_with('0') {
                self.buffer.extend_from_slice(&trimmed.as_bytes()[1..]);
            } else {
                self.buffer.extend_from_slice(trimmed.as_bytes());
            }
        }

        // Check if we only wrote a sign or "-0"
        let written = &self.buffer[start..];
        if written.is_empty() || written == b"-" || written == b"-0" {
            self.buffer.truncate(start);
            self.buffer.push(b'0');
        }
    }

    pub fn visit_group(
        &mut self,
        stmt: CssStmt,
        prev_was_group_end: bool,
        prev_requires_semicolon: bool,
        had_previous_visible: bool,
    ) -> SassResult<()> {
        if prev_requires_semicolon {
            self.buffer.push(b';');
        }

        if !self.buffer.is_empty() || had_previous_visible {
            self.write_optional_newline();
        }

        if prev_was_group_end && !self.buffer.is_empty() {
            self.write_optional_newline();
        }

        self.visit_stmt(stmt)?;

        Ok(())
    }

    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    fn finish_for_expr(self) -> String {
        // SAFETY: todo
        unsafe { String::from_utf8_unchecked(self.buffer) }
    }

    pub fn finish(mut self, prev_requires_semicolon: bool) -> String {
        let is_not_ascii = self.buffer.iter().any(|&c| !c.is_ascii());

        if prev_requires_semicolon {
            self.buffer.push(b';');
        }

        if !self.buffer.is_empty() {
            self.write_optional_newline();
        }

        // SAFETY: todo
        let mut as_string = unsafe { String::from_utf8_unchecked(self.buffer) };

        if is_not_ascii && self.options.is_compressed() && self.options.allows_charset {
            as_string.insert(0, '\u{FEFF}');
        } else if is_not_ascii && self.options.allows_charset {
            as_string.insert_str(0, "@charset \"UTF-8\";\n");
        }

        as_string
    }

    fn write_indentation(&mut self) {
        if self.options.is_compressed() {
            return;
        }

        // Use a static buffer for common indentation depths to avoid per-byte pushes
        const SPACES: [u8; 32] = [b' '; 32];
        let n = self.indentation;
        if n <= 32 {
            self.buffer.extend_from_slice(&SPACES[..n]);
        } else {
            self.buffer.resize(self.buffer.len() + n, b' ');
        }
    }

    fn write_list_separator(&mut self, sep: ListSeparator) {
        match (sep, self.options.is_compressed()) {
            (ListSeparator::Space | ListSeparator::Undecided, _) => self.buffer.push(b' '),
            (ListSeparator::Comma, true) => self.buffer.push(b','),
            (ListSeparator::Comma, false) => self.buffer.extend_from_slice(b", "),
            (ListSeparator::Slash, true) => self.buffer.push(b'/'),
            (ListSeparator::Slash, false) => self.buffer.extend_from_slice(b" / "),
        }
    }

    fn elem_needs_parens(sep: ListSeparator, elem: &Value) -> bool {
        match elem {
            Value::List(elems, sep2, brackets) => {
                if elems.len() < 2 {
                    return false;
                }

                if *brackets == Brackets::Bracketed {
                    return false;
                }

                match sep {
                    ListSeparator::Comma => *sep2 == ListSeparator::Comma,
                    ListSeparator::Slash => {
                        *sep2 == ListSeparator::Comma || *sep2 == ListSeparator::Slash
                    }
                    _ => *sep2 != ListSeparator::Undecided,
                }
            }
            // ArgLists are always comma-separated, so they need parens in the
            // same contexts as comma-separated lists
            Value::ArgList(arglist) => {
                if arglist.elems.len() < 2 {
                    return false;
                }
                matches!(
                    sep,
                    ListSeparator::Comma | ListSeparator::Slash | ListSeparator::Space
                )
            }
            _ => false,
        }
    }

    fn visit_list(
        &mut self,
        list_elems: &[Value],
        sep: ListSeparator,
        brackets: Brackets,
        span: Span,
    ) -> SassResult<()> {
        if brackets == Brackets::Bracketed {
            self.buffer.push(b'[');
        } else if list_elems.is_empty() {
            if !self.inspect {
                return Err(("() isn't a valid CSS value.", span).into());
            }

            self.buffer.extend_from_slice(b"()");
            return Ok(());
        }

        let is_singleton = self.inspect
            && list_elems.len() == 1
            && (sep == ListSeparator::Comma || sep == ListSeparator::Slash);

        if is_singleton && brackets != Brackets::Bracketed {
            self.buffer.push(b'(');
        }

        let (mut x, mut y);
        let elems: &mut dyn Iterator<Item = &Value> = if self.inspect {
            x = list_elems.iter();
            &mut x
        } else {
            y = list_elems.iter().filter(|elem| !elem.is_blank());
            &mut y
        };

        let mut elems = elems.peekable();

        while let Some(elem) = elems.next() {
            if self.inspect {
                let needs_parens = Self::elem_needs_parens(sep, elem);
                if needs_parens {
                    self.buffer.push(b'(');
                }

                self.visit_value(elem, span)?;

                if needs_parens {
                    self.buffer.push(b')');
                }
            } else {
                self.visit_value(elem, span)?;
            }

            if elems.peek().is_some() {
                self.write_list_separator(sep);
            }
        }

        if is_singleton {
            match sep {
                ListSeparator::Comma => self.buffer.push(b','),
                ListSeparator::Slash => self.buffer.push(b'/'),
                _ => unreachable!(),
            }

            if brackets != Brackets::Bracketed {
                self.buffer.push(b')');
            }
        }

        if brackets == Brackets::Bracketed {
            self.buffer.push(b']');
        }

        Ok(())
    }

    fn write_map_element(&mut self, value: &Value, span: Span) -> SassResult<()> {
        let needs_parens = matches!(value, Value::List(_, ListSeparator::Comma, Brackets::None))
            || matches!(value, Value::ArgList(..));

        if needs_parens {
            self.buffer.push(b'(');
        }

        self.visit_value(value, span)?;

        if needs_parens {
            self.buffer.push(b')');
        }

        Ok(())
    }

    fn visit_map(&mut self, map: &SassMap, span: Span) -> SassResult<()> {
        if !self.inspect {
            return Err((
                format!(
                    "{} isn't a valid CSS value.",
                    inspect_map(map, self.options, span)?
                ),
                span,
            )
                .into());
        }

        self.buffer.push(b'(');

        let mut elems = map.iter().peekable();

        while let Some((k, v)) = elems.next() {
            self.write_map_element(&k.node, k.span)?;
            self.buffer.extend_from_slice(b": ");
            self.write_map_element(v, k.span)?;
            if elems.peek().is_some() {
                self.buffer.extend_from_slice(b", ");
            }
        }

        self.buffer.push(b')');

        Ok(())
    }

    fn visit_unquoted_string(&mut self, string: &str) {
        let mut after_newline = false;
        self.buffer.reserve(string.len());

        for c in string.chars() {
            match c {
                '\n' => {
                    if self.in_custom_property {
                        self.buffer.push(b'\n');
                    } else {
                        self.buffer.push(b' ');
                    }
                    after_newline = true;
                }
                ' ' => {
                    if !after_newline || self.in_custom_property {
                        self.buffer.push(b' ');
                    }
                }
                _ if is_private_use(c) => {
                    self.buffer.push(b'\\');
                    let hex = format!("{:x}", c as u32);
                    self.buffer.extend_from_slice(hex.as_bytes());
                    after_newline = false;
                }
                _ => {
                    let mut buf = [0u8; 4];
                    self.buffer
                        .extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    after_newline = false;
                }
            }
        }
    }

    fn visit_quoted_string(&mut self, force_double_quote: bool, string: &str) {
        let mut has_single_quote = false;
        let mut has_double_quote = false;

        let mut buffer = Vec::new();

        if force_double_quote {
            buffer.push(b'"');
        }
        let mut chars = string.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    if force_double_quote {
                        buffer.push(b'\'');
                    } else if has_double_quote {
                        self.visit_quoted_string(true, string);
                        return;
                    } else {
                        has_single_quote = true;
                        buffer.push(b'\'');
                    }
                }
                '"' => {
                    if force_double_quote {
                        buffer.push(b'\\');
                        buffer.push(b'"');
                    } else if has_single_quote {
                        self.visit_quoted_string(true, string);
                        return;
                    } else {
                        has_double_quote = true;
                        buffer.push(b'"');
                    }
                }
                '\x00'..='\x08' | '\x0A'..='\x1F' | '\x7F' => {
                    write_hex_escape(&mut buffer, c as u32, chars.peek().copied());
                }
                '\\' => {
                    buffer.push(b'\\');
                    buffer.push(b'\\');
                }
                _ if is_private_use(c) => {
                    write_hex_escape(&mut buffer, c as u32, chars.peek().copied());
                }
                _ => {
                    let mut buf = [0u8; 4];
                    buffer.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
        }

        if force_double_quote {
            buffer.push(b'"');
            self.buffer.extend_from_slice(&buffer);
        } else {
            let quote = if has_double_quote { b'\'' } else { b'"' };
            self.buffer.push(quote);
            self.buffer.extend_from_slice(&buffer);
            self.buffer.push(quote);
        }
    }

    fn visit_function_ref(&mut self, func: &SassFunction, span: Span) -> SassResult<()> {
        if !self.inspect {
            return Err((
                format!(
                    "{} isn't a valid CSS value.",
                    inspect_function_ref(func, self.options, span)?
                ),
                span,
            )
                .into());
        }

        self.buffer.extend_from_slice(b"get-function(");
        self.visit_quoted_string(false, func.name().as_str());
        self.buffer.push(b')');

        Ok(())
    }

    fn visit_mixin_ref(&mut self, mixin: &SassMixin, span: Span) -> SassResult<()> {
        if !self.inspect {
            return Err((
                format!(
                    "{} isn't a valid CSS value.",
                    inspect_mixin_ref(mixin, self.options, span)?
                ),
                span,
            )
                .into());
        }

        self.buffer.extend_from_slice(b"get-mixin(");
        self.visit_quoted_string(false, mixin.name().as_str());
        self.buffer.push(b')');

        Ok(())
    }

    fn visit_arglist(&mut self, arglist: &ArgList, span: Span) -> SassResult<()> {
        self.visit_list(&arglist.elems, arglist.separator, Brackets::None, span)
    }

    fn visit_value(&mut self, value: &Value, span: Span) -> SassResult<()> {
        match value {
            Value::Dimension(num) => self.visit_number(num)?,
            Value::Color(color) => self.visit_color(color),
            Value::Calculation(calc) => self.visit_calculation(calc)?,
            Value::List(elems, sep, brackets) => self.visit_list(elems.as_slice(), *sep, *brackets, span)?,
            Value::True => self.buffer.extend_from_slice(b"true"),
            Value::False => self.buffer.extend_from_slice(b"false"),
            Value::Null => {
                if self.inspect {
                    self.buffer.extend_from_slice(b"null")
                }
            }
            Value::Map(map) => self.visit_map(map, span)?,
            Value::FunctionRef(func) => self.visit_function_ref(func, span)?,
            Value::MixinRef(mixin) => self.visit_mixin_ref(mixin, span)?,
            Value::String(s, QuoteKind::Quoted) => self.visit_quoted_string(false, s),
            Value::String(s, QuoteKind::None) => self.visit_unquoted_string(s),
            Value::ArgList(arglist) => self.visit_arglist(arglist, span)?,
        }

        Ok(())
    }

    fn write_style(&mut self, style: Style) -> SassResult<()> {
        if !self.options.is_compressed() {
            self.write_indentation();
        }

        self.buffer
            .extend_from_slice(style.property.resolve_ref().as_bytes());
        self.buffer.push(b':');

        if !style.declared_as_custom_property && !self.options.is_compressed() {
            self.buffer.push(b' ');
        }

        if style.declared_as_custom_property {
            let start = self.buffer.len();
            self.in_custom_property = true;
            self.visit_value(&style.value.node, style.value.span)?;
            self.in_custom_property = false;
            let name_col = self
                .map
                .look_up_pos(style.property_span.low())
                .position
                .column;
            self.reindent_buffer_from(start, name_col);
        } else {
            self.visit_value(&style.value.node, style.value.span)?;
        }

        Ok(())
    }

    /// Re-indent continuation lines in the buffer starting from `start` position.
    /// Matches dart-sass `_writeReindentedValue` / `_writeWithIndent` algorithm.
    fn reindent_buffer_from(&mut self, start: usize, name_col: usize) {
        let value_bytes = self.buffer[start..].to_vec();
        let value_str = String::from_utf8_lossy(&value_bytes);

        // Use _minimumIndentation logic: scan past first \n, find min indent of rest
        let first_newline = match value_str.find('\n') {
            Some(pos) => pos,
            None => return, // No newlines, nothing to reindent
        };

        // Check if everything after the first newline is just whitespace
        let after_first = &value_str[first_newline + 1..];
        let has_non_whitespace_continuation = after_first
            .lines()
            .any(|line| !line.trim().is_empty());

        if !has_non_whitespace_continuation {
            // dart-sass: -1 case — trimAsciiRight + space
            let trimmed = value_str.trim_end();
            self.buffer.truncate(start);
            self.buffer.extend_from_slice(trimmed.as_bytes());
            self.buffer.push(b' ');
            return;
        }

        // Find minimum indentation of non-empty continuation lines
        let min_indent = after_first
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);

        // dart-sass: min(minimumIndentation, node.name.span.start.column)
        let strip_amount = std::cmp::min(min_indent, name_col);

        // Now rewrite: first line as-is, then process continuation lines
        self.buffer.truncate(start);
        self.buffer
            .extend_from_slice(value_str[..first_newline].as_bytes());

        // Process the rest using a scanner-like approach matching dart-sass _writeWithIndent
        let rest = &value_str[first_newline + 1..];
        let mut pos = 0;
        let rest_bytes = rest.as_bytes();

        while pos < rest_bytes.len() {
            // Scan forward past whitespace/newlines to find the next non-empty line start
            let scan_start = pos;
            let mut newlines = 1; // We already consumed one \n
            let mut line_start = pos;

            loop {
                if pos >= rest_bytes.len() {
                    // End of text while in whitespace — write trailing space
                    self.buffer.push(b' ');
                    return;
                }
                match rest_bytes[pos] {
                    b' ' | b'\t' => {
                        pos += 1;
                    }
                    b'\n' => {
                        pos += 1;
                        line_start = pos;
                        newlines += 1;
                    }
                    _ => break,
                }
            }

            // Write newlines
            for _ in 0..newlines {
                self.buffer.push(b'\n');
            }

            // Write indentation + content after stripping
            let line_indent = pos - line_start;
            let actual_strip = std::cmp::min(strip_amount, line_indent);
            self.write_indentation();
            // Write from line_start + actual_strip to end of line
            let content_start = line_start + actual_strip;
            while pos < rest_bytes.len() && rest_bytes[pos] != b'\n' {
                pos += 1;
            }
            self.buffer
                .extend_from_slice(&rest_bytes[content_start..pos]);

            // Skip the newline
            if pos < rest_bytes.len() {
                pos += 1;
                // If the value ended with a newline (no more content),
                // add trailing space so serialized `;` becomes `} ;`
                if pos >= rest_bytes.len() {
                    self.buffer.push(b' ');
                    return;
                }
            } else {
                return;
            }
        }
    }

    fn write_import(&mut self, import: &str, modifiers: Option<String>) -> SassResult<()> {
        self.write_indentation();
        self.buffer.extend_from_slice(b"@import ");
        write!(&mut self.buffer, "{}", import)?;

        if let Some(modifiers) = modifiers {
            self.buffer.push(b' ');
            self.buffer.extend_from_slice(modifiers.as_bytes());
        }

        Ok(())
    }

    fn write_comment(&mut self, comment: &str, span: Span) -> SassResult<()> {
        if self.options.is_compressed() && !comment.starts_with("/*!") {
            return Ok(());
        }

        // Strip source map comments per CSS spec
        let trimmed = comment.trim_start_matches("/*").trim_start();
        if trimmed.starts_with("# sourceMappingURL=") || trimmed.starts_with("# sourceURL=") {
            return Ok(());
        }

        self.write_indentation();
        let col = self.map.look_up_pos(span.low()).position.column;
        let mut lines = comment.lines();

        if let Some(line) = lines.next() {
            self.buffer.extend_from_slice(line.trim_start().as_bytes());
        }

        let continuation: Vec<&str> = lines.collect();

        if !continuation.is_empty() {
            // Find minimum indentation of continuation lines
            let min_indent = continuation
                .iter()
                .filter(|line| !line.trim_start().is_empty())
                .map(|line| line.len() - line.trim_start().len())
                .min()
                .unwrap_or(0);

            // Use the smaller of source column and min indentation as the base
            let base = std::cmp::min(col, min_indent);

            for line in &continuation {
                let leading = line.len() - line.trim_start().len();
                let relative = leading.saturating_sub(base);
                let output_indent = self.indentation + relative;
                write!(
                    &mut self.buffer,
                    "\n{}{}",
                    " ".repeat(output_indent),
                    line.trim_start()
                )?;
            }
        }

        Ok(())
    }

    pub fn requires_semicolon(stmt: &CssStmt) -> bool {
        match stmt {
            CssStmt::Style(_) | CssStmt::Import(_, _) => true,
            CssStmt::UnknownAtRule(rule, _) => !rule.has_body,
            _ => false,
        }
    }

    /// Get the source line number for a span position
    fn source_line(&self, pos: codemap::Pos) -> usize {
        self.map.look_up_pos(pos).position.line
    }

    /// Write a comment inline (after a semicolon or opening brace) without indentation
    pub(crate) fn write_inline_comment(&mut self, comment: &str, span: Span) -> SassResult<()> {
        if self.options.is_compressed() && !comment.starts_with("/*!") {
            return Ok(());
        }

        // Strip source map comments per CSS spec
        let trimmed = comment.trim_start_matches("/*").trim_start();
        if trimmed.starts_with("# sourceMappingURL=") || trimmed.starts_with("# sourceURL=") {
            return Ok(());
        }

        self.buffer.push(b' ');
        // For inline comments, write on the same line without indentation
        let col = self.map.look_up_pos(span.low()).position.column;
        let mut lines = comment.lines();

        if let Some(line) = lines.next() {
            self.buffer.extend_from_slice(line.trim_start().as_bytes());
        }

        let continuation: Vec<&str> = lines.collect();

        if !continuation.is_empty() {
            let min_indent = continuation
                .iter()
                .filter(|line| !line.trim_start().is_empty())
                .map(|line| line.len() - line.trim_start().len())
                .min()
                .unwrap_or(0);

            let base = std::cmp::min(col, min_indent);

            for line in &continuation {
                let leading = line.len() - line.trim_start().len();
                let relative = leading.saturating_sub(base);
                let output_indent = self.indentation + relative;
                write!(
                    &mut self.buffer,
                    "\n{}{}",
                    " ".repeat(output_indent),
                    line.trim_start()
                )?;
            }
        }

        Ok(())
    }

    /// Get the end source line of a statement (for same-line comment detection)
    fn stmt_end_line(&self, stmt: &CssStmt) -> Option<usize> {
        match stmt {
            CssStmt::Style(style) => Some(self.source_line(style.value.span.high())),
            CssStmt::Comment(_, span) => Some(self.source_line(span.high())),
            _ => None,
        }
    }

    /// Get the source line of the closing `}` for block-level statements
    pub(crate) fn stmt_closing_brace_line(&self, stmt: &CssStmt) -> Option<usize> {
        match stmt {
            CssStmt::RuleSet { source_span: Some(span), .. } => {
                Some(self.source_line(span.high()))
            }
            _ => None,
        }
    }

    /// Get the start source line of a comment statement
    pub(crate) fn comment_start_line(&self, stmt: &CssStmt) -> Option<usize> {
        match stmt {
            CssStmt::Comment(_, span) => Some(self.source_line(span.low())),
            _ => None,
        }
    }

    fn write_children(
        &mut self,
        children: Vec<CssStmt>,
        opening_brace_line: Option<usize>,
    ) -> SassResult<()> {
        use std::collections::VecDeque;

        if self.options.is_compressed() {
            self.buffer.push(b'{');
        } else {
            self.buffer.extend_from_slice(b" {");
        }

        self.indentation += self.indent_width;

        let mut children: VecDeque<CssStmt> = children.into();

        // Sub-problem B: Check if first visible child is an inline comment
        // on the same source line as the opening `{`
        if !self.options.is_compressed() {
            if let Some(brace_line) = opening_brace_line {
                // Find first visible child
                let first_visible = children.iter().position(|c| !c.is_invisible());
                if let Some(idx) = first_visible {
                    if let Some(comment_line) = self.comment_start_line(&children[idx]) {
                        if comment_line == brace_line {
                            if let CssStmt::Comment(ref comment, span) = children[idx] {
                                let comment = comment.clone();
                                self.write_inline_comment(&comment, span)?;
                                children.remove(idx);
                            }
                        }
                    }
                }
            }
        }

        if !self.options.is_compressed() {
            self.buffer.push(b'\n');
        }

        while let Some(child) = children.pop_front() {
            let needs_semicolon = Self::requires_semicolon(&child);
            let end_line = self.stmt_end_line(&child);
            let closing_brace_line = self.stmt_closing_brace_line(&child);
            let is_last = children.is_empty();
            let did_write = self.visit_stmt(child)?;

            if !did_write {
                continue;
            }

            if needs_semicolon {
                if is_last && self.options.is_compressed() {
                    // skip trailing semicolon in compressed mode
                } else {
                    self.buffer.push(b';');
                }
            }

            if !self.options.is_compressed() {
                // Sub-problem A: If we just wrote a Style and the next visible child
                // is a Comment on the same source line, write it inline
                if let Some(style_end_line) = end_line {
                    if needs_semicolon {
                        let next_visible = children.iter().position(|c| !c.is_invisible());
                        if let Some(idx) = next_visible {
                            if let Some(comment_line) = self.comment_start_line(&children[idx]) {
                                if comment_line == style_end_line {
                                    if let CssStmt::Comment(ref comment, span) = children[idx] {
                                        let comment = comment.clone();
                                        self.write_inline_comment(&comment, span)?;
                                        children.remove(idx);
                                    }
                                }
                            }
                        }
                    }
                }

                // Sub-problem C: If we just wrote a block with closing `}` and the next
                // visible child is a Comment on the same source line, write it inline
                if let Some(brace_line) = closing_brace_line {
                    let next_visible = children.iter().position(|c| !c.is_invisible());
                    if let Some(idx) = next_visible {
                        if let Some(comment_line) = self.comment_start_line(&children[idx]) {
                            if comment_line == brace_line {
                                if let CssStmt::Comment(ref comment, span) = children[idx] {
                                    let comment = comment.clone();
                                    self.write_inline_comment(&comment, span)?;
                                    children.remove(idx);
                                }
                            }
                        }
                    }
                }
            }

            self.write_optional_newline();
        }

        // In compressed mode, remove trailing semicolons before closing brace
        if self.options.is_compressed() {
            while self.buffer.last() == Some(&b';') {
                self.buffer.pop();
            }
        }

        self.indentation -= self.indent_width;

        if self.options.is_compressed() {
            self.buffer.push(b'}');
        } else {
            self.write_indentation();
            self.buffer.extend_from_slice(b"}");
        }

        Ok(())
    }

    fn write_optional_space(&mut self) {
        if !self.options.is_compressed() {
            self.buffer.push(b' ');
        }
    }

    fn write_optional_newline(&mut self) {
        if !self.options.is_compressed() {
            self.buffer.push(b'\n');
        }
    }

    fn write_supports_rule(&mut self, supports_rule: SupportsRule) -> SassResult<()> {
        self.write_indentation();
        self.buffer.extend_from_slice(b"@supports");

        if !supports_rule.params.is_empty() {
            self.buffer.push(b' ');
            self.buffer
                .extend_from_slice(supports_rule.params.as_bytes());
        }

        self.write_children(supports_rule.body, None)?;

        Ok(())
    }

    /// Returns whether or not text was written
    fn visit_stmt(&mut self, stmt: CssStmt) -> SassResult<bool> {
        if stmt.is_invisible() {
            return Ok(false);
        }

        match stmt {
            CssStmt::RuleSet { selector, body, .. } => {
                self.write_indentation();
                let sel_list = selector.as_selector_list();
                let brace_line = Some(self.source_line(sel_list.span.high()));
                self.write_top_level_selector_list(&sel_list);

                // Comment-only body on same line as `{`: render single-line (issue_894)
                if !self.options.is_compressed()
                    && !body.is_empty()
                    && body.iter().all(|s| matches!(s, CssStmt::Comment(..)))
                {
                    if let Some(bl) = brace_line {
                        let all_on_brace_line = body.iter().all(|s| {
                            self.comment_start_line(s) == Some(bl)
                        });
                        if all_on_brace_line {
                            self.buffer.extend_from_slice(b" { ");
                            for stmt in body {
                                self.visit_stmt(stmt)?;
                            }
                            self.buffer.extend_from_slice(b" }");
                            return Ok(true);
                        }
                    }
                }

                self.write_children(body, brace_line)?;
            }
            CssStmt::Media(media_rule, ..) => {
                self.write_indentation();
                self.buffer.extend_from_slice(b"@media ");

                if let Some((last, rest)) = media_rule.query.split_last() {
                    for query in rest {
                        self.write_media_query(query);

                        self.buffer.push(b',');

                        self.write_optional_space();
                    }

                    self.write_media_query(last);
                }

                let brace_line = media_rule
                    .query_span
                    .map(|span| self.source_line(span.high()));
                self.write_children(media_rule.body, brace_line)?;
            }
            CssStmt::UnknownAtRule(unknown_at_rule, ..) => {
                self.write_indentation();
                self.buffer.push(b'@');
                self.buffer
                    .extend_from_slice(unknown_at_rule.name.as_bytes());

                if !unknown_at_rule.params.is_empty() {
                    self.buffer.push(b' ');
                    if unknown_at_rule.params.contains('\n') {
                        // Multi-line params: preserve line structure
                        self.buffer
                            .extend_from_slice(unknown_at_rule.params.as_bytes());
                    } else {
                        self.buffer
                            .extend_from_slice(normalize_whitespace(&unknown_at_rule.params).as_bytes());
                    }
                }

                if !unknown_at_rule.has_body {
                    debug_assert!(unknown_at_rule.body.is_empty());
                    return Ok(true);
                } else if unknown_at_rule.body.iter().all(CssStmt::is_invisible) {
                    self.buffer.extend_from_slice(b" {}");
                    return Ok(true);
                } else if unknown_at_rule
                    .body
                    .iter()
                    .all(|s| matches!(s, CssStmt::Comment(..)))
                {
                    // Comment-only body renders on a single line
                    self.buffer.extend_from_slice(b" { ");
                    for stmt in unknown_at_rule.body {
                        self.visit_stmt(stmt)?;
                    }
                    self.buffer.extend_from_slice(b" }");
                    return Ok(true);
                }

                self.write_children(unknown_at_rule.body, None)?;
            }
            CssStmt::Style(style) => self.write_style(style)?,
            CssStmt::Comment(comment, span) => self.write_comment(&comment, span)?,
            CssStmt::KeyframesRuleSet(keyframes_rule_set) => {
                self.write_indentation();
                // todo: i bet we can do something like write_with_separator to avoid extra allocation
                let selector = keyframes_rule_set
                    .selector
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");

                self.buffer.extend_from_slice(selector.as_bytes());

                self.write_children(keyframes_rule_set.body, None)?;
            }
            CssStmt::Import(import, modifier) => self.write_import(&import, modifier)?,
            CssStmt::Supports(supports_rule, _) => self.write_supports_rule(supports_rule)?,
        }

        Ok(true)
    }
}

/// Returns true for Unicode Private Use Area characters, which should be
/// hex-escaped in CSS strings to avoid invisible/empty rendering.
fn is_private_use(c: char) -> bool {
    let cp = c as u32;
    // BMP Private Use Area
    (0xE000..=0xF8FF).contains(&cp)
    // Supplementary Private Use Areas
    || (0xF0000..=0xFFFFD).contains(&cp)
    || (0x100000..=0x10FFFD).contains(&cp)
}

/// Write a CSS hex escape like `\e600` with a trailing space if the next
/// character is a hex digit, space, or tab.
fn write_hex_escape(buffer: &mut Vec<u8>, code_point: u32, next: Option<char>) {
    buffer.push(b'\\');
    let hex = format!("{:x}", code_point);
    buffer.extend_from_slice(hex.as_bytes());

    if let Some(next_ch) = next {
        if next_ch.is_ascii_hexdigit() || next_ch == ' ' || next_ch == '\t' {
            buffer.push(b' ');
        }
    }
}

/// Collapse runs of spaces/tabs to single spaces on each line,
/// while preserving newlines and whitespace inside quoted strings.
fn normalize_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_quote: Option<char> = None;
    let mut last_was_space = false;

    for c in s.chars() {
        match in_quote {
            Some(q) => {
                result.push(c);
                if c == q {
                    in_quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    in_quote = Some(c);
                    result.push(c);
                    last_was_space = false;
                } else if c == ' ' || c == '\t' {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    if c == '\n' {
                        last_was_space = false;
                    }
                    result.push(c);
                    last_was_space = false;
                }
            }
        }
    }

    result
}
