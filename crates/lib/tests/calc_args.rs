#[macro_use]
mod macros;

test!(
    arg_is_binop,
    "@use \"sass:meta\";

    a {
        color: meta.calc-args(calc(1vh + 1px));
    }",
    "a {\n  color: 1vh + 1px;\n}\n"
);

// Expectations verified with npx -y sass@1.101.0.
test!(
    interpolated_calc_parentheses_follow_precedence,
    "@function units($x) {
      @if $x == 2.5 { @return 2.5rem; }
      @if $x == 0.25 { @return 0.25rem; }
    }
    $v: calc((#{units(2.5)} / 2) - #{units(0.25)});
    a { x: calc($v * 2); }",
    "a {\n  x: calc((2.5rem / 2 - 0.25rem) * 2);\n}\n"
);

// Expectations verified with npx -y sass@1.101.0.
test!(
    interpolated_multiline_calc_whitespace,
    "@function units($x) {
      @if $x == 2.5 { @return 2.5rem; }
      @if $x == 0.5 { @return 0.5rem; }
      @if $x == 0.25 { @return 0.25rem; }
    }
    $v: calc(
      (
          (
              #{units(2.5)} -
                #{units(0.5)}
            ) /
            2
        ) +
        #{units(0.25)}
    );
    a { x: $v; }",
    "a {\n  x: calc((2.5rem - 0.5rem) / 2 + 0.25rem);\n}\n"
);
