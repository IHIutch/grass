use compact_str::CompactString;

use crate::builtin::builtin_imports::*;

/// `AstExpr` (via `Argument::default`) holds `Rc<Color>`, so this can't be a
/// `static`/`LazyLock` (not `Sync`) -- the caller's own arena is used
/// instead, matching how the rest of the AST is allocated (see Plan 091 /
/// todo #276): the three `Argument`s die with the arena's chunk rather than
/// leaking a fresh heap `Vec` on every call, as the old code did.
pub(crate) fn if_arguments<'a>(arena: &'a bumpalo::Bump) -> ArgumentDeclaration<'a> {
    ArgumentDeclaration {
        args: arena.alloc_slice_fill_iter(vec![
            Argument {
                name: Identifier::from("condition"),
                default: None,
                default_span: None,
            },
            Argument {
                name: Identifier::from("if-true"),
                default: None,
                default_span: None,
            },
            Argument {
                name: Identifier::from("if-false"),
                default: None,
                default_span: None,
            },
        ]),
        rest: None,
    }
}

fn if_(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    if args.get_err(0, "condition")?.is_truthy() {
        Ok(args.get_err(1, "if-true")?)
    } else {
        Ok(args.get_err(2, "if-false")?)
    }
}

pub(crate) fn feature_exists(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();
    let feature = args
        .get_err(0, "feature")?
        .assert_string_with_name("feature", span)?
        .0;

    visitor.emit_deprecation(Deprecation::FeatureExists, span, || {
        Ok(
            "The feature-exists() function is deprecated.\n\nMore info: \
            https://sass-lang.com/d/feature-exists"
                .to_string(),
        )
    })?;

    #[allow(clippy::match_same_arms)]
    Ok(match feature.as_str() {
        // A local variable will shadow a global variable unless
        // `!global` is used.
        "global-variable-shadowing" => Value::True,
        // the @extend rule will affect selectors nested in pseudo-classes
        // like :not()
        "extend-selector-pseudoclass" => Value::True,
        // Full support for unit arithmetic using units defined in the
        // [Values and Units Level 3][] spec.
        "units-level-3" => Value::True,
        // The Sass `@error` directive is supported.
        "at-error" => Value::True,
        // The "Custom Properties Level 1" spec is supported. This means
        // that custom properties are parsed statically, with only
        // interpolation treated as SassScript.
        "custom-property" => Value::True,
        _ => Value::False,
    })
}

pub(crate) fn unit(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let number = args
        .get_err(0, "number")?
        .assert_number_with_name("number", args.span())?;

    Ok(Value::String(
        number.unit.to_string().into(),
        QuoteKind::Quoted,
    ))
}

pub(crate) fn type_of(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let value = args.get_err(0, "value")?;
    Ok(Value::String(value.kind().into(), QuoteKind::None))
}

pub(crate) fn unitless(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let number = args
        .get_err(0, "number")?
        .assert_number_with_name("number", args.span())?;

    Ok(Value::bool(number.unit == Unit::None))
}

pub(crate) fn inspect(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let value = args.get_err(0, "value")?;
    Ok(Value::String(
        value.inspect(args.span())?.into(),
        QuoteKind::None,
    ))
}

pub(crate) fn variable_exists(
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    args.max_args(1)?;

    let name = Identifier::from(
        args.get_err(0, "name")?
            .assert_string_with_name("name", args.span())?
            .0
            .as_str(),
    );

    Ok(Value::bool(visitor.env.var_exists(
        name,
        None,
        args.span(),
    )?))
}

