#[macro_use]
mod macros;

test!(
    preserves_named_color_case,
    "a {\n  color: OrAnGe;\n}\n",
    "a {\n  color: OrAnGe;\n}\n"
);
test!(
    named_color_casing_is_color,
    "a {\n  color: hue(RED);\n}\n",
    "a {\n  color: 0deg;\n}\n"
);
test!(
    preserves_hex_color_case,
    "a {\n  color: #FfFfFf;\n}\n",
    "a {\n  color: #FfFfFf;\n}\n"
);
test!(
    preserves_hex_8_val_10000000,
    "a {\n  color: #10000000;\n}\n",
    "a {\n  color: rgba(16, 0, 0, 0);\n}\n"
);
test!(
    preserves_hex_8_val_12312312,
    "a {\n  color: #12312312;\n}\n",
    "a {\n  color: rgba(18, 49, 35, 0.0705882353);\n}\n"
);
test!(
    preserves_hex_8_val_ab234cff,
    "a {\n  color: #ab234cff;\n}\n",
    "a {\n  color: #ab234cff;\n}\n"
);
test!(
    preserves_hex_6_val_000000,
    "a {\n  color: #000000;\n}\n",
    "a {\n  color: #000000;\n}\n"
);
test!(
    preserves_hex_6_val_123123,
    "a {\n  color: #123123;\n}\n",
    "a {\n  color: #123123;\n}\n"
);
test!(
    preserves_hex_6_val_ab234c,
    "a {\n  color: #ab234c;\n}\n",
    "a {\n  color: #ab234c;\n}\n"
);
test!(
    preserves_hex_4_val_0000,
    "a {\n  color: #0000;\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0);\n}\n"
);
test!(
    preserves_hex_4_val_123a,
    "a {\n  color: #123a;\n}\n",
    "a {\n  color: rgba(17, 34, 51, 0.6666666667);\n}\n"
);
test!(
    preserves_hex_4_val_ab2f,
    "a {\n  color: #ab2f;\n}\n",
    "a {\n  color: #ab2f;\n}\n"
);
test!(
    preserves_hex_3_val_000,
    "a {\n  color: #000;\n}\n",
    "a {\n  color: #000;\n}\n"
);
test!(
    preserves_hex_3_val_123,
    "a {\n  color: #123;\n}\n",
    "a {\n  color: #123;\n}\n"
);
test!(
    preserves_hex_3_val_ab2,
    "a {\n  color: #ab2;\n}\n",
    "a {\n  color: #ab2;\n}\n"
);
test!(
    converts_rgb_to_named_color,
    "a {\n  color: rgb(0, 0, 0);\n}\n",
    "a {\n  color: rgb(0, 0, 0);\n}\n"
);
test!(
    converts_rgba_to_named_color_red,
    "a {\n  color: rgb(255, 0, 0, 255);\n}\n",
    "a {\n  color: rgb(255, 0, 0);\n}\n"
);
test!(
    rgb_negative,
    "a {\n  color: rgb(-1, 1, 1);\n}\n",
    "a {\n  color: rgb(0, 1, 1);\n}\n"
);
test!(
    rgb_binop,
    "a {\n  color: rgb(1, 2, 1+2);\n}\n",
    "a {\n  color: rgb(1, 2, 3);\n}\n"
);
test!(
    rgb_pads_0,
    "a {\n  color: rgb(1, 2, 3);\n}\n",
    "a {\n  color: rgb(1, 2, 3);\n}\n"
);
test!(
    rgba_percent,
    "a {\n  color: rgba(159%, 169, 169%, 50%);\n}\n",
    "a {\n  color: rgba(255, 169, 255, 0.5);\n}\n"
);
test!(
    rgba_percent_round_up,
    "a {\n  color: rgba(59%, 169, 69%, 50%);\n}\n",
    "a {\n  color: rgba(150.45, 169, 175.95, 0.5);\n}\n"
);
test!(
    rgb_double_digits,
    "a {\n  color: rgb(254, 255, 255);\n}\n",
    "a {\n  color: rgb(254, 255, 255);\n}\n"
);
test!(
    rgb_double_digits_white,
    "a {\n  color: rgb(255, 255, 255);\n}\n",
    "a {\n  color: rgb(255, 255, 255);\n}\n"
);
test!(
    alpha_function_4_hex,
    "a {\n  color: alpha(#0123);\n}\n",
    "a {\n  color: 0.2;\n}\n"
);
test!(
    alpha_function_named_color,
    "a {\n  color: alpha(red);\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    opacity_function_number,
    "a {\n  color: opacity(1);\n}\n",
    "a {\n  color: opacity(1);\n}\n"
);
test!(
    opacity_function_number_unit,
    "a {\n  color: opacity(1px);\n}\n",
    "a {\n  color: opacity(1px);\n}\n"
);
test!(
    rgba_one_arg,
    "a {\n  color: rgba(1 2 3);\n}\n",
    "a {\n  color: rgb(1, 2, 3);\n}\n"
);
test!(
    rgb_two_args,
    "a {\n  color: rgb(#123, 0);\n}\n",
    "a {\n  color: rgba(17, 34, 51, 0);\n}\n"
);
test!(
    rgba_two_args,
    "a {\n  color: rgba(red, 0.5);\n}\n",
    "a {\n  color: rgba(255, 0, 0, 0.5);\n}\n"
);
test!(
    rgba_opacity_over_1,
    "a {\n  color: rgba(1, 2, 3, 3);\n}\n",
    "a {\n  color: rgb(1, 2, 3);\n}\n"
);
test!(
    rgba_negative_alpha,
    "a {\n  color: rgba(1, 2, 3, -10%);\n}\n",
    "a {\n  color: rgba(1, 2, 3, 0);\n}\n"
);
test!(
    rgba_opacity_decimal,
    "a {\n  color: rgba(1, 2, 3, .6);\n}\n",
    "a {\n  color: rgba(1, 2, 3, 0.6);\n}\n"
);
test!(
    rgba_opacity_percent,
    "a {\n  color: rgba(1, 2, 3, 50%);\n}\n",
    "a {\n  color: rgba(1, 2, 3, 0.5);\n}\n"
);
test!(
    rgba_3_args,
    "a {\n  color: rgba(7.1%, 20.4%, 33.9%);\n}\n",
    "a {\n  color: rgb(18.105, 52.02, 86.445);\n}\n"
);
error!(
    rgb_no_args,
    "a {\n  color: rgb();\n}\n", "Error: Missing argument $channels."
);
error!(
    rgba_no_args,
    "a {\n  color: rgba();\n}\n", "Error: Missing argument $channels."
);
test!(
    invert_no_weight,
    "a {\n  color: invert(white);\n}\n",
    "a {\n  color: black;\n}\n"
);
test!(
    plain_invert_no_unit,
    "a {\n  color: invert(1);\n}\n",
    "a {\n  color: invert(1);\n}\n"
);
test!(
    plain_invert_unit_percent,
    "a {\n  color: invert(10%);\n}\n",
    "a {\n  color: invert(10%);\n}\n"
);
test!(
    plain_invert_unit_deg,
    "a {\n  color: invert(1deg);\n}\n",
    "a {\n  color: invert(1deg);\n}\n"
);
test!(
    plain_invert_negative,
    "a {\n  color: invert(-1);\n}\n",
    "a {\n  color: invert(-1);\n}\n"
);
test!(
    plain_invert_float,
    "a {\n  color: invert(1.5);\n}\n",
    "a {\n  color: invert(1.5);\n}\n"
);
test!(
    plain_invert_arithmetic,
    "a {\n  color: invert(1 + 1);\n}\n",
    "a {\n  color: invert(2);\n}\n"
);
test!(
    plain_invert_nan,
    "a {\n  color: invert((0 / 0));\n}\n",
    "a {\n  color: invert(NaN);\n}\n"
);
error!(
    plain_invert_two_args,
    "a {\n  color: invert(1, 50%);\n}\n",
    "Error: Only one argument may be passed to the plain-CSS invert() function."
);
test!(
    invert_weight_percent,
    "a {\n  color: invert(white, 20%);\n}\n",
    "a {\n  color: #cccccc;\n}\n"
);
test!(
    invert_weight_percent_turquoise,
    "a {\n  color: invert(turquoise, 23%);\n}\n",
    "a {\n  color: rgb(93.21, 179.61, 170.97);\n}\n"
);
test!(
    invert_weight_no_unit,
    "a {\n  color: invert(white, 20);\n}\n",
    "a {\n  color: #cccccc;\n}\n"
);

