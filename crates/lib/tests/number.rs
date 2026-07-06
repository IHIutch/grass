#[macro_use]
mod macros;

// this is `1` for node-sass, but .999999etc for web compiler
test!(
    precision_does_not_round_up,
    "a {\n  color: 0.99999999991;\n}\n",
    "a {\n  color: 0.9999999999;\n}\n"
);
// this is `1` for node-sass, but .999999etc for web compiler
test!(
    precision_does_round_up,
    "a {\n  color: 1.00000000009;\n}\n",
    "a {\n  color: 1.0000000001;\n}\n"
);
// Plan 051 (todo #192, root cause from Plan 046's todo #189 report): dart-sass's
// `_writeRounded` (serialize.dart:1237-1314) rounds half-up on the shortest-
// round-trip STRING's 11th fractional digit, not on the double's exact binary
// expansion -- so a carry can ripple all the way into the integer part (dart's
// own comment gives exactly this shape). `format!("{:.10}", num)` rounds the
// exact binary value instead and would print `9.9999999999` here, one part in
// 1e-10 short of the carry. Verified against `npx sass@1.97.3 --stdin`.
test!(
    precision_carry_ripples_into_integer_part,
    "a {\n  color: 9.99999999995;\n}\n",
    "a {\n  color: 10;\n}\n"
);
test!(
    precision_carry_ripples_into_integer_part_negative,
    "a {\n  color: -9.99999999995;\n}\n",
    "a {\n  color: -10;\n}\n"
);
// Carry ripples through the single leading "0" digit rather than adding a
// new integer digit -- a distinct code path in the digit-array algorithm
// (the loop's carry terminates at `digits[0]` itself, rather than passing
// through it as in the case above).
test!(
    precision_carry_ripples_through_leading_zero,
    "a {\n  color: 0.99999999995;\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    precision_carry_ripples_through_leading_zero_negative,
    "a {\n  color: -0.99999999995;\n}\n",
    "a {\n  color: -1;\n}\n"
);
test!(
    many_nines_becomes_one,
    "a {\n  color: 0.9999999999999999;\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    many_nines_becomes_one_neg,
    "a {\n  color: -0.9999999999999999;\n}\n",
    "a {\n  color: -1;\n}\n"
);
test!(
    negative_zero,
    "a {\n  color: -0;\n}\n",
    "a {\n  color: 0;\n}\n"
);
test!(
    decimal_is_zero,
    "a {\n  color: 1.0000;\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    unary_plus_on_integer,
    "a {\n  color: +1;\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    unary_plus_on_decimal,
    "a {\n  color: +1.5;\n}\n",
    "a {\n  color: 1.5;\n}\n"
);
test!(
    unary_plus_on_scientific,
    "a {\n  color: +1e5;\n}\n",
    "a {\n  color: 100000;\n}\n"
);
test!(
    many_nines_not_rounded,
    "a {\n  color: 0.999999;\n}\n",
    "a {\n  color: 0.999999;\n}\n"
);
test!(
    positive_integer,
    "a {\n  color: 1;\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    negative_integer,
    "a {\n  color: -1;\n}\n",
    "a {\n  color: -1;\n}\n"
);
test!(
    positive_float_no_leading_zero,
    "a {\n  color: .1;\n}\n",
    "a {\n  color: 0.1;\n}\n"
);
test!(
    negative_float_no_leading_zero,
    "a {\n  color: -.1;\n}\n",
    "a {\n  color: -0.1;\n}\n"
);
test!(
    positive_float_leading_zero,
    "a {\n  color: 0.1;\n}\n",
    "a {\n  color: 0.1;\n}\n"
);
test!(
    negative_float_leading_zero,
    "a {\n  color: -0.1;\n}\n",
    "a {\n  color: -0.1;\n}\n"
);
test!(
    negative_near_zero_no_sign,
    "a {\n  color: -0.000000000001;\n}\n",
    "a {\n  color: 0;\n}\n"
);
test!(
    equality_unit_conversions,
    "a {\n  color: 1in == 96px;\n}\n",
    "a {\n  color: true;\n}\n"
);
test!(
    positive_scientific_notation,
    "a {\n  color: 1e5;\n}\n",
    "a {\n  color: 100000;\n}\n"
);
test!(
    positive_scientific_notation_leading_zeroes,
    "a {\n  color: 1e05;\n}\n",
    "a {\n  color: 100000;\n}\n"
);
test!(
    positive_scientific_notation_capital,
    "a {\n  color: 1E5;\n}\n",
    "a {\n  color: 100000;\n}\n"
);
test!(
    negative_scientific_notation,
    "a {\n  color: 1e-5;\n}\n",
    "a {\n  color: 0.00001;\n}\n"
);
test!(
    negative_scientific_notation_leading_zeroes,
    "a {\n  color: 1e-05;\n}\n",
    "a {\n  color: 0.00001;\n}\n"
);
test!(
    negative_scientific_notation_capital,
    "a {\n  color: 1E-5;\n}\n",
    "a {\n  color: 0.00001;\n}\n"
);
test!(
    positive_scientific_notation_decimal,
    "a {\n  color: 1.2e5;\n}\n",
    "a {\n  color: 120000;\n}\n"
);
test!(
    negative_scientific_notation_decimal,
    "a {\n  color: 1.2e-5;\n}\n",
    "a {\n  color: 0.000012;\n}\n"
);
test!(unit_e, "a {\n  color: 1e;\n}\n", "a {\n  color: 1e;\n}\n");
test!(
    positive_scientific_notation_zero,
    "a {\n  color: 1e0;\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    negative_scientific_notation_zero,
    "a {\n  color: 1e-0;\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    scientific_notation_decimal,
    "a {\n  color: 1.2e5.5;\n}\n",
    "a {\n  color: 120000 0.5;\n}\n"
);
test!(
    binary_op_with_e_as_unit,
    "a {\n  color: 1e - 2;\n}\n",
    "a {\n  color: -1e;\n}\n"
);
error!(
    scientific_notation_nothing_after_dash_in_style,
    "a {\n  color: 1e-;\n}\n", "Error: Expected digit."
);
error!(
    scientific_notation_nothing_after_dash,
    "a {\n  color: 1e-", "Error: Expected digit."
);
error!(
    scientific_notation_whitespace_after_dash,
    "a {\n  color: 1e- 2;\n}\n", "Error: Expected digit."
);
error!(
    scientific_notation_ident_char_after_dash,
    "a {\n  color: 1e-a;\n}\n", "Error: Expected digit."
);
test!(
    number_overflow_from_addition,
    "a {\n  color: 999999999999999999
                + 999999999999999999
                + 999999999999999999
                + 999999999999999999
                + 999999999999999999
                + 999999999999999999
                + 999999999999999999
                + 999999999999999999
                + 999999999999999999
                + 999999999999999999;\n}\n",
    "a {\n  color: 10000000000000000000;\n}\n"
);
test!(
    number_overflow_from_multiplication,
    "a {\n  color: 999999999999999999 * 10;\n}\n",
    "a {\n  color: 10000000000000000000;\n}\n"
);
test!(
    number_overflow_from_division,
    "a {\n  color: (999999999999999999 / .1);\n}\n",
    "a {\n  color: 10000000000000000000;\n}\n"
);
test!(
    bigint_is_equal_to_smallint,
    "$a: 99999990000099999999999999 - 99999990000099999999999999;

    a {
      color: $a;
      color: $a == 0;
    }",
    "a {\n  color: 0;\n  color: true;\n}\n"
);
test!(
    scientific_notation_very_large_positive,
    "a {\n  color: 1e100;\n}\n", "a {\n  color: 10000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000;\n}\n"
);
test!(
    scientific_notation_very_large_negative,
    "a {\n  color: 1e-100;\n}\n",
    "a {\n  color: 0;\n}\n"
);
test!(
    overflows_float_positive,
    "a {\n  color: 1e999;\n}\n",
    "a {\n  color: calc(infinity);\n}\n"
);
test!(
    overflows_float_negative,
    "a {\n  color: -1e999;\n}\n",
    "a {\n  color: calc(-infinity);\n}\n"
);
test!(
    very_large_but_no_overflow,
    "a {\n  color: 17976931348623157000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000;\n}\n",
    "a {\n  color: 17976931348623158000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000;\n}\n"
);
error!(
    scientific_notation_no_number_after_decimal,
    "a {\n  color: 1.e3;\n}\n", "Error: Expected digit."
);

