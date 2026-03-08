#[macro_use]
mod macros;

test!(
    blackness_black,
    "@use \"sass:color\";\na {\n  color: color.blackness(black);\n}\n",
    "a {\n  color: 100%;\n}\n"
);
test!(
    blackness_white,
    "@use \"sass:color\";\na {\n  color: color.blackness(white);\n}\n",
    "a {\n  color: 0%;\n}\n"
);

test!(
    whiteness_black,
    "@use \"sass:color\";\na {\n  color: color.whiteness(black);\n}\n",
    "a {\n  color: 0%;\n}\n"
);
test!(
    whiteness_white,
    "@use \"sass:color\";\na {\n  color: color.whiteness(white);\n}\n",
    "a {\n  color: 100%;\n}\n"
);
test!(
    blackness_approx_50_pct,
    "@use \"sass:color\";\na {\n  color: color.blackness(color.hwb(0, 0%, 50%));\n}\n",
    "a {\n  color: 50%;\n}\n"
);
test!(
    blackness_approx_50_pct_and_whiteness,
    "@use \"sass:color\";\na {\n  color: color.blackness(color.hwb(0, 50%, 50%));\n}\n",
    "a {\n  color: 50%;\n}\n"
);
test!(
    blackness_approx_70_pct_and_whiteness,
    "@use \"sass:color\";\na {\n  color: color.blackness(color.hwb(0, 70%, 70%));\n}\n",
    "a {\n  color: 50%;\n}\n"
);
test!(
    blackness_approx_half_pct,
    "@use \"sass:color\";\na {\n  color: color.blackness(color.hwb(0, 0%, 0.5%));\n}\n",
    "a {\n  color: 0.5%;\n}\n"
);
test!(
    hwb_half_blackness,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 0%, 50%);\n}\n",
    "a {\n  color: hsl(0, 100%, 25%);\n}\n"
);
test!(
    hwb_equal_white_black_50,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 50%, 50%);\n}\n",
    "a {\n  color: hsl(0, 0%, 50%);\n}\n"
);
test!(
    hwb_equal_white_black_70,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 70%, 70%);\n}\n",
    "a {\n  color: hsl(0, 0%, 50%);\n}\n"
);
test!(
    hwb_half_percent_black,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 0%, 0.5%);\n}\n",
    "a {\n  color: hsl(0, 100%, 49.75%);\n}\n"
);
test!(
    hwb_black_100,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 0%, 100%);\n}\n",
    "a {\n  color: black;\n}\n"
);
test!(
    blackness_named,
    "@use \"sass:color\";\na {\n  color: color.blackness($color: color.hwb(0, 0%, 42%));\n}\n",
    "a {\n  color: 42%;\n}\n"
);
test!(
    hwb_alpha_unitless,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 0%, 100%, 0.04);\n}\n",
    "a {\n  color: hsla(0, 0%, 0%, 0.04);\n}\n"
);
test!(
    hwb_alpha_unit_percent,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 0%, 100%, 0.04%);\n}\n",
    "a {\n  color: hsla(0, 0%, 0%, 0.0004);\n}\n"
);
test!(
    hwb_negative_alpha,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 0%, 100%, -0.5);\n}\n",
    "a {\n  color: hsla(0, 0%, 0%, 0);\n}\n"
);
test!(
    hue_60_whiteness_20_blackness_100,
    "@use \"sass:color\";\na {\n  color: color.hwb(60, 20%, 100%);\n}\n",
    "a {\n  color: hsl(0, 0%, 16.6666666667%);\n}\n"
);
test!(
    one_arg_with_slash,
    "@use \"sass:color\";\na {\n  color: color.hwb(180 30% 40% / 0);\n}\n",
    "a {\n  color: hsla(180, 33.3333333333%, 45%, 0);\n}\n"
);
test!(
    hue_has_unit_rad,
    "@use \"sass:color\";\na {\n  color: color.hwb(1rad, 30%, 40%);\n}\n",
    "a {\n  color: hsl(57.2957795131, 33.3333333333%, 45%);\n}\n"
);
test!(
    scale_whiteness,
    "a {\n  color: scale-color(#cc6666, $whiteness: 100%);\n}\n",
    "a {\n  color: rgb(212.5, 212.5, 212.5);\n}\n"
);
error!(
    hwb_whiteness_missing_pct,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 0, 100);\n}\n",
    "Error: $whiteness: Expected 0 to have unit \"%\"."
);
error!(
    hwb_two_args,
    "@use \"sass:color\";\na {\n  color: color.hwb(#123, 0.5);\n}\n",
    "Error: Only 1 argument allowed, but 2 were passed."
);
test!(
    hwb_blackness_too_high,
    "@use \"sass:color\";\na {\n  color: color.hwb(0, 30%, 101%, 0.5);\n}\n",
    "a {\n  color: hsla(0, 0%, 22.9007633588%, 0.5);\n}\n"
);
error!(
    blackness_no_args,
    "@use \"sass:color\";\na {\n  color: color.blackness();\n}\n",
    "Error: Missing argument $color."
);
error!(
    whiteness_no_args,
    "@use \"sass:color\";\na {\n  color: color.whiteness();\n}\n",
    "Error: Missing argument $color."
);
error!(
    hwb_var_channels,
    "@use \"sass:color\";\na {\n  color: color.hwb(var(--foo));\n}\n",
    "Error: Expected numeric channels, got \"hwb(var(--foo))\"."
);
