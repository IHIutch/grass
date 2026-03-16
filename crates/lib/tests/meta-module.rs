use std::io::Write;

#[macro_use]
mod macros;

test!(
    #[ignore = "function ordering differs from dart-sass"],
    module_functions_builtin,
    "@use 'sass:meta';\na {\n  color: inspect(meta.module-functions(meta));\n}\n",
    "a {\n  color: (\"module-functions\": get-function(\"module-functions\"), \"inspect\": get-function(\"inspect\"), \"feature-exists\": get-function(\"feature-exists\"), \"type-of\": get-function(\"type-of\"), \"keywords\": get-function(\"keywords\"), \"global-variable-exists\": get-function(\"global-variable-exists\"), \"variable-exists\": get-function(\"variable-exists\"), \"function-exists\": get-function(\"function-exists\"), \"mixin-exists\": get-function(\"mixin-exists\"), \"content-exists\": get-function(\"content-exists\"), \"module-variables\": get-function(\"module-variables\"), \"get-function\": get-function(\"get-function\"), \"call\": get-function(\"call\"), \"calc-args\": get-function(\"calc-args\"), \"calc-name\": get-function(\"calc-name\"), \"get-mixin\": get-function(\"get-mixin\"), \"module-mixins\": get-function(\"module-mixins\"), \"accepts-content\": get-function(\"accepts-content\"));\n}\n"
);
test!(
    module_variables_builtin,
    "@use 'sass:meta';\n@use 'sass:math';\na {\n  color: inspect(map-get(meta.module-variables(math), 'e'));\n}\n",
    "a {\n  color: 2.7182818285;\n}\n"
);
test!(
    global_var_exists_module,
    "@use 'sass:math';\na {\n  color: global-variable-exists(pi, $module: math);\n}\n",
    "a {\n  color: true;\n}\n"
);
test!(
    fn_exists_builtin,
    "@use 'sass:math';\na {\n  color: function-exists(acos, $module: math);\n}\n",
    "a {\n  color: true;\n}\n"
);
error!(
    fn_exists_module_dne,
    "a {\n  color: function-exists(c, d);\n}\n",
    "Error: There is no module with the namespace \"d\"."
);

#[test]
fn mixin_exists_module() {
    let input = "@use \"mixin_exists_module\" as module;\na {\n color: mixin-exists(foo, $module: module);\n}";
    tempfile!("mixin_exists_module.scss", "@mixin foo {}");
    assert_eq!(
        "a {\n  color: true;\n}\n",
        &grass::from_string(input.to_string(), &grass::Options::default()).expect(input)
    );
}

#[test]
fn load_css_simple() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(load_css_simple);\n}";
    tempfile!("load_css_simple.scss", "a { color: red; }");
    assert_eq!(
        "a a {\n  color: red;\n}\n",
        &grass::from_string(input.to_string(), &grass::Options::default()).expect(input)
    );
}

#[test]
fn load_css_explicit_args() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css($url: load_css_explicit_args, $with: null);\n}";
    tempfile!("load_css_explicit_args.scss", "a { color: red; }");
    assert_eq!(
        "a a {\n  color: red;\n}\n",
        &grass::from_string(input.to_string(), &grass::Options::default()).expect(input)
    );
}

#[test]
fn load_css_non_string_url() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(2);\n}";
    tempfile!("load_css_non_string_url.scss", "a { color: red; }");
    assert_err!("Error: $url: 2 is not a string.", input);
}

#[test]
fn load_css_non_map_with() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(foo, 2);\n}";
    assert_err!("Error: $with: 2 is not a map.", input);
}

#[test]
fn load_css_with_single_variable() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(load_css_with_single_variable, $with: (\"a\": configured));\n}";
    tempfile!(
        "load_css_with_single_variable.scss",
        "$a: default !default;\nb { color: $a; }"
    );
    assert_eq!(
        "a b {\n  color: configured;\n}\n",
        &grass::from_string(input.to_string(), &grass::Options::default()).expect(input)
    );
}

#[test]
fn load_css_with_multiple_variables() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(load_css_with_multiple_variables, $with: (\"x\": 1, \"y\": 2));\n}";
    tempfile!(
        "load_css_with_multiple_variables.scss",
        "$x: default !default;\n$y: default !default;\nb { color: $x $y; }"
    );
    assert_eq!(
        "a b {\n  color: 1 2;\n}\n",
        &grass::from_string(input.to_string(), &grass::Options::default()).expect(input)
    );
}

#[test]
fn load_css_with_null_value() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(load_css_with_null_value, $with: (\"a\": null));\n}";
    tempfile!(
        "load_css_with_null_value.scss",
        "$a: default !default;\nb { color: $a; }"
    );
    assert_eq!(
        "a b {\n  color: default;\n}\n",
        &grass::from_string(input.to_string(), &grass::Options::default()).expect(input)
    );
}

#[test]
fn load_css_with_unconfigured_variable() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(load_css_with_unconfigured_variable, $with: (\"b\": value));\n}";
    tempfile!(
        "load_css_with_unconfigured_variable.scss",
        "$a: default !default;\nb { color: $a; }"
    );
    assert_err!(
        "Error: $b was not declared with !default in the @used module.",
        input
    );
}

#[test]
fn load_css_with_builtin_module() {
    let input = "@use \"sass:meta\";\na {\n @include meta.load-css(\"sass:color\", $with: (\"a\": value));\n}";
    assert_err!(
        "Error: Built-in module sass:color can't be configured.",
        input
    );
}