// Edge battery for Plan 023's stack-buffer number parsing (parse/value.rs
// `parse_number`); expectations verified byte-for-byte against
// `npx sass@1.97.3 --stdin --style=expanded`.
test!(
    edge_battery_large_exponent,
    "a {\n  b: 1e100;\n}\n",
    "a {\n  b: 10000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000;\n}\n"
);
test!(
    edge_battery_exact_integer_exponent,
    "a {\n  b: 1e15;\n}\n",
    "a {\n  b: 1000000000000000;\n}\n"
);
test!(
    edge_battery_decimal_with_exponent,
    "a {\n  b: 1.5e15;\n}\n",
    "a {\n  b: 1500000000000000;\n}\n"
);
test!(
    edge_battery_two_decimal_digits_with_exponent,
    "a {\n  b: 9.99e15;\n}\n",
    "a {\n  b: 9990000000000000;\n}\n"
);
test!(
    edge_battery_very_large_exponent,
    "a {\n  b: 5e300;\n}\n",
    "a {\n  b: 5000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000;\n}\n"
);
test!(
    edge_battery_leading_dot,
    "a {\n  b: .5;\n}\n",
    "a {\n  b: 0.5;\n}\n"
);
test!(
    edge_battery_small_decimal,
    "a {\n  b: 0.00001;\n}\n",
    "a {\n  b: 0.00001;\n}\n"
);
test!(
    edge_battery_negative_large_exponent,
    "a {\n  b: -1e20;\n}\n",
    "a {\n  b: -100000000000000000000;\n}\n"
);
test!(
    edge_battery_long_integer_part_with_decimal,
    "a {\n  b: 100000000000000.5;\n}\n",
    "a {\n  b: 100000000000000.5;\n}\n"
);
test!(
    edge_battery_many_significant_digits,
    "a {\n  b: 3.14159265358979;\n}\n",
    "a {\n  b: 3.1415926536;\n}\n"
);
test!(
    edge_battery_negative_exponent_underflow,
    "a {\n  b: 1e-45;\n}\n",
    "a {\n  b: 0;\n}\n"
);