test!(
    transparentize,
    "a {\n  color: transparentize(rgba(0, 0, 0, 0.5), 0.1);\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0.4);\n}\n"
);
test!(
    fade_out,
    "a {\n  color: fade-out(rgba(0, 0, 0, 0.8), 0.2);\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0.6);\n}\n"
);
test!(
    opacify,
    "a {\n  color: opacify(rgba(0, 0, 0, 0.5), 0.1);\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0.6);\n}\n"
);
test!(
    fade_in,
    "a {\n  color: opacify(rgba(0, 0, 17, 0.8), 0.2);\n}\n",
    "a {\n  color: #000011;\n}\n"
);
// Plan 027 / Solo scratchpad #76: with_alpha/fade_in/fade_out previously
// rounded fractional channels via from_rgba()'s fuzzy_round. Expectations
// verified against dart-sass 1.97.3 via:
// printf '%s' '<input>' | npx sass@1.97.3 --stdin --style=expanded
test!(
    with_alpha_fractional_rgb_channels,
    "a {\n  color: rgba(rgb(206.6, 226, 254.6), 0.5);\n}\n",
    "a {\n  color: rgba(206.6, 226, 254.6, 0.5);\n}\n"
);
test!(
    fade_in_fractional_rgb_channels,
    "a {\n  color: fade-in(rgba(206.6, 226, 254.6, 0.5), 0.1);\n}\n",
    "a {\n  color: rgba(206.6, 226, 254.6, 0.6);\n}\n"
);
test!(
    fade_out_fractional_rgb_channels,
    "a {\n  color: fade-out(rgba(206.6, 226, 254.6, 0.5), 0.1);\n}\n",
    "a {\n  color: rgba(206.6, 226, 254.6, 0.4);\n}\n"
);
test!(
    rgba_mix_fractional_channels_bootstrap_shaped,
    "a {\n  color: rgba(mix(#0d6efd, #ced4da, 15%), .5);\n}\n",
    "a {\n  color: rgba(177.05, 196.7, 223.25, 0.5);\n}\n"
);
test!(
    grayscale_1,
    "a {\n  color: grayscale(plum);\n}\n",
    "a {\n  color: rgb(190.5, 190.5, 190.5);\n}\n"
);
test!(
    grayscale_2,
    "a {\n  color: grayscale(red);\n}\n",
    "a {\n  color: rgb(127.5, 127.5, 127.5);\n}\n"
);
test!(
    grayscale_number,
    "a {\n  color: grayscale(15%);\n}\n",
    "a {\n  color: grayscale(15%);\n}\n"
);
test!(
    complement,
    "a {\n  color: complement(red);\n}\n",
    "a {\n  color: aqua;\n}\n"
);
test!(
    complement_hue_under_180,
    "a {\n  color: complement(#abcdef);\n}\n",
    "a {\n  color: #efcdab;\n}\n"
);
test!(
    mix_no_weight,
    "a {\n  color: mix(#f00, #00f);\n}\n",
    "a {\n  color: rgb(127.5, 0, 127.5);\n}\n"
);
test!(
    mix_weight_25,
    "a {\n  color: mix(#f00, #00f, 25%);\n}\n",
    "a {\n  color: rgb(63.75, 0, 191.25);\n}\n"
);
test!(
    mix_opacity,
    "a {\n  color: mix(rgba(255, 0, 0, 0.5), #00f);\n}\n",
    "a {\n  color: rgba(63.75, 0, 191.25, 0.75);\n}\n"
);
test!(
    mix_sanity_check,
    "a {\n  color: mix(black, white);\n}\n",
    "a {\n  color: rgb(127.5, 127.5, 127.5);\n}\n"
);
// Plan 027 / Solo scratchpad #76: mix() previously blended fractional input
// channels through the rounding red()/green()/blue() getters instead of raw
// channels. Expectations verified against dart-sass 1.97.3 via:
// printf '%s' '<input>' | npx sass@1.97.3 --stdin --style=expanded
test!(
    mix_fractional_rgb_channels,
    "a {\n  color: mix(#000, rgb(206.6, 226, 254.6), 5%);\n}\n",
    "a {\n  color: rgb(196.27, 214.7, 241.87);\n}\n"
);
test!(
    mix_fractional_hsl_input,
    "a {\n  color: mix(hsl(210.5, 60.3%, 50.2%), #123456, 33.3%);\n}\n",
    "a {\n  color: rgb(29.13386499, 76.8863389165, 125.48879501);\n}\n"
);
test!(
    mix_fractional_alpha_weight,
    "a {\n  color: mix(rgba(100.4, 50, 50, 0.4), rgba(20, 200.7, 30, 0.9), 25%);\n}\n",
    "a {\n  color: rgba(28.04, 185.63, 32, 0.775);\n}\n"
);
test!(
    mix_fractional_nested_bootstrap_shaped,
    "a {\n  color: mix(#000, mix(white, #0d6efd, 80%), 5%);\n}\n",
    "a {\n  color: rgb(196.27, 214.7, 241.87);\n}\n"
);
test!(
    change_color_blue,
    "a {\n  color: change-color(#102030, $blue: 5);\n}\n",
    "a {\n  color: #102005;\n}\n"
);
test!(
    change_color_red_blue,
    "a {\n  color: change-color(#102030, $red: 120, $blue: 5);\n}\n",
    "a {\n  color: #782005;\n}\n"
);
test!(
    change_color_lum_alpha,
    "a {\n  color: change-color(hsl(25, 100%, 80%), $lightness: 40%, $alpha: 0.8);\n}\n",
    "a {\n  color: hsla(25, 100%, 40%, 0.8);\n}\n"
);
test!(
    adjust_color_blue,
    "a {\n  color: adjust-color(#102030, $blue: 5);\n}\n",
    "a {\n  color: #102035;\n}\n"
);
test!(
    adjust_color_negative,
    "a {\n  color: adjust-color(#102030, $red: -5, $blue: 5);\n}\n",
    "a {\n  color: #0b2035;\n}\n"
);
test!(
    adjust_color_lum_alpha,
    "a {\n  color: adjust-color(hsl(25, 100%, 80%), $lightness: -30%, $alpha: -0.4);\n}\n",
    "a {\n  color: hsla(25, 100%, 50%, 0.6);\n}\n"
);
test!(
    scale_color_lightness,
    "a {\n  color: scale-color(hsl(120, 70%, 80%), $lightness: 50%);\n}\n",
    "a {\n  color: hsl(120, 70%, 90%);\n}\n"
);
test!(
    scale_color_neg_lightness_and_pos_saturation,
    "a {\n  color: scale-color(turquoise, $saturation: 24%, $lightness: -48%);\n}\n",
    "a {\n  color: rgb(15.8934486486, 133.8665513514, 122.0692410811);\n}\n"
);
error!(
    scale_color_named_arg_hue,
    "a {\n  color: scale-color(red, $hue: 10%);\n}\n", "Error: No argument named $hue."
);
test!(
    scale_color_negative,
    "a {\n  color: scale-color(rgb(200, 150%, 170%), $green: -40%, $blue: 70%);\n}\n",
    "a {\n  color: #c899ff;\n}\n"
);
test!(
    change_color_named_arg_hue,
    "a {\n  color: change-color(blue, $hue: 150);\n}\n",
    "a {\n  color: rgb(0, 255, 127.5);\n}\n"
);
test!(
    adjust_color_named_arg_hue,
    "a {\n  color: adjust-color(blue, $hue: 150);\n}\n",
    "a {\n  color: rgb(255, 127.5, 0);\n}\n"
);
test!(
    change_color_negative_hue,
    "a {\n  color: change-color(red, $hue: -60);\n}\n",
    "a {\n  color: fuchsia;\n}\n"
);
test!(
    scale_color_alpha,
    "a {\n  color: scale-color(hsl(200, 70%, 80%), $saturation: -90%, $alpha: -30%);\n}\n",
    "a {\n  color: hsla(200, 7%, 80%, 0.7);\n}\n"
);
test!(
    scale_color_alpha_over_1,
    "a {\n  color: scale-color(sienna, $alpha: -70%);\n}\n",
    "a {\n  color: rgba(160, 82, 45, 0.3);\n}\n"
);
test!(
    ie_hex_str_hex_3,
    "a {\n  color: ie-hex-str(#abc);\n}\n",
    "a {\n  color: #FFAABBCC;\n}\n"
);
test!(
    ie_hex_str_hex_6,
    "a {\n  color: ie-hex-str(#3322BB);\n}\n",
    "a {\n  color: #FF3322BB;\n}\n"
);
test!(
    ie_hex_str_rgb,
    "a {\n  color: ie-hex-str(rgba(0, 255, 0, 0.5));\n}\n",
    "a {\n  color: #8000FF00;\n}\n"
);
test!(
    rgba_1_arg,
    "a {\n  color: rgba(74.7% 173 93%);\n}\n",
    "a {\n  color: rgb(190.485, 173, 237.15);\n}\n"
);
test!(
    hsla_1_arg,
    "a {\n  color: hsla(60 60% 50%);\n}\n",
    "a {\n  color: hsl(60, 60%, 50%);\n}\n"
);
test!(
    hsla_1_arg_weird_units,
    "a {\n  color: hsla(60foo 60foo 50foo);\n}\n",
    "a {\n  color: hsl(60, 60%, 50%);\n}\n"
);
test!(
    sass_spec__spec_colors_basic,
    r#"p {
  color: rgb(255, 128, 0);
  color: red green blue;
  color: (red) (green) (blue);
  color: red + hux;
  color: unquote("red") + green;
  foo: rgb(200, 150%, 170%);
}
"#,
    "p {\n  color: rgb(255, 128, 0);\n  color: red green blue;\n  color: red green blue;\n  color: redhux;\n  color: redgreen;\n  foo: rgb(200, 255, 255);\n}\n"
);
test!(
    sass_spec__spec_colors_change_color,
    "p {
  color: change-color(#102030, $blue: 5);
  color: change-color(#102030, $alpha: .325);
  color: change-color(#102030, $red: 120, $blue: 5);
  color: change-color(hsl(25, 100%, 80%), $lightness: 40%, $alpha: 0.8);
}
",
    "p {\n  color: #102005;\n  color: rgba(16, 32, 48, 0.325);\n  color: #782005;\n  color: hsla(25, 100%, 40%, 0.8);\n}\n"
);
test!(
    transparent_from_function,
    "a {\n  color: rgb(transparent, 0);\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0);\n}\n"
);
test!(
    named_color_transparent_opacity,
    "a {\n  color: opacity(transparent);\n}\n",
    "a {\n  color: 0;\n}\n"
);
test!(
    negative_values_in_rgb,
    "a {\n  color: rgb(-1 -1 -1);\n}\n",
    "a {\n  color: rgb(0, 0, 0);\n}\n"
);
test!(
    interpolation_after_hash_containing_only_hex_chars,
    "a {\n  color: ##{123};\n  color: type-of(##{123});\n}\n",
    "a {\n  color: #123;\n  color: string;\n}\n"
);
test!(
    non_hex_chars_after_hash_are_still_touching_hash,
    "a {\n  color: #ooobar;\n}\n",
    "a {\n  color: #ooobar;\n}\n"
);
test!(
    more_than_8_hex_chars_after_hash_starts_with_letter,
    "a {\n  color: #ffffffffff;\n}\n",
    "a {\n  color: #ffffffffff;\n}\n"
);
test!(
    more_than_8_hex_chars_after_hash_starts_with_number,
    "a {\n  color: #0000000000;\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0) 0;\n}\n"
);
test!(
    more_than_8_hex_chars_after_hash_starts_with_number_contains_hex_char,
    "a {\n  color: #00000000f00;\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0) f00;\n}\n"
);
test!(
    all_three_rgb_channels_have_decimal,
    "a {\n  color: rgba(1.5, 1.5, 1.5, 1);\n}\n",
    "a {\n  color: rgb(1.5, 1.5, 1.5);\n}\n"
);
test!(
    builtin_fn_red_rounds_channel,
    "a {\n  color: red(rgba(1.5, 1.5, 1.5, 1));\n}\n",
    "a {\n  color: 2;\n}\n"
);
test!(
    builtin_fn_green_rounds_channel,
    "a {\n  color: green(rgba(1.5, 1.5, 1.5, 1));\n}\n",
    "a {\n  color: 2;\n}\n"
);
test!(
    builtin_fn_blue_rounds_channel,
    "a {\n  color: blue(rgba(1.5, 1.5, 1.5, 1));\n}\n",
    "a {\n  color: 2;\n}\n"
);
test!(
    color_equality_named_and_hex,
    "a {\n  color: red==#ff0000;\n}\n",
    "a {\n  color: true;\n}\n"
);
test!(
    color_equality_named_and_hsla,
    "a {\n  color: hsla(0deg, 100%, 50%)==red;\n}\n",
    "a {\n  color: true;\n}\n"
);
test!(
    alpha_filter_one_arg,
    "a {\n  color: alpha(a=a);\n}\n",
    "a {\n  color: alpha(a=a);\n}\n"
);
test!(
    alpha_filter_multiple_args,
    "a {\n  color: alpha(a=a, b=b, c=d, d=d);\n}\n",
    "a {\n  color: alpha(a=a, b=b, c=d, d=d);\n}\n"
);
test!(
    alpha_filter_whitespace,
    "a {\n  color: alpha(a   =    a);\n}\n",
    "a {\n  color: alpha(a=a);\n}\n"
);
test!(
    alpha_filter_named,
    "a {\n  color: alpha($color: a=a);\n}\n",
    "a {\n  color: alpha(a=a);\n}\n"
);
error!(
    alpha_filter_both_null,
    "a {\n  color: alpha(null=null);\n}\n", "Error: $color: = is not a color."
);
error!(
    alpha_filter_multiple_args_one_not_valid_filter,
    "a {\n  color: alpha(a=a, b);\n}\n", "Error: Only 1 argument allowed, but 2 were passed."
);
error!(
    alpha_filter_invalid_from_whitespace,
    "a {\n  color: alpha( A a   =    a  );\n}\n", "Error: $color: A a=a is not a color."
);
error!(
    alpha_filter_invalid_non_alphabetic_start,
    "a {\n  color: alpha(1=a);\n}\n", "Error: $color: 1=a is not a color."
);
// todo: we need many more of these tests
test!(
    rgba_one_arg_special_fn_4th_arg_max,
    "a {\n  color: rgba(1 2 max(3, 3));\n}\n",
    "a {\n  color: rgb(1, 2, 3);\n}\n"
);
test!(
    rgb_special_fn_4_arg_maintains_units,
    "a {\n  color: rgb(1, 0.02, 3%, max(0.4));\n}\n",
    "a {\n  color: rgba(1, 0.02, 7.65, 0.4);\n}\n"
);
test!(
    rgb_special_fn_3_arg_maintains_units,
    "a {\n  color: rgb(1, 0.02, max(0.4));\n}\n",
    "a {\n  color: rgb(1, 0.02, 0.4);\n}\n"
);
test!(
    rgb_special_fn_2_arg_first_non_color,
    "a {\n  color: rgb(1, var(--foo));\n}\n",
    "a {\n  color: rgb(1, var(--foo));\n}\n"
);
test!(
    // Expectation updated for Plan 027 / Solo scratchpad #76: rgb(1%, 1, 1)'s
    // red channel is fractional (2.55), and the var()-alpha fallback path
    // must preserve raw channels rather than rounding through red()/green()/
    // blue(). Verified against dart-sass 1.97.3:
    // printf '%s' 'a { color: rgb(rgb(1%, 1, 1), var(--foo)); }' | npx sass@1.97.3 --stdin --style=expanded
    // -> rgb(2.55, 1, 1, var(--foo))
    rgb_special_fn_2_arg_first_is_color,
    "a {\n  color: rgb(rgb(1%, 1, 1), var(--foo));;\n}\n",
    "a {\n  color: rgb(2.55, 1, 1, var(--foo));\n}\n"
);
// Plan 027 / Solo scratchpad #76: the calc()-alpha fallback path (same
// code as the var()-alpha path above) also preserves raw channels.
// Verified against dart-sass 1.97.3 via:
// printf '%s' '<input>' | npx sass@1.97.3 --stdin --style=expanded
test!(
    rgba_special_fn_alpha_fractional_rgb_channels,
    "a {\n  color: rgba(rgb(206.6, 226, 254.6), calc(1 - var(--x)));\n}\n",
    "a {\n  color: rgba(206.6, 226, 254.6, calc(1 - var(--x)));\n}\n"
);
test!(
    interpolated_named_color_is_not_color,
    "a {\n  color: type-of(r#{e}d);\n}\n",
    "a {\n  color: string;\n}\n"
);
test!(
    color_equality_differ_in_green_channel,
    "a {\n  color: rgb(1, 1, 1) == rgb(1, 2, 1);\n}\n",
    "a {\n  color: false;\n}\n"
);
test!(
    color_equality_differ_in_blue_channel,
    "a {\n  color: rgb(1, 1, 1) == rgb(1, 1, 2);\n}\n",
    "a {\n  color: false;\n}\n"
);
test!(
    color_equality_differ_in_alpha_channel,
    "a {\n  color: rgba(1, 1, 1, 1.0) == rgba(1, 1, 1, 0.5);\n}\n",
    "a {\n  color: false;\n}\n"
);
test!(
    invert_weight_zero_is_nop,
    "a {\n  color: invert(#0f0f0f, 0);\n}\n",
    "a {\n  color: #0f0f0f;\n}\n"
);
test!(
    mix_combined_weight_is_normalized_weight,
    "a {\n  color: mix(rgba(255, 20, 0, 0), rgba(0, 20, 255, 1), 100);\n}\n",
    "a {\n  color: rgba(255, 20, 0, 0);\n}\n"
);
test!(
    hue_largest_channel_is_blue,
    "a {\n  color: hue(rgb(1, 2, 5));\n}\n",
    "a {\n  color: 225deg;\n}\n"
);
test!(
    rgb_3_args_first_arg_is_special_fn,
    "a {\n  color: rgb(env(--foo), 2, 3);\n}\n",
    "a {\n  color: rgb(env(--foo), 2, 3);\n}\n"
);
test!(
    hsl_conversion_is_correct,
    "a {
        color: hue(red);
        color: saturation(red);
        color: lightness(red);
        color: change-color(red, $lightness: 95%);
        color: red(change-color(red, $lightness: 95%));
        color: blue(change-color(red, $lightness: 95%));
        color: green(change-color(red, $lightness: 95%));
    }",
    "a {\n  color: 0deg;\n  color: 100%;\n  color: 50%;\n  color: rgb(255, 229.5, 229.5);\n  color: 255;\n  color: 229;\n  color: 229;\n}\n"
);
test!(
    slash_list_alpha,
    "@use 'sass:list';
    a {
        color: rgb(list.slash(1 2 3, var(--c)));
    }",
    "a {\n  color: rgb(1, 2, 3, var(--c));\n}\n"
);
test!(
    rgb_two_arg_nan_alpha,
    "a {
        color: rgb(red, 0/0);
        color: opacity(rgb(red, 0/0));
    }",
    "a {\n  color: red;\n  color: 1;\n}\n"
);
error!(
    rgb_more_than_4_args,
    "a {\n  color: rgb(59%, 169, 69%, 50%, 50%);\n}\n",
    "Error: Only 4 arguments allowed, but 5 were passed."
);
error!(
    rgba_more_than_4_args,
    "a {\n  color: rgba(59%, 169, 69%, 50%, 50%);\n}\n",
    "Error: Only 4 arguments allowed, but 5 were passed."
);
error!(
    opacify_amount_nan,
    "a {\n  color: opacify(#fff, (0/0));\n}\n",
    "Error: $amount: Expected calc(NaN) to be within 0 and 1."
);
error!(
    interpolated_string_is_not_color,
    "a {\n  color: red(r#{e}d);\n}\n", "Error: $color: red is not a color."
);
error!(
    single_arg_saturate_expects_number,
    "a {\n  color: saturate(red);\n}\n", "Error: $amount: red is not a number."
);
error!(
    saturate_two_arg_first_is_number,
    "a {\n  color: saturate(1, 2);\n}\n", "Error: $color: 1 is not a color."
);
error!(
    hex_color_starts_with_number_non_hex_digit_at_position_2,
    "a {\n  color: #0zz;\n}\n", "Error: Expected hex digit."
);
error!(
    hex_color_starts_with_number_non_hex_digit_at_position_3,
    "a {\n  color: #00z;\n}\n", "Error: Expected hex digit."
);
test!(
    hex_color_starts_with_number_non_hex_digit_at_position_4,
    "a {\n  color: #000z;\n}\n",
    "a {\n  color: #000 z;\n}\n"
);
test!(
    hex_color_starts_with_number_non_hex_digit_at_position_5,
    "a {\n  color: #0000z;\n}\n",
    "a {\n  color: rgba(0, 0, 0, 0) z;\n}\n"
);
test!(
    opacity_nan,
    "a {\n  color: opacity(0/0);\n}\n",
    "a {\n  color: opacity(NaN);\n}\n"
);
test!(
    change_color_no_change,
    "a {\n  color: change-color(red);\n}\n",
    "a {\n  color: red;\n}\n"
);
test!(
    change_color_hwb_hue,
    "a {\n  color: change-color(red, $whiteness: 50%, $hue: 230);\n}\n",
    "a {\n  color: rgb(127.5, 148.75, 255);\n}\n"
);
test!(
    aqua_alias,
    "a {\n  color: cyan == aqua;\n}\n",
    "a {\n  color: true;\n}\n"
);
test!(
    fuchsia_alias,
    "a {\n  color: magenta == fuchsia;\n}\n",
    "a {\n  color: true;\n}\n"
);
error!(
    hex_color_starts_with_number_non_hex_digit_at_position_6,
    "a {\n  color: #00000z;\n}\n", "Error: Expected hex digit."
);
error!(
    opacity_arg_not_color_or_number,
    "a {\n  color: opacity(a);\n}\n", "Error: $color: a is not a color."
);
error!(
    ie_hex_str_no_args,
    "a {\n  color: ie-hex-str();\n}\n", "Error: Missing argument $color."
);
error!(
    opacify_no_args,
    "a {\n  color: opacify();\n}\n", "Error: Missing argument $color."
);
error!(
    opacify_one_arg,
    "a {\n  color: opacify(red);\n}\n", "Error: Missing argument $amount."
);
error!(
    transparentize_no_args,
    "a {\n  color: transparentize();\n}\n", "Error: Missing argument $color."
);
error!(
    transparentize_one_arg,
    "a {\n  color: transparentize(red);\n}\n", "Error: Missing argument $amount."
);
error!(
    adjust_color_sl_and_wb,
    "a {\n  color: adjust-color(red, $saturation: 50%, $whiteness: 50%);\n}\n",
    "Error: HSL parameters may not be passed along with HWB parameters."
);
error!(
    adjust_color_rgb_and_sl,
    "a {\n  color: adjust-color(red, $red: 50%, $saturation: 50%);\n}\n",
    "Error: RGB parameters may not be passed along with HSL parameters."
);
error!(
    adjust_color_rgb_and_wb,
    "a {\n  color: adjust-color(red, $red: 50%, $whiteness: 50%);\n}\n",
    "Error: RGB parameters may not be passed along with HWB parameters."
);
error!(
    adjust_color_two_unknown_named_args,
    "a {\n  color: adjust-color(red, $foo: 50%, $bar: 50%);\n}\n",
    "Error: No arguments named $foo or $bar."
);
error!(
    adjust_color_two_positional_args,
    "a {\n  color: adjust-color(red, 50%);\n}\n",
    "Error: Only one positional argument is allowed. All other arguments must be passed by name."
);
error!(
    adjust_color_no_args,
    "a {\n  color: adjust-color();\n}\n", "Error: Missing argument $color."
);
error!(
    mix_weight_nan,
    "a {\n  color: mix(red, blue, (0/0));\n}\n",
    "Error: $weight: Expected calc(NaN) to be within 0 and 100."
);
error!(
    mix_weight_infinity,
    "a {\n  color: mix(red, blue, (1/0));\n}\n",
    "Error: $weight: Expected calc(infinity) to be within 0 and 100."
);

