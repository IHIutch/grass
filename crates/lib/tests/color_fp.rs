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