// Battery for Plan 031's write_float fix (serializer.rs `write_float`):
// integer-valued doubles at extreme magnitude were previously rendered by
// reconstructing Rust's shortest-round-trip `{:e}` mantissa and zero-padding
// it out to the right magnitude, which is only correct when the double's
// true low-order digits happen to be zero. dart-sass instead prints the
// EXACT decimal integer the double represents whenever its native (64-bit)
// `int.round()` doesn't overflow — i.e. for |x| < 2^63 — and only falls back
// to a shortest-round-trip-plus-zero-padding strategy at/above 2^63, where
// dart's own `int.round()` saturates. Expectations below verified byte-for-
// byte against `npx sass@1.97.3 --stdin --style=expanded`, using `+0` to
// force numeric evaluation and a base literal safely under 2^53 (where
// grass and dart-sass's number literal parsers are already known to agree)
// multiplied by a small constant to reach each magnitude band, mirroring
// this plan's own probe battery.
test!(
    write_float_exact_integer_1e15,
    "a {\n  b: 1e15 + 0;\n}\n",
    "a {\n  b: 1000000000000000;\n}\n"
);
test!(
    write_float_exact_integer_1e16,
    "a {\n  b: 1e16 + 0;\n}\n",
    "a {\n  b: 10000000000000000;\n}\n"
);
test!(
    write_float_exact_integer_1e17,
    "a {\n  b: 1e17 + 0;\n}\n",
    "a {\n  b: 100000000000000000;\n}\n"
);
test!(
    write_float_exact_integer_negative_1e16,
    "a {\n  b: -1e16 + 0;\n}\n",
    "a {\n  b: -10000000000000000;\n}\n"
);
test!(
    write_float_2_53_boundary_below,
    "a {\n  b: 4503599627370497 + 0;\n}\n",
    "a {\n  b: 4503599627370497;\n}\n"
);
test!(
    write_float_2_53_boundary_at,
    "a {\n  b: 9007199254740992 + 0;\n}\n",
    "a {\n  b: 9007199254740992;\n}\n"
);
test!(
    write_float_2_53_boundary_above_rounds_down,
    "a {\n  b: 9007199254740993 + 0;\n}\n",
    "a {\n  b: 9007199254740992;\n}\n"
);
// `-4037681194356056 * 10`: this exact expression is Plan 031's probe
// battery witness (base literal `4037681194356056` < 2^53, scaled by a
// small integer multiplier to land in the (2^53, 2^63) band where the old
// zero-padding logic was first observed to diverge from dart-sass).
test!(
    write_float_mid_range_non_padded_digits,
    "a {\n  b: -4037681194356056 * 10;\n}\n",
    "a {\n  b: -40376811943560560;\n}\n"
);
// Beyond 2^63 (9223372036854775808), dart-sass's own `int.round()`
// saturates and it falls back to shortest-round-trip-plus-zero-padding;
// this is the OLD grass algorithm, still correct in this regime.
test!(
    write_float_beyond_i64_saturation_boundary,
    "a {\n  b: 1e21 + 0;\n}\n",
    "a {\n  b: 1000000000000000000000;\n}\n"
);
test!(
    write_float_beyond_i64_saturation_boundary_negative,
    "a {\n  b: -1e22 + 0;\n}\n",
    "a {\n  b: -10000000000000000000000;\n}\n"
);
// Real-world regression witness: this exact input is
// sass-spec's `core_functions/color/to_space/oklab/xyz_d50.hrx
// ::out_of_range/far` fixture (verified against that checked-in fixture,
// which is generated from a native dart-sass build — a live
// `npx sass@1.97.3` run of this same input was independently found to
// return `...694510` instead of `...694512`, a 1-ULP divergence traced to
// the dart2js-compiled build npx resolves to, not to this fixture or to
// write_float; see Plan 031's final report on todo #178 for detail). Before
// this fix, grass's zero-padding logic produced `...694510` here too,
// diverging from the fixture for the wrong (write_float) reason.
test!(
    write_float_oklab_to_xyz_d50_regression_witness,
    "@use \"sass:color\";\na {\n  b: color.to-space(oklab(50% -999999 0), xyz-d50);\n}\n",
    "a {\n  b: color(xyz-d50 -80704145963694512 1378316536921807 4824362248731981);\n}\n"
);