// color.mix() with $method parameter
test!(
    mix_method_xyz,
    "@use \"sass:color\";\na {\n  color: color.mix(red, green, $method: xyz);\n}\n",
    "a {\n  color: rgb(187.5160306784, 92.3735312967, 0);\n}\n"
);
test!(
    mix_method_oklch_shorter_hue,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 0.1 30), oklch(0.5 0.1 230), $method: oklch);\n}\n",
    "a {\n  color: oklch(50% 0.1 310deg);\n}\n"
);
test!(
    mix_method_oklch_longer_hue,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 0.1 30), oklch(0.5 0.1 230), $method: oklch longer hue);\n}\n",
    "a {\n  color: oklch(50% 0.1 130deg);\n}\n"
);
test!(
    mix_method_oklch_explicit_shorter,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 0.1 30), oklch(0.5 0.1 230), $method: oklch shorter hue);\n}\n",
    "a {\n  color: oklch(50% 0.1 310deg);\n}\n"
);
test!(
    mix_method_missing_channel_one_none,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 none 30), oklch(0.5 0.1 230), $method: oklch);\n}\n",
    "a {\n  color: oklch(50% 0.1 310deg);\n}\n"
);
test!(
    mix_method_missing_channel_both_none,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 none 30), oklch(0.5 none 230), $method: oklch);\n}\n",
    "a {\n  color: oklch(50% none 310deg);\n}\n"
);
test!(
    mix_method_alpha,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 0.1 30 / 0.4), oklch(0.8 0.2 200 / 0.8), $method: oklch);\n}\n",
    "a {\n  color: oklch(70% 0.1666666667 115deg / 0.6);\n}\n"
);
test!(
    mix_method_weight_25,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 0.1 30), oklch(0.5 0.1 230), 25%, $method: oklch);\n}\n",
    "a {\n  color: oklch(50% 0.1 270deg);\n}\n"
);
test!(
    mix_method_legacy_with_method,
    "@use \"sass:color\";\na {\n  color: color.mix(red, green, $method: oklch);\n}\n",
    "a {\n  color: hsl(42.7171999454, 267.6278571926%, 18.772156262%);\n}\n"
);
test!(
    mix_method_legacy_with_method_oklch_result,
    "@use \"sass:color\";\na {\n  color: color.mix(red, blue, $method: oklch);\n}\n",
    "a {\n  color: hsl(298.0621910541, 159.4931345486%, 29.2910601787%);\n}\n"
);
error!(
    mix_method_non_legacy_without_method,
    "@use \"sass:color\";\na {\n  color: color.mix(oklch(0.5 0.1 30), oklch(0.5 0.1 230));\n}\n",
    "Error: $color1: To use color.mix() with non-legacy color oklch(50% 0.1 30deg), you must provide a $method."
);
error!(
    mix_method_quoted_string,
    "@use \"sass:color\";\na {\n  color: color.mix(red, green, $method: \"oklch\");\n}\n",
    "Error: $method: Expected \"oklch\" to be an unquoted string."
);
error!(
    mix_method_unknown_space,
    "@use \"sass:color\";\na {\n  color: color.mix(red, green, $method: banana);\n}\n",
    "Error: $method: Unknown color space \"banana\"."
);
error!(
    mix_method_hue_on_rectangular,
    "@use \"sass:color\";\na {\n  color: color.mix(red, green, $method: lab shorter hue);\n}\n",
    "Error: $method: Hue interpolation method \"HueInterpolationMethod.shorter hue\" may not be set for rectangular color space lab."
);

