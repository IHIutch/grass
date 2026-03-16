use crate::builtin::{
    color::{
        css_color4::{color_fn, lab, lch, oklab, oklch},
        hsl::{complement, grayscale, hue, invert, lightness, saturation},
        hwb::{blackness, hwb, whiteness},
        opacity::{alpha, module_opacity},
        other::{adjust_color, change_color, ie_hex_str, scale_color},
        rgb::{blue, green, mix, red},
        space_fns::{
            channel, is_in_gamut, is_legacy, is_missing, is_powerless, same, space, to_gamut,
            to_space,
        },
    },
    modules::Module,
};

pub(crate) fn declare(f: &mut Module) {
    f.insert_builtin("adjust", adjust_color);
    f.insert_builtin("alpha", alpha);
    f.insert_builtin("blue", blue);
    f.insert_builtin("change", change_color);
    f.insert_builtin("channel", channel);
    f.insert_builtin("complement", complement);
    f.insert_builtin("grayscale", grayscale);
    f.insert_builtin("green", green);
    f.insert_builtin("hue", hue);
    f.insert_builtin("ie-hex-str", ie_hex_str);
    f.insert_builtin("invert", invert);
    f.insert_builtin("is-in-gamut", is_in_gamut);
    f.insert_builtin("is-legacy", is_legacy);
    f.insert_builtin("is-missing", is_missing);
    f.insert_builtin("is-powerless", is_powerless);
    f.insert_builtin("lightness", lightness);
    f.insert_builtin("mix", mix);
    f.insert_builtin("opacity", module_opacity);
    f.insert_builtin("red", red);
    f.insert_builtin("saturation", saturation);
    f.insert_builtin("same", same);
    f.insert_builtin("scale", scale_color);
    f.insert_builtin("space", space);
    f.insert_builtin("to-gamut", to_gamut);
    f.insert_builtin("to-space", to_space);
    f.insert_builtin("blackness", blackness);
    f.insert_builtin("whiteness", whiteness);
    f.insert_builtin("hwb", hwb);
    f.insert_builtin("lab", lab);
    f.insert_builtin("lch", lch);
    f.insert_builtin("oklab", oklab);
    f.insert_builtin("oklch", oklch);
    f.insert_builtin("color", color_fn);
}
