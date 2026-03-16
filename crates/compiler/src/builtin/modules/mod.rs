use std::{cell::RefCell, fmt, rc::Rc};

use rustc_hash::{FxHashMap, FxHashSet};

use codemap::{Span, Spanned};

use crate::{
    ast::{ArgumentResult, AstForwardRule, BuiltinMixin, Mixin, SassMixin},
    builtin::Builtin,
    common::Identifier,
    error::SassResult,
    evaluate::{Environment, Visitor},
    selector::ExtensionStore,
    utils::{
        BaseMapView, LimitedMapView, MapView, MergedMapView, PrefixedMapView, PublicMemberMapView,
    },
    value::{SassFunction, SassMap, Value},
};

use super::builtin_imports::QuoteKind;

mod color;
mod list;
mod map;
mod math;
mod meta;
mod selector;
mod string;

/// A [Module] that only exposes members that aren't shadowed by a given
/// blocklist of member names.
#[derive(Debug, Clone)]
pub(crate) struct ShadowedModule {
    pub(crate) inner: Rc<RefCell<Module>>,
    scope: ModuleScope,
}

impl ShadowedModule {
    pub fn new(
        module: Rc<RefCell<Module>>,
        variables: Option<&FxHashSet<Identifier>>,
        functions: Option<&FxHashSet<Identifier>>,
        mixins: Option<&FxHashSet<Identifier>>,
    ) -> Self {
        let module_scope = module.borrow().scope();

        let variables = Self::shadowed_map(Rc::clone(&module_scope.variables), variables);
        let functions = Self::shadowed_map(Rc::clone(&module_scope.functions), functions);
        let mixins = Self::shadowed_map(Rc::clone(&module_scope.mixins), mixins);

        let new_scope = ModuleScope {
            variables,
            functions,
            mixins,
        };

        Self {
            inner: module,
            scope: new_scope,
        }
    }

    fn needs_blocklist<V: fmt::Debug + Clone>(
        map: Rc<dyn MapView<Value = V>>,
        blocklist: Option<&FxHashSet<Identifier>>,
    ) -> bool {
        blocklist.is_some()
            && !map.is_empty()
            && blocklist.unwrap().iter().any(|key| map.contains_key(*key))
    }