// Out-of-range perceptual colors serialize as color-mix()
test!(
    oklch_out_of_range_lightness_color_mix,
    "@use \"sass:color\";\na {\n  b: color.change(oklch(50% 0.2 30deg), $lightness: 120%);\n}\n",
    "a {\n  b: color-mix(in oklch, color(xyz 2.0602077969 1.6344741917 1.0169248199) 100%, black);\n}\n"
);
test!(
    oklch_out_of_range_with_none_no_color_mix,
    "@use \"sass:color\";\na {\n  b: color.change(oklch(50% 0.2 none), $lightness: 120%);\n}\n",
    "a {\n  b: oklch(120% 0.2 none);\n}\n"
);
test!(
    lab_out_of_range_color_mix,
    "@use \"sass:color\";\na {\n  b: color.change(lab(50% 80 68), $lightness: 120%);\n}\n",
    "a {\n  b: color-mix(in lab, color(xyz 2.1723280023 1.5729564638 0.6281767308) 100%, black);\n}\n"
);
test!(
    lch_out_of_range_color_mix,
    "@use \"sass:color\";\na {\n  b: color.change(lch(50% 80 30deg), $lightness: 120%);\n}\n",
    "a {\n  b: color-mix(in lch, color(xyz 2.0867101966 1.5819797171 1.0030360544) 100%, black);\n}\n"
);
test!(
    oklab_out_of_range_color_mix,
    "@use \"sass:color\";\na {\n  b: color.change(oklab(0.5 0.2 0.1), $lightness: 120%);\n}\n",
    "a {\n  b: color-mix(in oklab, color(xyz 2.1300875486 1.6198708696 1.0050848824) 100%, black);\n}\n"
);
test!(
    display_p3_out_of_range_no_color_mix,
    "@use \"sass:color\";\na {\n  b: color.change(color(display-p3 0.5 0.5 0.5), $red: 1.5);\n}\n",
    "a {\n  b: color(display-p3 1.5 0.5 0.5);\n}\n"
);
test!(
    oklch_out_of_range_with_alpha_color_mix,
    "@use \"sass:color\";\na {\n  b: color.change(oklch(50% 0.2 30deg), $lightness: 120%, $alpha: 0.5);\n}\n",
    "a {\n  b: color-mix(in oklch, color(xyz 2.0602077969 1.6344741917 1.0169248199 / 0.5) 100%, black);\n}\n"
);
test!(
    is_in_gamut_oklch_out_of_range,
    "@use \"sass:color\";\na {\n  b: color.is-in-gamut(oklch(120% 0.2 30deg));\n}\n",
    "a {\n  b: true;\n}\n"
);
test!(
    mix_red_blue_fractional,
    "@use \"sass:color\";\na {\n  b: color.mix(red, blue);\n}\n",
    "a {\n  b: rgb(127.5, 0, 127.5);\n}\n"
);
test!(
    scale_fractional_rgb,
    "@use \"sass:color\";\na {\n  b: color.scale(#ffff00, $red: -50%);\n}\n",
    "a {\n  b: rgb(127.5, 255, 0);\n}\n"
);
test!(
    adjust_oklch_clamp_lightness,
    "@use \"sass:color\";\na {\n  b: color.adjust(oklch(90% 0.2 30), $lightness: 20%);\n}\n",
    "a {\n  b: oklch(100% 0.2 30deg);\n}\n"
);
// todo #194 item 2: `update_modern` previously used the raw numeric value for
// `$alpha` regardless of unit, so `$alpha: 50%` set alpha to 50 (clamped to 1)
// instead of scaling to 0.5, unlike dart-sass's `_changeColor`. Verified
// byte-identical against npx sass@1.97.3.
test!(
    change_modern_space_alpha_percent,
    "@use \"sass:color\";\na {\n  b: color.change(oklch(50% 0.1 200), $alpha: 50%);\n}\n",
    "a {\n  b: oklch(50% 0.1 200deg / 0.5);\n}\n"
);
test!(
    change_lab_alpha_percent,
    "@use \"sass:color\";\na {\n  b: color.change(lab(50% 20 20), $alpha: 25%);\n}\n",
    "a {\n  b: lab(50% 20 20 / 0.25);\n}\n"
);
// todo #199: `update_modern` never checked for the `none` keyword on `$alpha`,
// unlike the legacy path (`update_components`'s non-modern branch), so this
// errored with "$alpha: none is not a number." instead of setting alpha to
// missing. Verified byte-identical against npx sass@1.97.3.
test!(
    change_modern_space_alpha_none,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(oklch(50% 0.1 200), $alpha: none));\n}\n",
    "a {\n  b: oklch(50% 0.1 200deg / none);\n}\n"
);
test!(
    change_lab_alpha_none,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(lab(50% 20 30), $alpha: none));\n}\n",
    "a {\n  b: lab(50% 20 30 / none);\n}\n"
);
test!(
    change_legacy_rgb_alpha_none,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(rgb(10 20 30), $alpha: none));\n}\n",
    "a {\n  b: rgb(10 20 30 / none);\n}\n"
);
// dart-sass's `_changeColor` never guards on a missing alpha being modified —
// only `adjust()`/`scale()` do (via `_adjustChannel`/`_scaleChannel`). Verified
// against npx sass@1.97.3: neither `$alpha: none` nor a numeric `$alpha` on an
// already-missing-alpha color errors for `change()`.
test!(
    change_modern_space_alpha_none_on_already_missing_alpha,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(oklch(none 0.1 200 / none), $alpha: none));\n}\n",
    "a {\n  b: oklch(none 0.1 200deg / none);\n}\n"
);
test!(
    change_modern_space_alpha_numeric_on_already_missing_alpha,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(oklch(none 0.1 200 / none), $alpha: 0.5));\n}\n",
    "a {\n  b: oklch(none 0.1 200deg / 0.5);\n}\n"
);
// Confirms adjust()'s missing-alpha guard is unaffected by the change() fix above.
error!(
    adjust_modern_space_alpha_on_already_missing_alpha_still_errors,
    "@use \"sass:color\";\na {\n  b: color.adjust(oklch(none 0.1 200 / none), $alpha: 0.1);\n}\n",
    "Error: $alpha: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: oklch(none 0.1 200deg / none))."
);