pub(crate) fn global_variable_exists(
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    args.max_args(2)?;

    let name = Identifier::from(
        args.get_err(0, "name")?
            .assert_string_with_name("name", args.span())?
            .0
            .as_str(),
    );

    let module = match args.default_arg(1, "module", Value::Null) {
        Value::String(s, _) => Some(s),
        Value::Null => None,
        v => {
            return Err((
                format!("$module: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    Ok(Value::bool(if let Some(module_name) = module {
        (*(*visitor.env.modules)
            .borrow()
            .get(Identifier::verbatim(&module_name), args.span())?)
        .borrow()
        .var_exists(name)
    } else {
        visitor.env.global_var_exists(name, args.span())?
    }))
}

pub(crate) fn mixin_exists(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let name = Identifier::from(
        args.get_err(0, "name")?
            .assert_string_with_name("name", args.span())?
            .0
            .as_str(),
    );

    let module = match args.default_arg(1, "module", Value::Null) {
        Value::String(s, _) => Some(s),
        Value::Null => None,
        v => {
            return Err((
                format!("$module: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    Ok(Value::bool(if let Some(module_name) = module {
        (*(*visitor.env.modules)
            .borrow()
            .get(Identifier::verbatim(&module_name), args.span())?)
        .borrow()
        .mixin_exists(name)
    } else {
        visitor.env.mixin_exists(name, args.span())?
    }))
}

pub(crate) fn function_exists(
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    args.max_args(2)?;

    let name = Identifier::from(
        args.get_err(0, "name")?
            .assert_string_with_name("name", args.span())?
            .0
            .as_str(),
    );

    let module = match args.default_arg(1, "module", Value::Null) {
        Value::String(s, _) => Some(s),
        Value::Null => None,
        v => {
            return Err((
                format!("$module: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    Ok(Value::bool(if let Some(module_name) = module {
        (*(*visitor.env.modules)
            .borrow()
            .get(Identifier::verbatim(&module_name), args.span())?)
        .borrow()
        .fn_exists(name)
    } else {
        visitor.env.fn_exists(name, args.span())?
    }))
}

pub(crate) fn get_function(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let name: Identifier = match args.get_err(0, "name")? {
        Value::String(s, _) => Identifier::from(s.as_str()),
        v => {
            return Err((
                format!("$name: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };
    let css = args.default_arg(1, "css", Value::False).is_truthy();
    let module = match args.default_arg(2, "module", Value::Null) {
        Value::String(s, ..) => Some(s),
        Value::Null => None,
        v => {
            return Err((
                format!("$module: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    if css && module.is_some() {
        return Err((
            "$css and $module may not both be passed at once.",
            args.span(),
        )
            .into());
    }

    let func = if css {
        Some(SassFunction::Plain {
            original_name: CompactString::from(name.as_str()),
            name,
        })
    } else if let Some(module_name) = module {
        visitor.env.get_fn(
            name,
            Some(Spanned {
                node: Identifier::verbatim(&module_name),
                span: args.span(),
            }),
            args.span(),
        )?
    } else {
        match visitor.env.get_fn(name, None, args.span())? {
            Some(f) => Some(f),
            None => GLOBAL_FUNCTIONS
                .get(name.as_str())
                .map(|f| SassFunction::Builtin(f.clone(), name)),
        }
    };

    match func {
        Some(func) => Ok(Value::FunctionRef(Box::new(func))),
        None => Err((format!("Function not found: {name}"), args.span()).into()),
    }
}

pub(crate) fn get_mixin(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let name: Identifier = match args.get_err(0, "name")? {
        Value::String(s, _) => Identifier::from(s.as_str()),
        v => {
            return Err((
                format!("$name: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };
    let module = match args.default_arg(1, "module", Value::Null) {
        Value::String(s, ..) => Some(s),
        Value::Null => None,
        v => {
            return Err((
                format!("$module: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    let mixin = if let Some(module_name) = module {
        let spanned = Spanned {
            node: Identifier::verbatim(&module_name),
            span: args.span(),
        };
        Some(visitor.env.get_mixin(
            Spanned {
                node: name,
                span: args.span(),
            },
            Some(spanned),
        )?)
    } else {
        visitor
            .env
            .get_mixin(
                Spanned {
                    node: name,
                    span: args.span(),
                },
                None,
            )
            .ok()
    };

    use crate::ast::SassMixin;

    match mixin {
        Some(mixin) => Ok(Value::MixinRef(Box::new(SassMixin { name, mixin }))),
        None => Err((format!("Mixin not found: {name}"), args.span()).into()),
    }
}

pub(crate) fn accepts_content(
    mut args: ArgumentResult,
    _visitor: &mut Visitor,
) -> SassResult<Value> {
    args.max_args(1)?;
    use crate::ast::Mixin;
    let mixin = match args.get_err(0, "mixin")? {
        Value::MixinRef(m) => *m,
        v => {
            return Err((
                format!(
                    "$mixin: {} is not a mixin reference.",
                    v.inspect(args.span())?
                ),
                args.span(),
            )
                .into())
        }
    };
    match mixin.mixin {
        Mixin::UserDefined(m, _, _) => Ok(Value::bool(m.has_content)),
        Mixin::Builtin(_) => Ok(Value::False),
        Mixin::BuiltinWithContent(_) => Ok(Value::True),
    }
}

pub(crate) fn call(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    let span = args.span();
    let func = match args.get_err(0, "function")? {
        Value::FunctionRef(f) => *f,
        value @ Value::String(..) => {
            // dart-sass's message reconstructs the function name via the
            // string Value's own `toString()`, which preserves its original
            // quotedness (e.g. `unquote("if")` shows as `if`, not `"if"`).
            let quoted_name = value.to_css_string(span, false)?;
            visitor.emit_deprecation(Deprecation::CallString, span, || {
                Ok(format!(
                    "Passing a string to call() is deprecated and will be illegal in Dart Sass \
                     2.0.0.\n\nRecommendation: call(get-function({quoted_name}))"
                ))
            })?;

            let Value::String(name, ..) = value else {
                unreachable!()
            };
            let name = Identifier::from(name.as_str());

            match visitor.env.get_fn(name, None, span)? {
                Some(f) => f,
                None => match GLOBAL_FUNCTIONS.get(name.as_str()) {
                    Some(f) => SassFunction::Builtin(f.clone(), name),
                    None => SassFunction::Plain {
                        original_name: CompactString::from(name.as_str()),
                        name,
                    },
                },
            }
        }
        v => {
            return Err((
                format!(
                    "$function: {} is not a function reference.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };

    args.remove_positional(0);
    // dart maps arguments forwarded through `call()` to the `call(...)`
    // expression itself, not to its argument expressions (verified vs
    // sass 1.101.0).
    args.degrade_spans_to_callable_node();

    visitor.run_function_callable_with_maybe_evaled(func, MaybeEvaledArguments::Evaled(args), span)
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn content_exists(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(0)?;
    if !visitor.flags.in_mixin() {
        return Err((
            "content-exists() may only be called within a mixin.",
            args.span(),
        )
            .into());
    }
    Ok(Value::bool(visitor.env.content.is_some()))
}

pub(crate) fn keywords(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let span = args.span();

    let args = match args.get_err(0, "args")? {
        Value::ArgList(args) => args,
        v => {
            return Err((
                format!("$args: {} is not an argument list.", v.inspect(span)?),
                span,
            )
                .into())
        }
    };

    Ok(Value::Map(SassMap::new_with(
        args.into_keywords()
            .into_iter()
            .map(|(name, val)| {
                (
                    Value::String(name.to_string().into(), QuoteKind::None).span(span),
                    val,
                )
            })
            .collect(),
    )))
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    // No module equivalent in dart-sass; never warns.
    f.insert("if", Builtin::new(if_));
    f.insert(
        "feature-exists",
        Builtin::new(feature_exists).with_deprecated_global("meta", "feature-exists"),
    );
    // "unit"/"unitless" live here for shared code, but their dart-sass module
    // replacement is math.unit / math.is-unitless, not meta.*.
    f.insert(
        "unit",
        Builtin::new(unit).with_deprecated_global("math", "unit"),
    );
    f.insert(
        "type-of",
        Builtin::new(type_of).with_deprecated_global("meta", "type-of"),
    );
    f.insert(
        "unitless",
        Builtin::new(unitless).with_deprecated_global("math", "is-unitless"),
    );
    f.insert(
        "inspect",
        Builtin::new(inspect).with_deprecated_global("meta", "inspect"),
    );
    f.insert(
        "variable-exists",
        Builtin::new(variable_exists).with_deprecated_global("meta", "variable-exists"),
    );
    f.insert(
        "global-variable-exists",
        Builtin::new(global_variable_exists)
            .with_deprecated_global("meta", "global-variable-exists"),
    );
    f.insert(
        "mixin-exists",
        Builtin::new(mixin_exists).with_deprecated_global("meta", "mixin-exists"),
    );
    f.insert(
        "function-exists",
        Builtin::new(function_exists).with_deprecated_global("meta", "function-exists"),
    );
    f.insert(
        "get-function",
        Builtin::new(get_function).with_deprecated_global("meta", "get-function"),
    );
    f.insert(
        "call",
        Builtin::new(call).with_deprecated_global("meta", "call"),
    );
    f.insert(
        "content-exists",
        Builtin::new(content_exists).with_deprecated_global("meta", "content-exists"),
    );
    f.insert(
        "keywords",
        Builtin::new(keywords).with_deprecated_global("meta", "keywords"),
    );
}
