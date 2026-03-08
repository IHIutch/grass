use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::{Configuration, ConfiguredValue};
use crate::builtin::builtin_imports::*;

use crate::ast::Mixin;
use crate::ContextFlags;
use crate::builtin::{
    meta::{
        accepts_content, call, content_exists, feature_exists, function_exists, get_function,
        get_mixin, global_variable_exists, inspect, keywords, mixin_exists, type_of,
        variable_exists,
    },
    modules::Module,
};
use crate::serializer::serialize_calculation_arg;

fn load_css(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<()> {
    args.max_args(2)?;

    let span = args.span();

    let url = args
        .get_err(0, "module")?
        .assert_string_with_name("module", args.span())?
        .0;

    let with = match args.default_arg(1, "with", Value::Null) {
        Value::Map(map) => Some(map),
        Value::List(v, ..) if v.is_empty() => Some(SassMap::new()),
        Value::ArgList(v) if v.is_empty() => Some(SassMap::new()),
        Value::Null => None,
        v => return Err((format!("$with: {} is not a map.", v.inspect(span)?), span).into()),
    };

    let mut configuration = Configuration::empty();

    if let Some(with) = with {
        visitor.emit_warning("`grass` does not currently support the $with parameter of load-css. This file will be imported the same way it would using `@import`.", args.span());

        let mut values = HashMap::new();
        for (key, value) in with {
            let name =
                Identifier::from(key.node.assert_string_with_name("with key", args.span())?.0);

            if values.contains_key(&name) {
                // todo: write test for this
                return Err((
                    format!("The variable {name} was configured twice.", name = name),
                    key.span,
                )
                    .into());
            }

            values.insert(name, ConfiguredValue::explicit(value, args.span()));
        }

        configuration = Configuration::explicit(values, args.span());
    }

    let _configuration = Arc::new(RefCell::new(configuration));

    let style_sheet = visitor.load_style_sheet(url.as_ref(), false, args.span())?;

    visitor.visit_stylesheet(style_sheet)?;

    // todo: support the $with argument to load-css
    // visitor.load_module(
    //     url.as_ref(),
    //     Some(Arc::clone(&configuration)),
    //     true,
    //     args.span(),
    //     |visitor, module, stylesheet| {
    //         // (*module).borrow()
    //         Ok(())
    //     },
    // )?;

    // Visitor::assert_configuration_is_empty(&configuration, true)?;

    Ok(())
}

fn module_functions(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let module = Identifier::from(
        args.get_err(0, "module")?
            .assert_string_with_name("module", args.span())?
            .0,
    );

    Ok(Value::Map(
        (*(*visitor.env.modules).borrow().get(module, args.span())?)
            .borrow()
            .functions(args.span()),
    ))
}

fn module_variables(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let module = Identifier::from(
        args.get_err(0, "module")?
            .assert_string_with_name("module", args.span())?
            .0,
    );

    Ok(Value::Map(
        (*(*visitor.env.modules).borrow().get(module, args.span())?)
            .borrow()
            .variables(args.span()),
    ))
}

fn calc_args(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let calc = match args.get_err(0, "calc")? {
        Value::Calculation(calc) => calc,
        v => {
            return Err((
                format!("$calc: {} is not a calculation.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    let args = calc
        .args
        .into_iter()
        .map(|arg| {
            Ok(match arg {
                CalculationArg::Number(num) => Value::Dimension(num),
                CalculationArg::Calculation(calc) => Value::Calculation(calc),
                CalculationArg::String(s) | CalculationArg::Interpolation(s) => {
                    Value::String(s, QuoteKind::None)
                }
                CalculationArg::Operation { .. } => Value::String(
                    serialize_calculation_arg(&arg, visitor.options, args.span())?,
                    QuoteKind::None,
                ),
            })
        })
        .collect::<SassResult<Vec<_>>>()?;

    Ok(Value::List(args, ListSeparator::Comma, Brackets::None))
}

fn calc_name(mut args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let calc = match args.get_err(0, "calc")? {
        Value::Calculation(calc) => calc,
        v => {
            return Err((
                format!("$calc: {} is not a calculation.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    Ok(Value::String(calc.name.to_string(), QuoteKind::Quoted))
}

fn module_mixins(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let module = Identifier::from(
        args.get_err(0, "module")?
            .assert_string_with_name("module", args.span())?
            .0,
    );

    Ok(Value::Map(
        (*(*visitor.env.modules).borrow().get(module, args.span())?)
            .borrow()
            .mixins(args.span()),
    ))
}

fn apply(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<()> {
    let span = args.span();
    let mixin = match args.get_err(0, "mixin")? {
        Value::MixinRef(m) => *m,
        v => {
            return Err((
                format!(
                    "$mixin: {} is not a mixin reference.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };
    args.remove_positional(0);

    let has_content = visitor.env.content.is_some();

    match mixin.mixin {
        Mixin::Builtin(func) => {
            if has_content {
                return Err(("Mixin doesn't accept a content block.", span).into());
            }
            func(args, visitor)?;
            Ok(())
        }
        Mixin::BuiltinWithContent(func) => {
            func(args, visitor)?;
            Ok(())
        }
        Mixin::UserDefined(mixin_def, env) => {
            if has_content && !mixin_def.has_content {
                return Err(("Mixin doesn't accept a content block.", span).into());
            }

            let old_in_mixin = visitor.flags.in_mixin();
            visitor.flags.set(ContextFlags::IN_MIXIN, true);

            let content = visitor.env.content.take();

            visitor.run_user_defined_callable::<_, (), _>(
                MaybeEvaledArguments::Evaled(args),
                mixin_def,
                &env,
                span,
                |mixin, visitor| {
                    visitor.with_content(content, |visitor| {
                        for stmt in mixin.body.iter().cloned() {
                            let result = visitor.visit_stmt(stmt)?;
                            debug_assert!(result.is_none());
                        }
                        Ok(())
                    })
                },
            )?;

            visitor.flags.set(ContextFlags::IN_MIXIN, old_in_mixin);
            Ok(())
        }
    }
}

pub(crate) fn declare(f: &mut Module) {
    f.insert_builtin("feature-exists", feature_exists);
    f.insert_builtin("inspect", inspect);
    f.insert_builtin("type-of", type_of);
    f.insert_builtin("keywords", keywords);
    f.insert_builtin("global-variable-exists", global_variable_exists);
    f.insert_builtin("variable-exists", variable_exists);
    f.insert_builtin("function-exists", function_exists);
    f.insert_builtin("mixin-exists", mixin_exists);
    f.insert_builtin("content-exists", content_exists);
    f.insert_builtin("module-variables", module_variables);
    f.insert_builtin("module-functions", module_functions);
    f.insert_builtin("get-function", get_function);
    f.insert_builtin("call", call);
    f.insert_builtin("calc-args", calc_args);
    f.insert_builtin("calc-name", calc_name);
    f.insert_builtin("get-mixin", get_mixin);
    f.insert_builtin("module-mixins", module_mixins);
    f.insert_builtin("accepts-content", accepts_content);

    f.insert_builtin_mixin("load-css", load_css);
    f.insert_builtin_mixin_with_content("apply", apply);
}
