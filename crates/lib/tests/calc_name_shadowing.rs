#[macro_use]
mod macros;

// User-defined/module functions shadow CSS math-function names (min, max,
// sqrt, round, atan2, calc-size, ...) at evaluation time, matching dart-sass's
// `getFunction`-then-switch order in `visitFunctionExpression`
// (lib/src/visitor/async_evaluate.dart:3042). All expectations are verified
// against `npx sass@1.101.0 --stdin --style=expanded`.

test!(
    min_overridden_by_user_function,
    "@function min($a, $b) { @return overridden; }\na { b: min(1, 2); }\n",
    "a {\n  b: overridden;\n}\n"
);
test!(
    max_overridden_by_user_function,
    "@function max($a, $b) { @return overridden; }\na { b: max(1, 2); }\n",
    "a {\n  b: overridden;\n}\n"
);
test!(
    sqrt_overridden_by_user_function,
    "@function sqrt($a) { @return overridden; }\na { b: sqrt(4); }\n",
    "a {\n  b: overridden;\n}\n"
);
test!(
    atan2_overridden_by_user_function,
    "@function atan2($a, $b) { @return overridden; }\na { b: atan2(1, 2); }\n",
    "a {\n  b: overridden;\n}\n"
);
test!(
    calc_size_overridden_by_user_function,
    "@function calc-size($a, $b) { @return overridden; }\na { b: calc-size(auto, size + 1px); }\n",
    "a {\n  b: overridden;\n}\n"
);
test!(
    round_one_arg_overridden_by_user_function,
    "@function round($a) { @return overridden; }\na { b: round(1.5); }\n",
    "a {\n  b: overridden;\n}\n"
);
test!(
    round_two_arg_overridden_by_user_function,
    "@function round($a, $b) { @return overridden; }\na { b: round(1.5, 0.1); }\n",
    "a {\n  b: overridden;\n}\n"
);

// No override in scope: behavior is unchanged from plain calculation evaluation.
test!(
    min_not_overridden_is_unchanged,
    "a { b: min(1px, 2px); }\n",
    "a {\n  b: 1px;\n}\n"
);

// Namespaced calls (math.min) always resolve through the module's function
// map and never hit the calc-name shadowing path, so a same-named unqualified
// `@function min` has no effect on them.
test!(
    namespaced_math_min_unaffected_by_user_override,
    "@use \"sass:math\";\n@function min($a, $b) { @return overridden; }\na { b: math.min(1, 2); }\n",
    "a {\n  b: 1;\n}\n"
);

// A rest-arg list isn't calc-safe grammar, so with no override this still
// falls through to the global list-capable min() builtin, unaffected by the
// shadowing machinery.
test!(
    min_with_list_arg_no_override_hits_global_builtin,
    "$list: 3px 1px 2px;\na { b: min($list...); }\n",
    "a {\n  b: 1px;\n}\n"
);

// The three-way precedence (user override > calculation > global list
// builtin) puts the override first regardless of argument shape.
test!(
    min_with_list_arg_and_override_hits_override,
    "@function min($args...) { @return overridden; }\n$list: 3px 1px 2px;\na { b: min($list...); }\n",
    "a {\n  b: overridden;\n}\n"
);

// Shadowing applies recursively to nested calc-name calls, since dart-sass
// re-enters `visitFunctionExpression` (and thus the getFunction check) for
// every nested FunctionExpression inside a calculation.
test!(
    nested_calc_name_call_overridden,
    "@function max($a, $b) { @return nested-overridden; }\na { b: min(max(1, 2), 3); }\n",
    "a {\n  b: min(nested-overridden, 3);\n}\n"
);

// calc() and clamp() are also shadowable as of dart-sass 1.99.0.
test!(
    calc_overridden_by_user_function,
    "@function calc() { @return overridden; }\na { b: calc(); }\n",
    "a {\n  b: overridden;\n}\n"
);
test!(
    calc_with_argument_overridden_by_user_function,
    "@function calc($a) { @return $a * 2; }\na { b: calc(3); }\n",
    "a {\n  b: 6;\n}\n"
);
test!(
    clamp_overridden_by_user_function,
    "@function clamp() { @return overridden; }\na { b: clamp(); }\n",
    "a {\n  b: overridden;\n}\n"
);
