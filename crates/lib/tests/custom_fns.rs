#![cfg(feature = "custom-builtin-fns")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use grass::{
    sass_value::{ArgumentResult, SassNumber, Value},
    Builtin, Options, Result as SassResult, Visitor,
};

// An example function that looks up the length of an array or map and adds 2 to it
fn length(mut args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let len = args.get_err(0, "list")?.as_list().len();

    Ok(Value::Dimension(SassNumber::new_unitless(len + 2)))
}

#[test]
fn custom_fn_reachable_through_grass_crate() {
    let options = Options::default().add_custom_fn("length", Builtin::new(length));
    let css = grass::from_string("a { color: length([a, b]); }".to_owned(), &options).unwrap();

    assert_eq!(css, "a {\n  color: 4;\n}\n");
}

#[test]
fn dynamic_fn_binds_named_args_to_declared_position() {
    // The closure receives args already bound to declared positions, so
    // `get_err(0, ..)`/`get_err(1, ..)` work regardless of call-site order.
    fn subtract(mut args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
        let span = args.span();
        let a = args.get_err(0, "a")?.assert_number(span)?.num;
        let b = args.get_err(1, "b")?.assert_number(span)?.num;

        Ok(Value::Dimension(SassNumber::new_unitless(a.0 - b.0)))
    }

    let options = Options::default()
        .add_custom_fn_with_signature("subtract($a, $b)", subtract)
        .unwrap();

    let css = grass::from_string(
        "a { width: subtract($b: 3, $a: 10); }".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(css, "a {\n  width: 7;\n}\n");
}

#[test]
fn dynamic_fn_fills_default_referencing_earlier_arg() {
    fn scale(mut args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
        let span = args.span();
        let a = args.get_err(0, "a")?.assert_number(span)?.num;
        let b = args.get_err(1, "b")?.assert_number(span)?.num;

        Ok(Value::Dimension(SassNumber::new_unitless(a.0 * b.0)))
    }

    let options = Options::default()
        .add_custom_fn_with_signature("scale($a, $b: $a)", scale)
        .unwrap();

    let css = grass::from_string("a { width: scale(4); }".to_owned(), &options).unwrap();
    assert_eq!(css, "a {\n  width: 16;\n}\n");

    let css = grass::from_string("a { width: scale(4, 2); }".to_owned(), &options).unwrap();
    assert_eq!(css, "a {\n  width: 8;\n}\n");
}

#[test]
fn dynamic_fn_collects_rest_arg_into_arg_list() {
    fn describe(mut args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
        let span = args.span();
        let first = args.get_err(0, "first")?.assert_number(span)?.num;

        let rest = args.get_err(1, "rest")?;
        let Value::ArgList(rest) = rest else {
            panic!("expected an ArgList, got {rest:?}");
        };

        let rest_len = rest.len();
        let has_extra = rest.keywords().get(&"extra".into()).is_some();

        Ok(Value::String(
            format!("{}-{}-{}", first.0, rest_len, has_extra).into(),
            grass::sass_value::QuoteKind::None,
        ))
    }

    let options = Options::default()
        .add_custom_fn_with_signature("describe($first, $rest...)", describe)
        .unwrap();

    let css = grass::from_string(
        "a { content: describe(1, 2, 3, $extra: 4); }".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(css, "a {\n  content: 1-2-true;\n}\n");
}

#[test]
fn dynamic_fn_captures_environment_state() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_clone = Arc::clone(&calls);

    let counter = move |mut args: ArgumentResult, _visitor: &mut Visitor| -> SassResult<Value> {
        let span = args.span();
        let n = args.get_err(0, "n")?.assert_number(span)?.num;
        let call_index = calls_clone.fetch_add(1, Ordering::SeqCst);

        Ok(Value::Dimension(SassNumber::new_unitless(
            n.0 + call_index as f64,
        )))
    };

    let options = Options::default()
        .add_custom_fn_with_signature("counter($n)", counter)
        .unwrap();

    let css = grass::from_string(
        "a { one: counter(10); two: counter(10); three: counter(10); }".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(
        css,
        "a {\n  one: 10;\n  two: 11;\n  three: 12;\n}\n"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn dynamic_fn_arity_error_matches_user_defined_function_message() {
    fn noop(_args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
        Ok(Value::Null)
    }

    let dynamic_options = Options::default()
        .add_custom_fn_with_signature("takes-two($a, $b)", noop)
        .unwrap();

    let dynamic_err =
        grass::from_string("a { b: takes-two(1); }".to_owned(), &dynamic_options).unwrap_err();

    let user_defined_err = grass::from_string(
        "@function takes-two($a, $b) { @return null; }\na { b: takes-two(1); }".to_owned(),
        &Options::default(),
    )
    .unwrap_err();

    assert!(dynamic_err.to_string().contains("Missing argument $b."));
    assert!(user_defined_err.to_string().contains("Missing argument $b."));
}

#[test]
fn add_custom_fn_with_signature_rejects_malformed_signature() {
    fn noop(_args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
        Ok(Value::Null)
    }

    let result = Options::default().add_custom_fn_with_signature("not-a-valid-signature", noop);

    assert!(result.is_err());
}