// todo #223: dart-sass's unified `_changeColor` (used for both legacy and
// modern color spaces) never guards missing/powerless CHANNELS either — only
// `_adjustChannel`/`_scaleChannel` (adjust()/scale()) do. grass's
// check_missing_channel (legacy) and update_modern's channel loop
// unconditionally errored on every update kind, including Change. All values
// below verified byte-identical against npx sass@1.97.3.
test!(
    change_legacy_rgb_missing_red_channel,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(rgb(none 20 30), $red: 100));\n}\n",
    "a {\n  b: #64141e;\n}\n"
);
test!(
    change_legacy_hsl_missing_hue_channel,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(hsl(none 50% 50%), $hue: 100));\n}\n",
    "a {\n  b: hsl(100, 50%, 50%);\n}\n"
);
test!(
    change_modern_oklch_missing_lightness_channel,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(oklch(none 0.1 200), $lightness: 60%));\n}\n",
    "a {\n  b: oklch(60% 0.1 200deg);\n}\n"
);
// Changing a DIFFERENT channel than the missing one: the missing channel
// survives into the output as `none`, matching dart exactly.
test!(
    change_legacy_rgb_missing_channel_survives_when_other_changed,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(rgb(none 20 30), $green: 100));\n}\n",
    "a {\n  b: rgb(none 100 30);\n}\n"
);
test!(
    change_legacy_hsl_missing_hue_survives_when_lightness_changed,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(hsl(none 50% 50%), $lightness: 80%));\n}\n",
    "a {\n  b: hsl(none 50% 80%);\n}\n"
);
test!(
    change_modern_oklch_missing_chroma_survives_when_lightness_changed,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(oklch(0.5 none 200), $lightness: 60%));\n}\n",
    "a {\n  b: oklch(60% none 200deg);\n}\n"
);
test!(
    change_legacy_hwb_missing_hue_survives_when_blackness_changed,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(hwb(none 10% 10%), $blackness: 50%));\n}\n",
    "a {\n  b: hwb(none 10% 50%);\n}\n"
);
// Legacy alpha: `check_missing_channel`'s alpha branch also unconditionally
// errored for Change; now scoped to Adjust/Scale like the modern path (#199).
test!(
    change_legacy_rgb_missing_alpha_numeric,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(rgb(255 0 0 / none), $alpha: 0.5));\n}\n",
    "a {\n  b: rgba(255, 0, 0, 0.5);\n}\n"
);
// dart-sass's `_changeColor` reads an untouched, non-`$alpha`-provided alpha
// via `color.alpha`, which is `alphaOrNull ?? 0` — unlike
// `_adjustChannel`/`_scaleChannel`, which read `color.alphaOrNull` and so
// preserve a missing alpha. So `change()` on a color with a missing alpha,
// where `$alpha` itself is not passed, coerces the untouched alpha to 0
// rather than leaving it `none`. This is a dart-sass quirk, not a grass
// choice — verified against npx sass@1.97.3.
test!(
    change_legacy_rgb_untouched_missing_alpha_defaults_to_zero,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(rgb(0 0 0 / none), $red: 10));\n}\n",
    "a {\n  b: rgba(10, 0, 0, 0);\n}\n"
);
test!(
    change_legacy_hsl_untouched_missing_alpha_defaults_to_zero,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(hsl(0 0% 0% / none), $hue: 10));\n}\n",
    "a {\n  b: hsla(10, 0%, 0%, 0);\n}\n"
);
test!(
    change_modern_oklch_untouched_missing_alpha_defaults_to_zero,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(oklch(none 0.1 200 / none), $lightness: 60%));\n}\n",
    "a {\n  b: oklch(60% 0.1 200deg / 0);\n}\n"
);
// A previously-missing HSL literal (constructed via the CSS Color 4 path
// because it contains `none`) must still serialize as hsl() — not rgb() with
// fractional values — once change() fills in the missing channel. Regression
// test for a format-tag bug this fix unmasked: `hsl(none 50% 50%)` never got
// tagged `ColorFormat::Hsl` (only literal hsl()/hsla() without `none` does,
// via `from_hsla_fn`), so once the missing-channel guard above stopped
// blocking this call, the result fell through to rgb() fractional output.
test!(
    change_legacy_hsl_missing_hue_channel_keeps_hsl_format,
    "@use \"sass:color\";\n@use \"sass:meta\";\na {\n  b: meta.inspect(color.change(hsl(0 0% 0% / none), $hue: 10));\n}\n",
    "a {\n  b: hsla(10, 0%, 0%, 0);\n}\n"
);
// Controls: adjust()/scale() must still error on a missing channel — only
// change() gets the exemption.
error!(
    adjust_legacy_rgb_missing_red_channel_still_errors,
    "@use \"sass:color\";\na {\n  b: color.adjust(rgb(none 20 30), $red: 10);\n}\n",
    "Error: $red: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: rgb(none 20 30))."
);
error!(
    adjust_legacy_hsl_missing_hue_channel_still_errors,
    "@use \"sass:color\";\na {\n  b: color.adjust(hsl(none 50% 50%), $hue: 10);\n}\n",
    "Error: $hue: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: hsl(none 50% 50%))."
);
error!(
    scale_modern_oklch_missing_lightness_channel_still_errors,
    "@use \"sass:color\";\na {\n  b: color.scale(oklch(none 0.1 200), $lightness: 10%);\n}\n",
    "Error: $lightness: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: oklch(none 0.1 200deg))."
);

