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