    fn shadowed_map<V: fmt::Debug + Clone + 'static>(
        map: Rc<dyn MapView<Value = V>>,
        blocklist: Option<&FxHashSet<Identifier>>,
    ) -> Rc<dyn MapView<Value = V>> {
        match blocklist {
            Some(..) if !Self::needs_blocklist(Rc::clone(&map), blocklist) => map,
            Some(blocklist) => Rc::new(LimitedMapView::blocklist(map, blocklist)),
            None => map,
        }
    }

    pub fn if_necessary(
        module: Rc<RefCell<Module>>,
        variables: Option<&FxHashSet<Identifier>>,
        functions: Option<&FxHashSet<Identifier>>,
        mixins: Option<&FxHashSet<Identifier>>,
    ) -> Option<Rc<RefCell<Module>>> {
        let module_scope = module.borrow().scope();

        let needs_blocklist = Self::needs_blocklist(Rc::clone(&module_scope.variables), variables)
            || Self::needs_blocklist(Rc::clone(&module_scope.functions), functions)
            || Self::needs_blocklist(Rc::clone(&module_scope.mixins), mixins);

        if needs_blocklist {
            Some(Rc::new(RefCell::new(Module::Shadowed(Self::new(
                module, variables, functions, mixins,
            )))))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ForwardedModule {
    scope: ModuleScope,
    pub(crate) inner: Rc<RefCell<Module>>,
    pub(crate) forward_rule: AstForwardRule<'static>,
}

impl ForwardedModule {
    pub fn new(module: Rc<RefCell<Module>>, rule: AstForwardRule<'static>) -> Self {
        let scope = (*module).borrow().scope();

        let variables = Self::forwarded_map(
            scope.variables,
            rule.prefix.as_deref(),
            rule.shown_variables.as_ref(),
            rule.hidden_variables.as_ref(),
        );

        let functions = Self::forwarded_map(
            scope.functions,
            rule.prefix.as_deref(),
            rule.shown_mixins_and_functions.as_ref(),
            rule.hidden_mixins_and_functions.as_ref(),
        );

        let mixins = Self::forwarded_map(
            scope.mixins,
            rule.prefix.as_deref(),
            rule.shown_mixins_and_functions.as_ref(),
            rule.hidden_mixins_and_functions.as_ref(),
        );

        let scope = ModuleScope {
            variables,
            mixins,
            functions,
        };

        ForwardedModule {
            inner: module,
            forward_rule: rule,
            scope,
        }
    }

    fn forwarded_map<T: Clone + fmt::Debug + 'static>(
        mut map: Rc<dyn MapView<Value = T>>,
        prefix: Option<&str>,
        safelist: Option<&FxHashSet<Identifier>>,
        blocklist: Option<&FxHashSet<Identifier>>,
    ) -> Rc<dyn MapView<Value = T>> {
        debug_assert!(safelist.is_none() || blocklist.is_none());

        if prefix.is_none() && safelist.is_none() && blocklist.is_none() {
            return map;
        }

        if let Some(prefix) = prefix {
            map = Rc::new(PrefixedMapView(map, prefix.to_owned()));
        }

        // Apply show/hide after prefix, since show/hide names are in prefixed form
        if let Some(safelist) = safelist {
            map = Rc::new(LimitedMapView::safelist(map, safelist));
        } else if let Some(blocklist) = blocklist {
            map = Rc::new(LimitedMapView::blocklist(map, blocklist));
        }

        map
    }

    pub fn if_necessary(
        module: Rc<RefCell<Module>>,
        rule: AstForwardRule<'static>,
    ) -> Rc<RefCell<Module>> {
        if rule.prefix.is_none()
            && rule.shown_mixins_and_functions.is_none()
            && rule.shown_variables.is_none()
            && rule
                .hidden_mixins_and_functions
                .as_ref()
                .is_some_and(FxHashSet::is_empty)
            && rule
                .hidden_variables
                .as_ref()
                .is_some_and(FxHashSet::is_empty)
        {
            module
        } else {
            Rc::new(RefCell::new(Module::Forwarded(ForwardedModule::new(
                module, rule,
            ))))
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ModuleScope {
    pub variables: Rc<dyn MapView<Value = Value>>,
    pub mixins: Rc<dyn MapView<Value = Mixin>>,
    pub functions: Rc<dyn MapView<Value = SassFunction>>,
}

impl ModuleScope {
    pub fn new() -> Self {
        Self {
            variables: Rc::new(BaseMapView(Rc::new(RefCell::new(FxHashMap::default())))),
            mixins: Rc::new(BaseMapView(Rc::new(RefCell::new(FxHashMap::default())))),
            functions: Rc::new(BaseMapView(Rc::new(RefCell::new(FxHashMap::default())))),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Module {
    Environment {
        scope: ModuleScope,
        upstream: Vec<Rc<RefCell<Module>>>,
        extension_store: ExtensionStore,
        #[allow(dead_code)]
        env: Environment,
    },
    Builtin {
        scope: ModuleScope,
    },
    Forwarded(ForwardedModule),
    Shadowed(ShadowedModule),
}

#[derive(Debug, Clone)]
pub(crate) struct Modules(pub FxHashMap<Identifier, Rc<RefCell<Module>>>);

impl Modules {
    pub fn new() -> Self {
        Self(FxHashMap::default())
    }

    pub fn insert(
        &mut self,
        name: Identifier,
        module: Rc<RefCell<Module>>,
        span: Span,
    ) -> SassResult<()> {
        if self.0.contains_key(&name) {
            return Err((
                format!("There's already a module with namespace \"{}\".", name),
                span,
            )
                .into());
        }

        self.0.insert(name, module);

        Ok(())
    }

    pub fn get(&self, name: Identifier, span: Span) -> SassResult<Rc<RefCell<Module>>> {
        match self.0.get(&name) {
            Some(v) => Ok(Rc::clone(v)),
            None => Err((
                format!(
                    "There is no module with the namespace \"{}\".",
                    name.as_str()
                ),
                span,
            )
                .into()),
        }
    }

    pub fn get_mut(
        &mut self,
        name: Identifier,
        span: Span,
    ) -> SassResult<&mut Rc<RefCell<Module>>> {
        match self.0.get_mut(&name) {
            Some(v) => Ok(v),
            None => Err((
                format!(
                    "There is no module with the namespace \"{}\".",
                    name.as_str()
                ),
                span,
            )
                .into()),
        }
    }
}

fn member_map<V: fmt::Debug + Clone + 'static>(
    local: Rc<dyn MapView<Value = V>>,
    others: Vec<Rc<dyn MapView<Value = V>>>,
) -> Rc<dyn MapView<Value = V>> {
    let local_map = PublicMemberMapView(local);

    if others.is_empty() {
        return Rc::new(local_map);
    }

    let mut all_maps: Vec<Rc<dyn MapView<Value = V>>> =
        others.into_iter().filter(|map| !map.is_empty()).collect();

    all_maps.push(Rc::new(local_map));

    // todo: potential optimization when all_maps.len() == 1
    Rc::new(MergedMapView::new(all_maps))
}

impl Module {
    pub fn new_env_with_upstream(
        env: Environment,
        extension_store: ExtensionStore,
        upstream: Vec<Rc<RefCell<Module>>>,
    ) -> Self {
        let variables = {
            let variables = (*env.forwarded_modules).borrow();
            let variables = variables
                .iter()
                .map(|module| Rc::clone(&(*module).borrow().scope().variables));
            let this = Rc::new(BaseMapView(env.global_vars()));
            member_map(this, variables.collect())
        };

        let mixins = {
            let mixins = (*env.forwarded_modules).borrow();
            let mixins = mixins
                .iter()
                .map(|module| Rc::clone(&(*module).borrow().scope().mixins));
            let this = Rc::new(BaseMapView(env.global_mixins()));
            member_map(this, mixins.collect())
        };

        let functions = {
            let functions = (*env.forwarded_modules).borrow();
            let functions = functions
                .iter()
                .map(|module| Rc::clone(&(*module).borrow().scope().functions));
            let this = Rc::new(BaseMapView(env.global_functions()));
            member_map(this, functions.collect())
        };

        let scope = ModuleScope {
            variables,
            mixins,
            functions,
        };

        Module::Environment {
            scope,
            upstream,
            extension_store,
            env,
        }
    }

    pub fn new_builtin() -> Self {
        Module::Builtin {
            scope: ModuleScope::new(),
        }
    }

    pub(crate) fn scope(&self) -> ModuleScope {
        match self {
            Self::Builtin { scope }
            | Self::Environment { scope, .. }
            | Self::Forwarded(ForwardedModule { scope, .. })
            | Self::Shadowed(ShadowedModule { scope, .. }) => scope.clone(),
        }
    }

    pub fn get_var(&self, name: Spanned<Identifier>) -> SassResult<Value> {
        let scope = self.scope();

        match scope.variables.get(name.node) {
            Some(v) => Ok(v),
            None => Err(("Undefined variable.", name.span).into()),
        }
    }

    pub fn get_var_no_err(&self, name: Identifier) -> Option<Value> {
        let scope = self.scope();

        scope.variables.get(name)
    }

    pub fn get_mixin_no_err(&self, name: Identifier) -> Option<Mixin> {
        let scope = self.scope();

        scope.mixins.get(name)
    }

    pub fn update_var(&mut self, name: Spanned<Identifier>, value: Value) -> SassResult<()> {
        // For Environment modules, check forwarded modules first.
        // When a midstream module shadows an upstream variable (via @forward),
        // assignment through the midstream's namespace should go to the upstream
        // (forwarded) variable, matching dart-sass's _modulesByVariable behavior.
        if let Self::Environment { env, scope, .. } = self {
            for module in env.forwarded_modules.borrow().iter() {
                if module.borrow().var_exists(name.node) {
                    module.borrow_mut().update_var(name, value)?;
                    return Ok(());
                }
            }

            if scope.variables.insert(name.node, value).is_none() {
                return Err(("Undefined variable.", name.span).into());
            }

            return Ok(());
        }

        let scope = match self {
            Self::Builtin { .. } => {
                return Err(("Cannot modify built-in variable.", name.span).into())
            }
            Self::Environment { .. } => unreachable!(),
            Self::Forwarded(ForwardedModule { scope, .. })
            | Self::Shadowed(ShadowedModule { scope, .. }) => scope.clone(),
        };

        if scope.variables.insert(name.node, value).is_none() {
            return Err(("Undefined variable.", name.span).into());
        }

        Ok(())
    }

    pub fn get_mixin(&self, name: Spanned<Identifier>) -> SassResult<Mixin> {
        let scope = self.scope();

        match scope.mixins.get(name.node) {
            Some(v) => Ok(v),
            None => Err(("Undefined mixin.", name.span).into()),
        }
    }

    pub fn insert_builtin_mixin(&mut self, name: &'static str, mixin: BuiltinMixin) {
        let scope = self.scope();

        scope.mixins.insert(name.into(), Mixin::Builtin(mixin));
    }

    pub fn insert_builtin_mixin_with_content(&mut self, name: &'static str, mixin: BuiltinMixin) {
        let scope = self.scope();

        scope
            .mixins
            .insert(name.into(), Mixin::BuiltinWithContent(mixin));
    }

    pub fn insert_builtin_var(&mut self, name: &'static str, value: Value) {
        let ident = name.into();

        let scope = self.scope();

        scope.variables.insert(ident, value);
    }

    pub fn get_fn(&self, name: Identifier) -> Option<SassFunction> {
        let scope = self.scope();

        scope.functions.get(name)
    }

    pub fn var_exists(&self, name: Identifier) -> bool {
        let scope = self.scope();

        scope.variables.get(name).is_some()
    }

    pub fn mixin_exists(&self, name: Identifier) -> bool {
        let scope = self.scope();

        scope.mixins.get(name).is_some()
    }

    pub fn fn_exists(&self, name: Identifier) -> bool {
        let scope = self.scope();

        scope.functions.get(name).is_some()
    }

    pub fn insert_builtin(
        &mut self,
        name: &'static str,
        function: fn(ArgumentResult, &mut Visitor) -> SassResult<Value>,
    ) {
        let ident = name.into();

        let scope = match self {
            Self::Builtin { scope } => scope,
            _ => unreachable!(),
        };

        scope
            .functions
            .insert(ident, SassFunction::Builtin(Builtin::new(function), ident));
    }

    pub fn functions(&self, span: Span) -> SassMap {
        SassMap::new_with(
            self.scope()
                .functions
                .iter()
                .into_iter()
                .filter(|(key, _)| !key.as_str().starts_with('-'))
                .map(|(key, value)| {
                    (
                        Value::String(key.to_string().into(), QuoteKind::Quoted).span(span),
                        Value::FunctionRef(Box::new(value)),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    pub fn mixins(&self, span: Span) -> SassMap {
        SassMap::new_with(
            self.scope()
                .mixins
                .iter()
                .into_iter()
                .filter(|(key, _)| !key.as_str().starts_with('-'))
                .map(|(key, value)| {
                    (
                        Value::String(key.to_string().into(), QuoteKind::Quoted).span(span),
                        Value::MixinRef(Box::new(SassMixin {
                            name: key,
                            mixin: value,
                        })),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    pub fn variables(&self, span: Span) -> SassMap {
        SassMap::new_with(
            self.scope()
                .variables
                .iter()
                .into_iter()
                .filter(|(key, _)| !key.as_str().starts_with('-'))
                .map(|(key, value)| {
                    (
                        Value::String(key.to_string().into(), QuoteKind::Quoted).span(span),
                        value,
                    )
                })
                .collect::<Vec<_>>(),
        )
    }
}

pub(crate) fn declare_module_color() -> Module {
    let mut module = Module::new_builtin();
    color::declare(&mut module);
    module
}

pub(crate) fn declare_module_list() -> Module {
    let mut module = Module::new_builtin();
    list::declare(&mut module);
    module
}

pub(crate) fn declare_module_map() -> Module {
    let mut module = Module::new_builtin();
    map::declare(&mut module);
    module
}

pub(crate) fn declare_module_math() -> Module {
    let mut module = Module::new_builtin();
    math::declare(&mut module);
    module
}

pub(crate) fn declare_module_meta() -> Module {
    let mut module = Module::new_builtin();
    meta::declare(&mut module);
    module
}

pub(crate) fn declare_module_selector() -> Module {
    let mut module = Module::new_builtin();
    selector::declare(&mut module);
    module
}

pub(crate) fn declare_module_string() -> Module {
    let mut module = Module::new_builtin();
    string::declare(&mut module);
    module
}