// Note: dart-sass outputs oklch(50% 0.8 30deg) directly, but grass uses
// color-mix() serialization for out-of-range perceptual values (separate issue)
test!(
    #[ignore = "out-of-range chroma serialization uses color-mix instead of oklch"]
    adjust_oklch_no_clamp_chroma,
    "@use \"sass:color\";\na {\n  b: color.adjust(oklch(50% 0.3 30), $chroma: 0.5);\n}\n",
    "a {\n  b: oklch(50% 0.8 30deg);\n}\n"
);
test!(
    adjust_lab_clamp_lightness,
    "@use \"sass:color\";\na {\n  b: color.adjust(lab(90 50 50), $lightness: 30);\n}\n",
    "a {\n  b: lab(100% 50 50);\n}\n"
);
test!(
    invert_hsl_preserves_format,
    "@use \"sass:color\";\na {\n  b: color.invert(hsl(200, 100%, 50%));\n}\n",
    "a {\n  b: hsl(20, 100%, 50%);\n}\n"
);
// dart-sass 1.97.3 verdict: explicit `$space: null` is NOT treated as omitted
// for color.change/adjust/scale — it errors, same as any other non-string value.
error!(
    change_explicit_null_space_errors,
    "@use \"sass:color\";\na {b: color.change(red, $lightness: 50%, $space: null)}",
    "Error: $space: null is not a string."
);
error!(
    adjust_explicit_null_space_errors,
    "@use \"sass:color\";\na {b: color.adjust(red, $lightness: 5%, $space: null)}",
    "Error: $space: null is not a string."
);
error!(
    scale_explicit_null_space_errors,
    "@use \"sass:color\";\na {b: color.scale(red, $lightness: 5%, $space: null)}",
    "Error: $space: null is not a string."
);
// xyz-d50 <-> lab/lch must convert directly (both are already in the D50 white
// point) rather than round-tripping through XYZ-D65. At extreme magnitudes that
// detour introduces enough FP noise to push a boundary-exact L of 0 negative,
// wrongly triggering the out-of-gamut color-mix() serialization fallback.
// Expected values verified against npx sass@1.97.3.
test!(
    to_space_xyz_d50_to_lab_extreme_magnitude,
    "@use \"sass:color\";\na {b: color.to-space(color(xyz-d50 -999999 0 0), lab)}",
    "a {\n  b: lab(0% -4037677156.674863 0);\n}\n"
);
test!(
    to_space_xyz_d50_to_lch_extreme_magnitude,
    "@use \"sass:color\";\na {b: color.to-space(color(xyz-d50 -999999 0 0), lch)}",
    "a {\n  b: lch(0% 4037677156.674863 180deg);\n}\n"
);
test!(
    to_space_lch_to_xyz_d50_round_trip,
    "@use \"sass:color\";\na {b: color.to-space(lch(69.4695307685% 4338.814723033 181.6020122751deg), xyz-d50)}",
    "a {\n  b: color(xyz-d50 -1 0.4 2);\n}\n"
);
// dart-sass's `SassColor._normalizeHue` renormalizes an Lch/OKLch hue with
// `(hue % 360 + 360) % 360`, not a plain `if h < 0 { h += 360 }`. The extra
// modulo shifts the last bit at extreme magnitudes, which both changes far
// out-of-gamut to-space() results and (as a side effect) makes a $space: lab
// round-trip of an OKLch color with hue 0deg bit-exact instead of drifting to
// 360deg. Expected values verified against npx sass@1.97.3 / sass-spec.
test!(
    to_space_hsl_to_lch_extreme_saturation,
    "@use \"sass:color\";\na {b: color.to-space(hsl(20deg 999999% 50%), lch)}",
    "a {\n  b: color-mix(in lch, color(xyz 136956388.67576775 59264689.51984791 -623200798.6134329) 100%, black);\n}\n"
);
test!(
    to_space_xyz_to_oklch_extreme_magnitude,
    "@use \"sass:color\";\na {b: color.to-space(color(xyz -999999 0 0), oklch)}",
    "a {\n  b: color-mix(in oklch, color(xyz -999998.9999999988 0 -0.0000000009) 100%, black);\n}\n"
);
test!(
    adjust_oklch_lab_space_noop_is_bit_exact,
    "@use \"sass:color\";\na {b: color.adjust(oklch(50% 0.2 0deg), $space: lab)}",
    "a {\n  b: oklch(50% 0.2 0deg);\n}\n"
);
test!(
    change_oklch_lab_space_noop_is_bit_exact,
    "@use \"sass:color\";\na {b: color.change(oklch(50% 0.2 0deg), $space: lab)}",
    "a {\n  b: oklch(50% 0.2 0deg);\n}\n"
);
test!(
    scale_oklch_lab_space_no_channels_is_bit_exact,
    "@use \"sass:color\";\na {b: color.scale(oklch(50% 0.2 0deg), $space: lab)}",
    "a {\n  b: oklch(50% 0.2 0deg);\n}\n"
);
// dart-sass computes an out-of-gamut Lch/OKLch hue in TWO separate stages that
// are not equivalent in IEEE double arithmetic: labToLch's own conditional
// `hue + 360` for a negative angle, then an independent second renormalization
// `(hue % 360 + 360) % 360` when the color is actually constructed. Collapsing
// these into a single normalization (as an earlier version of this fix did)
// still lands 1 ULP off from dart-sass at extreme magnitudes. Expected values
// verified against sass-spec / npx sass@1.97.3.
test!(
    to_space_rgb_to_lch_extreme_magnitude,
    "@use \"sass:color\";\na {b: color.to-space(color.change(black, $red: -999999), lch)}",
    "a {\n  b: color-mix(in lch, color(xyz -152693379.43919504 -78732523.77333494 -7157502.161212466) 100%, black);\n}\n"
);
test!(
    to_space_a98_rgb_to_lch_extreme_magnitude,
    "@use \"sass:color\";\na {b: color.to-space(color(a98-rgb -999999 0 0), lch)}",
    "a {\n  b: color-mix(in lch, color(xyz -9041452038524.758 -4661998707364.329 -423818064305.86096) 100%, black);\n}\n"
);
// OKLab/OKLCH <-> Lab/LCH/XYZ-D50 must convert directly via the LMS<->XYZ-D50
// matrices (dart-sass's LmsColorSpace/XyzD50ColorSpace special-case this),
// not via an XYZ-D65 round trip. Expected values verified against sass-spec.
test!(
    to_space_oklab_to_lab_extreme_magnitude,
    "@use \"sass:color\";\na {b: color.to-space(oklab(50% -999999 0), lab)}",
    "a {\n  b: color-mix(in lab, color(xyz -76837317949857280 3783158056963294.5 5396109066377520) 100%, black);\n}\n"
);
test!(
    to_space_xyz_d50_to_oklab_extreme_magnitude,
    "@use \"sass:color\";\na {b: color.to-space(color(xyz-d50 -999999 0 0), oklab)}",
    "a {\n  b: color-mix(in oklab, color(xyz -955472.4660146532 28369.6809641542 -12314.0025504671) 100%, black);\n}\n"
);
