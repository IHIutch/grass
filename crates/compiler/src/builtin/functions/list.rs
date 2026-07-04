use crate::builtin::builtin_imports::*;

pub(crate) fn length(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let len = args.get_err(0, "list")?.list_len();

    Ok(Value::Dimension(SassNumber::new_unitless(len)))
}

pub(crate) fn nth(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let list = args.get_err(0, "list")?;
    let index = args
        .get_err(1, "n")?
        .assert_number_with_name("n", args.span())?;

    if index.num.is_zero() {
        return Err(("$n: List index may not be 0.", args.span()).into());
    }

    let len = list.list_len();

    if index.num.abs() > Number::from(len) {
        return Err((
            format!(
                "$n: Invalid index {}{} for a list with {} elements.",
                index.num.inspect(),
                index.unit,
                len
            ),
            args.span(),
        )
            .into());
    }

    let index_int = index.assert_int_with_name("n", args.span())?;

    let idx = if index.num.is_positive() {
        debug_assert!(index_int > 0);
        index_int as usize - 1
    } else {
        len - index_int.unsigned_abs() as usize
    };

    Ok(match list {
        Value::List(v, ..) => v[idx].clone(),
        Value::Map(m) => {
            let (k, v) = m.iter().nth(idx).expect("idx validated against list_len above");
            Value::List(
                Rc::new(vec![k.node.clone(), v.clone()]),
                ListSeparator::Space,
                Brackets::None,
            )
        }
        Value::ArgList(v) => v.elems[idx].clone(),
        v => v,
    })
}

pub(crate) fn list_separator(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    Ok(Value::String(
        args.get_err(0, "list")?.separator().name().into(),
        QuoteKind::None,
    ))
}

pub(crate) fn set_nth(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let (mut list, sep, brackets) = match args.get_err(0, "list")? {
        Value::List(v, sep, b) => (Rc::unwrap_or_clone(v), sep, b),
        Value::ArgList(v) => (
            v.elems.into_iter().collect(),
            ListSeparator::Comma,
            Brackets::None,
        ),
        Value::Map(m) => (m.as_list(), ListSeparator::Comma, Brackets::None),
        v => (vec![v], ListSeparator::Undecided, Brackets::None),
    };
    let index = args
        .get_err(1, "n")?
        .assert_number_with_name("n", args.span())?;

    if index.num.is_zero() {
        return Err(("$n: List index may not be 0.", args.span()).into());
    }

    let index_int = index.assert_int_with_name("n", args.span())?;

    let len = list.len();

    if index.num.abs() > Number::from(len) {
        return Err((
            format!(
                "$n: Invalid index {}{} for a list with {} elements.",
                index.num.inspect(),
                index.unit,
                len
            ),
            args.span(),
        )
            .into());
    }

    let val = args.get_err(2, "value")?;

    if index_int.is_positive() {
        list[index_int as usize - 1] = val;
    } else {
        list[len - index_int.unsigned_abs() as usize] = val;
    }

    Ok(Value::List(Rc::new(list), sep, brackets))
}

