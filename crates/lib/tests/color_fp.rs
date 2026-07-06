#[macro_use]
mod macros;

// Plan 039 (todo #180): OKLCH -> OKLAB was taking a direct (chroma, hue) ->
// (a, b) shortcut in `conversion::convert_direct`. dart-sass has no such
// shortcut for this specific pair: `OklchColorSpace.convert` always computes
// a/b from chroma/hue and delegates uniformly to `OklabColorSpace.convert`,
// which only special-cases `dest == oklch` (a direct `labToLch` shortcut).
// For any other dest -- including oklab itself, when reached via oklch -- it
// falls through to the generic LMS path: cube the oklab-space values into
// raw LMS (`pow(oklabToLms . [L,a,b], 3)`), then (via `LmsColorSpace.convert`'s
// `case oklab`) cube-root them back and reapply the inverse matrix. That
// round trip is not a floating-point no-op, so grass's shortcut diverged from
// dart-sass by several ULPs at extreme chroma. The reverse direction
// (OKLAB -> OKLCH) keeps its direct shortcut: `OklabColorSpace.convert`
// really does special-case `dest == oklch` with no round trip.
// Expectations verified byte-for-byte against `npx sass@1.97.3 --stdin` and
// match the checked-in sass-spec fixture
// core_functions/color/to_space/oklch/oklab.hrx::out_of_range/far.
test!(
    oklch_to_oklab_extreme_chroma_matches_dart_lms_roundtrip,
    "@use \"sass:color\";\na {b: color.to-space(oklch(10% 999999 0deg), oklab)}",
    "a {\n  b: oklab(9.9999999976% 999998.9999999992 0);\n}\n"
);
test!(
    oklch_to_oklab_zero_chroma_roundtrip_is_exact,
    "@use \"sass:color\";\na {b: color.to-space(oklch(50% 0 0deg), oklab)}",
    "a {\n  b: oklab(50% 0 0);\n}\n"
);

// Plan 041 (todo #183, root cause from Plan 039's todo #180 report): dart-sass
// computes HSL saturation as `100 * (max - lightness) / min(lightness, 1 -
// lightness)` and HWB blackness as `100 - max * 100` -- scaling INSIDE the
// formula, before the division/subtraction. grass's srgb_to_hsl/srgb_to_hwb
// instead divided/subtracted first and left the *100 scaling to callers,
// a different floating-point association that diverged from dart in the low
// digits at ordinary magnitudes. Fixed by transcribing dart's operation order
// inside conversion::srgb_to_hsl/srgb_to_hwb (dividing the result back by 100
// once, to preserve the functions' existing 0-1 return contract). Verified
// byte-for-byte against `npx sass@1.97.3 --stdin` and the checked-in fixture
// core_functions/color/to_space/lab/hwb.hrx::missing/lightness.
test!(
    lab_to_hwb_missing_lightness_matches_dart_saturation_association,
    "@use \"sass:color\";\na {b: color.to-space(lab(none 20 30), hwb)}",
    "a {\n  b: hsl(17.5913578322, 6051.6428880587%, 0.2688304082%);\n}\n"
);

// Normal-magnitude hsl()/hwb() controls, unaffected by the association fix
// above (verified byte-identical to npx sass@1.97.3 before and after).
test!(
    hsl_normal_magnitude_control_unaffected,
    "a {b: hsl(210, 50%, 40%);}",
    "a {\n  b: hsl(210, 50%, 40%);\n}\n"
);
test!(
    hwb_normal_magnitude_control_unaffected,
    "a {b: hwb(210 20% 30%);}",
    "a {\n  b: hsl(210, 55.5555555556%, 45%);\n}\n"
);

// Plan 051 (todo #192, root cause from Plan 046's todo #189 report): these 8
// tests were blocked purely by `write_float`'s rounding-algorithm mismatch
// (see number.rs's `precision_carry_ripples_*` tests for the mechanism), not
// by anything in the color conversion pipeline -- Plan 046 traced this
// family's arithmetic as bit-identical to dart-sass at every stage up to
// serialization. Expectations are the checked-in sass-spec fixtures, listed
// alongside each test.
//
// core_functions/color/to_space/prophoto_rgb/hsl.hrx::out_of_range/near
test!(
    prophoto_rgb_to_hsl_out_of_range_near,
    "@use \"sass:color\";\na {b: color.to-space(color(prophoto-rgb -1 0.4 2), hsl)}",
    "a {\n  b: hsl(199.2935266227, 2154.1559841675%, 8.1167706475%);\n}\n"
);
// core_functions/color/to_space/prophoto_rgb/hwb.hrx::out_of_range/near
test!(
    prophoto_rgb_to_hwb_out_of_range_near,
    "@use \"sass:color\";\na {b: color.to-space(color(prophoto-rgb -1 0.4 2), hwb)}",
    "a {\n  b: hsl(199.2935266227, 2154.1559841675%, 8.1167706475%);\n}\n"
);
// core_functions/color/to_space/prophoto_rgb/rgb.hrx::out_of_range/near
test!(
    prophoto_rgb_to_rgb_out_of_range_near,
    "@use \"sass:color\";\na {b: color.to-space(color(prophoto-rgb -1 0.4 2), rgb)}",
    "a {\n  b: hsl(199.2935266227, 2154.1559841675%, 8.1167706475%);\n}\n"
);
// core_functions/color/to_space/hsl/prophoto_rgb.hrx::out_of_range/far
test!(
    hsl_to_prophoto_rgb_out_of_range_far,
    "@use \"sass:color\";\na {b: color.to-space(hsl(20deg 999999% 50%), prophoto-rgb)}",
    "a {\n  b: color(prophoto-rgb 45494.0440115899 5344.0720850434 -73058.7852099565);\n}\n"
);
// core_functions/color/to_space/rec2020/display_p3.hrx::out_of_range/far
test!(
    rec2020_to_display_p3_out_of_range_far,
    "@use \"sass:color\";\na {b: color.to-space(color(rec2020 -999999 0 0), display-p3)}",
    "a {\n  b: color(display-p3 -392808.6781006625 111415.2873247036 -30092.3347141782);\n}\n"
);
// core_functions/color/to_space/srgb/display_p3.hrx::out_of_range/far
test!(
    srgb_to_display_p3_out_of_range_far,
    "@use \"sass:color\";\na {b: color.to-space(color(srgb -999999 0 0), display-p3)}",
    "a {\n  b: color(display-p3 -921788.227771966 -241977.733146743 -183469.5263235596);\n}\n"
);
// core_functions/color/to_space/xyz_d50/hwb.hrx::out_of_range/far
test!(
    xyz_d50_to_hwb_out_of_range_far,
    "@use \"sass:color\";\na {b: color.to-space(color(xyz-d50 -999999 0 0), hwb)}",
    "a {\n  b: hsl(329.431996419, 420.4439814741%, -10316.9080915763%);\n}\n"
);
// core_functions/color/to_space/display_p3_linear/lab.hrx::out_of_range/far
test!(
    display_p3_linear_to_lab_out_of_range_far,
    "@use \"sass:color\";\na {b: color.to-space(color(display-p3-linear -999999 0 0), lab)}",
    "a {\n  b: color-mix(in lab, color(xyz -486570.4620772619 -228974.3350951829 0.0000001214) 100%, black);\n}\n"
);
