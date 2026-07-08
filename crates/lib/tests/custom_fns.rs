#![cfg(feature = "custom-builtin-fns")]

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