pub(crate) fn append(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let (mut list, sep, brackets) = match args.get_err(0, "list")? {
        Value::List(v, sep, b) => (Rc::unwrap_or_clone(v), sep, b),
        Value::Map(m) => {
            let sep = if m.is_empty() {
                ListSeparator::Undecided
            } else {
                ListSeparator::Comma
            };
            (m.as_list(), sep, Brackets::None)
        }
        v => (vec![v], ListSeparator::Undecided, Brackets::None),
    };
    let val = args.get_err(1, "val")?;
    let sep = match args.default_arg(
        2,
        "separator",
        Value::String("auto".into(), QuoteKind::None),
    ) {
        Value::String(s, ..) => match s.as_str() {
            "auto" => {
                if sep == ListSeparator::Undecided {
                    ListSeparator::Space
                } else {
                    sep
                }
            }
            "comma" => ListSeparator::Comma,
            "space" => ListSeparator::Space,
            "slash" => ListSeparator::Slash,
            _ => {
                return Err((
                    "$separator: Must be \"space\", \"comma\", \"slash\", or \"auto\".",
                    args.span(),
                )
                    .into())
            }
        },
        v => {
            return Err((
                format!("$separator: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    list.push(val);

    Ok(Value::List(Rc::new(list), sep, brackets))
}

pub(crate) fn join(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(4)?;
    let (mut list1, sep1, brackets) = match args.get_err(0, "list1")? {
        Value::List(v, sep, brackets) => (Rc::unwrap_or_clone(v), sep, brackets),
        Value::ArgList(v) => (v.elems, v.separator, Brackets::None),
        Value::Map(m) => {
            let sep = if m.is_empty() {
                ListSeparator::Undecided
            } else {
                ListSeparator::Comma
            };
            (m.as_list(), sep, Brackets::None)
        }
        v => (vec![v], ListSeparator::Undecided, Brackets::None),
    };
    let (list2, sep2) = match args.get_err(1, "list2")? {
        Value::List(v, sep, ..) => (Rc::unwrap_or_clone(v), sep),
        Value::ArgList(v) => (v.elems, v.separator),
        Value::Map(m) => {
            let sep = if m.is_empty() {
                ListSeparator::Undecided
            } else {
                ListSeparator::Comma
            };
            (m.as_list(), sep)
        }
        v => (vec![v], ListSeparator::Undecided),
    };
    let sep = match args.default_arg(
        2,
        "separator",
        Value::String("auto".into(), QuoteKind::None),
    ) {
        Value::String(s, ..) => match s.as_str() {
            "auto" => {
                if sep1 != ListSeparator::Undecided {
                    sep1
                } else if sep2 != ListSeparator::Undecided {
                    sep2
                } else {
                    ListSeparator::Space
                }
            }
            "comma" => ListSeparator::Comma,
            "space" => ListSeparator::Space,
            "slash" => ListSeparator::Slash,
            _ => {
                return Err((
                    "$separator: Must be \"space\", \"comma\", \"slash\", or \"auto\".",
                    args.span(),
                )
                    .into())
            }
        },
        v => {
            return Err((
                format!("$separator: {} is not a string.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    let brackets = match args.default_arg(
        3,
        "bracketed",
        Value::String("auto".into(), QuoteKind::None),
    ) {
        Value::String(s, ..) => match s.as_str() {
            "auto" => brackets,
            _ => Brackets::Bracketed,
        },
        v => {
            if v.is_truthy() {
                Brackets::Bracketed
            } else {
                Brackets::None
            }
        }
    };

    args.no_remaining_named()?;

    list1.extend(list2);

    Ok(Value::List(Rc::new(list1), sep, brackets))
}

pub(crate) fn is_bracketed(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    Ok(Value::bool(match args.get_err(0, "list")? {
        Value::List(.., brackets) => match brackets {
            Brackets::Bracketed => true,
            Brackets::None => false,
        },
        _ => false,
    }))
}

pub(crate) fn index(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let list = args.get_err(0, "list")?;
    let value = args.get_err(1, "value")?;

    let found = match &list {
        Value::List(v, ..) => v.iter().position(|item| item == &value),
        Value::Map(m) => m.iter().position(|(k, v)| {
            Value::List(
                Rc::new(vec![k.node.clone(), v.clone()]),
                ListSeparator::Space,
                Brackets::None,
            ) == value
        }),
        Value::ArgList(v) => v.elems.iter().position(|item| item == &value),
        v => {
            if v == &value {
                Some(0)
            } else {
                None
            }
        }
    };

    match found {
        Some(idx) => Ok(Value::Dimension(SassNumber::new_unitless(idx + 1))),
        None => Ok(Value::Null),
    }
}

pub(crate) fn zip(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    let lists = args
        .get_variadic()?
        .into_iter()
        .map(|x| x.node.as_list())
        .collect::<Vec<Vec<Value>>>();

    let len = lists.iter().map(Vec::len).min().unwrap_or(0);

    if len == 0 {
        return Ok(Value::List(
            Rc::new(Vec::new()),
            ListSeparator::Comma,
            Brackets::None,
        ));
    }

    let result = (0..len)
        .map(|i| {
            let items = lists.iter().map(|v| v[i].clone()).collect();
            Value::List(Rc::new(items), ListSeparator::Space, Brackets::None)
        })
        .collect();

    Ok(Value::List(Rc::new(result), ListSeparator::Comma, Brackets::None))
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("length", Builtin::new(length));
    f.insert("nth", Builtin::new(nth));
    f.insert("list-separator", Builtin::new(list_separator));
    f.insert("set-nth", Builtin::new(set_nth));
    f.insert("append", Builtin::new(append));
    f.insert("join", Builtin::new(join));
    f.insert("is-bracketed", Builtin::new(is_bracketed));
    f.insert("index", Builtin::new(index));
    f.insert("zip", Builtin::new(zip));
}
