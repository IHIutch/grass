#[macro_use]
mod macros;

test!(
    inspect_unquoted_string,
    "a {\n  color: inspect(foo)\n}\n",
    "a {\n  color: foo;\n}\n"
);
test!(
    inspect_dbl_quoted_string,
    "a {\n  color: inspect(\"foo\")\n}\n",
    "a {\n  color: \"foo\";\n}\n"
);
test!(
    inspect_sgl_quoted_string,
    "a {\n  color: inspect(\"foo\")\n}\n",
    "a {\n  color: \"foo\";\n}\n"
);
test!(
    inspect_unitless_number,
    "a {\n  color: inspect(1)\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    inspect_px_number,
    "a {\n  color: inspect(1px)\n}\n",
    "a {\n  color: 1px;\n}\n"
);
test!(
    inspect_color_3_hex,
    "a {\n  color: inspect(#fff)\n}\n",
    "a {\n  color: #fff;\n}\n"
);
test!(
    inspect_color_6_hex,
    "a {\n  color: inspect(#ffffff)\n}\n",
    "a {\n  color: #ffffff;\n}\n"
);
test!(
    inspect_color_name,
    "a {\n  color: inspect(red)\n}\n",
    "a {\n  color: red;\n}\n"
);
test!(
    inspect_true,
    "a {\n  color: inspect(true)\n}\n",
    "a {\n  color: true;\n}\n"
);
test!(
    inspect_false,
    "a {\n  color: inspect(false)\n}\n",
    "a {\n  color: false;\n}\n"
);
test!(
    inspect_null,
    "a {\n  color: inspect(null)\n}\n",
    "a {\n  color: null;\n}\n"
);
test!(
    inspect_empty_brackets,
    "a {\n  color: inspect([]);\n}\n",
    "a {\n  color: [];\n}\n"
);
test!(
    inspect_comma_separated_one_val,
    "a {\n  color: inspect((1, ));\n}\n",
    "a {\n  color: (1,);\n}\n"
);
test!(
    inspect_comma_separated_one_val_bracketed,
    "a {\n  color: inspect([1, ]);\n}\n",
    "a {\n  color: [1,];\n}\n"
);
test!(
    inspect_space_separated_one_val_bracketed,
    "a {\n  color: inspect(append((), 1, space));\n}\n",
    "a {\n  color: 1;\n}\n"
);
test!(
    inspect_list_of_empty_list,
    "a {\n  color: inspect(((), ()));\n}\n",
    "a {\n  color: (), ();\n}\n"
);
test!(
    inspect_comma_separated_list_of_comma_separated_lists,
    "a {\n  color: inspect([(1, 2), (3, 4)]);\n}\n",
    "a {\n  color: [(1, 2), (3, 4)];\n}\n"
);
test!(
    inspect_map_with_bracketed_key_and_value,
    "a {\n  color: inspect(([a, b]: [c, d]));\n}\n",
    "a {\n  color: ([a, b]: [c, d]);\n}\n"
);
test!(
    inspect_map_with_comma_separated_key_and_value,
    "a {\n  color: inspect(((a, b): (c, d)));\n}\n",
    "a {\n  color: ((a, b): (c, d));\n}\n"
);
test!(
    inspect_slash_list_singleton,
    "a {\n  color: inspect(join((a,), (), slash));\n}\n",
    "a {\n  color: (a/);\n}\n"
);
test!(
    inspect_empty_list,
    "a {\n  color: inspect(())\n}\n",
    "a {\n  color: ();\n}\n"
);
test!(
    inspect_spaced_list,
    "a {\n  color: inspect(1 2 3)\n}\n",
    "a {\n  color: 1 2 3;\n}\n"
);
error!(
    inspect_comma_list,
    "a {\n  color: inspect(1, 2, 3)\n}\n", "Error: Only 1 argument allowed, but 3 were passed."
);
test!(
    inspect_parens,
    "a {\n  color: inspect((((a))));\n}\n",
    "a {\n  color: a;\n}\n"
);
// npx-verified against dart-sass 1.97.3: complex-unit numbers render wrapped
// in `calc(...)` under `inspect`/`meta.inspect`/`@debug`, same as normal CSS
// property-value serialization — dart's `visitNumber` does not special-case
// inspect mode for this. A single unit (no complex units) is unaffected.
test!(
    inspect_mul_units,
    "a {\n  color: inspect(1em * 1px);\n}\n",
    "a {\n  color: calc(1em * 1px);\n}\n"
);
test!(
    inspect_single_unit_not_wrapped,
    "a {\n  color: inspect(1px);\n}\n",
    "a {\n  color: 1px;\n}\n"
);
test!(
    inspect_mul_three_units,
    "a {\n  color: inspect(1px * 1em * 1s);\n}\n",
    "a {\n  color: calc(1px * 1em * 1s);\n}\n"
);
test!(
    inspect_div_units_denominator_only,
    "@use \"sass:math\";\na {\n  color: inspect(math.div(1, 1s));\n}\n",
    "a {\n  color: calc(1 / 1s);\n}\n"
);
test!(
    inspect_div_units_numerator_and_denominator,
    "@use \"sass:math\";\na {\n  color: inspect(math.div(1px, 1s));\n}\n",
    "a {\n  color: calc(1px / 1s);\n}\n"
);
test!(
    inspect_div_units_multiple_denominators,
    "@use \"sass:math\";\na {\n  color: inspect(math.div(1px, 1s * 1s));\n}\n",
    "a {\n  color: calc(1px / 1s / 1s);\n}\n"
);
test!(
    inspect_mul_and_div_units,
    "@use \"sass:math\";\na {\n  color: inspect(math.div(1px * 1em, 1s));\n}\n",
    "a {\n  color: calc(1px * 1em / 1s);\n}\n"
);
test!(
    inspect_map_with_map_key_and_value,
    "a {\n  color: inspect(((a: b): (c: d)));\n}\n",
    "a {\n  color: ((a: b): (c: d));\n}\n"
);
test!(
    inspect_map_in_arglist,
    "@function foo($a...) {
        @return inspect($a);
    }

    a {
        color: foo((a: b));
    }",
    "a {\n  color: ((a: b),);\n}\n"
);
