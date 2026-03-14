use codemap::{Span, Spanned};

use crate::{
    ast::{AstForwardRule, Configuration, ConfiguredValue, Mixin},
    builtin::modules::{ForwardedModule, Module, ModuleScope, Modules, ShadowedModule},
    common::Identifier,
    error::SassResult,
    selector::ExtensionStore,
    value::{SassFunction, Value},
};
use std::{cell::RefCell, rc::Rc};

use rustc_hash::{FxHashMap, FxHashSet};

type Mutable<T> = Rc<RefCell<T>>;

use super::{scope::Scopes, visitor::CallableContentBlock};

#[derive(Debug, Clone)]
pub(crate) struct Environment {
    pub scopes: Scopes,
    pub modules: Mutable<Modules>,
    pub global_modules: Vec<Mutable<Module>>,
    pub content: Option<Rc<CallableContentBlock>>,
    pub forwarded_modules: Mutable<Vec<Mutable<Module>>>,
    pub imported_modules: Mutable<Vec<Mutable<Module>>>,
    #[allow(clippy::type_complexity)]
    pub nested_forwarded_modules: Option<Mutable<Vec<Mutable<Vec<Mutable<Module>>>>>>,
    /// Cached source identity maps for conflict detection in @forward.
    /// Maps member name → source module pointer for all previously forwarded members.
    forwarded_member_sources: ForwardedMemberSources,
}

type SourcePtr = *const RefCell<Module>;

#[derive(Debug, Clone, Default)]
struct ForwardedMemberSources {
    variables: FxHashMap<Identifier, SourcePtr>,
    functions: FxHashMap<Identifier, SourcePtr>,
    mixins: FxHashMap<Identifier, SourcePtr>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            scopes: Scopes::new(),
            modules: Rc::new(RefCell::new(Modules::new())),
            global_modules: Vec::new(),
            content: None,
            forwarded_modules: Rc::new(RefCell::new(Vec::new())),
            imported_modules: Rc::new(RefCell::new(Vec::new())),
            nested_forwarded_modules: None,
            forwarded_member_sources: ForwardedMemberSources::default(),
        }
    }

    pub fn new_closure(&self) -> Self {
        Self {
            scopes: self.scopes.new_closure(),
            modules: Rc::clone(&self.modules),
            global_modules: self.global_modules.iter().map(Rc::clone).collect(),
            content: self.content.as_ref().map(Rc::clone),
            forwarded_modules: Rc::clone(&self.forwarded_modules),
            imported_modules: Rc::clone(&self.imported_modules),
            nested_forwarded_modules: self.nested_forwarded_modules.as_ref().map(Rc::clone),
            forwarded_member_sources: self.forwarded_member_sources.clone(),
        }
    }

    pub fn for_import(&self) -> Self {
        Self {
            scopes: self.scopes.new_closure(),
            modules: Rc::new(RefCell::new(Modules::new())),
            global_modules: Vec::new(),
            content: self.content.as_ref().map(Rc::clone),
            // Create a new forwarded_modules list for the import context.
            // The imported file's @forward rules will push to this new list,
            // and import_forwards() will process them back into the parent env.
            // Sharing the parent's list would cause @forward to leak directly
            // into the parent, bypassing import_forwards' scoping and shadowing.
            forwarded_modules: Rc::new(RefCell::new(Vec::new())),
            imported_modules: Rc::clone(&self.imported_modules),
            nested_forwarded_modules: self.nested_forwarded_modules.as_ref().map(Rc::clone),
            forwarded_member_sources: self.forwarded_member_sources.clone(),
        }
    }

    pub fn to_dummy_module(&self, span: Span) -> Module {
        Module::Environment {
            scope: ModuleScope::new(),
            upstream: Vec::new(),
            extension_store: ExtensionStore::new(span),
            env: self.clone(),
        }
    }

    /// Makes the members forwarded by [module] available in the current
    /// environment.
    ///
    /// This is called when [module] is `@import`ed.
    pub fn import_forwards(&mut self, _env: Module) {
        if let Module::Environment { env, .. } = _env {
            let mut forwarded = env.forwarded_modules;

            if (*forwarded).borrow().is_empty() {
                return;
            }

            // Omit modules from [forwarded] that are already globally available and
            // forwarded in this module.
            let forwarded_modules = Rc::clone(&self.forwarded_modules);
            if !(*forwarded_modules).borrow().is_empty() {
                let fwd_ptrs: FxHashSet<*const RefCell<Module>> = forwarded_modules
                    .borrow()
                    .iter()
                    .map(Rc::as_ptr)
                    .collect();
                let global_ptrs: FxHashSet<*const RefCell<Module>> = self
                    .global_modules
                    .iter()
                    .map(Rc::as_ptr)
                    .collect();
                let mut x = Vec::new();
                for entry in (*forwarded).borrow().iter() {
                    let ptr = Rc::as_ptr(entry);
                    if !fwd_ptrs.contains(&ptr) || !global_ptrs.contains(&ptr) {
                        x.push(Rc::clone(entry));
                    }
                }

                forwarded = Rc::new(RefCell::new(x));
            }

            let mut forwarded_var_names = FxHashSet::<Identifier>::default();
            let mut forwarded_fn_names = FxHashSet::<Identifier>::default();
            let mut forwarded_mixin_names = FxHashSet::<Identifier>::default();
            for module in forwarded.borrow().iter() {
                let m = (*module).borrow();
                let scope = m.scope();
                forwarded_var_names.extend(scope.variables.keys());
                forwarded_fn_names.extend(scope.functions.keys());
                forwarded_mixin_names.extend(scope.mixins.keys());
            }

            if self.at_root() {
                // Hide members from modules that have already been imported or
                // forwarded that would otherwise conflict with the @imported members.
                (*self.imported_modules).borrow_mut().retain(|module| {
                    ShadowedModule::if_necessary(
                        Rc::clone(module),
                        Some(&forwarded_var_names),
                        Some(&forwarded_fn_names),
                        Some(&forwarded_mixin_names),
                    )
                    .is_none()
                });

                (*self.forwarded_modules).borrow_mut().retain(|module| {
                    ShadowedModule::if_necessary(
                        Rc::clone(module),
                        Some(&forwarded_var_names),
                        Some(&forwarded_fn_names),
                        Some(&forwarded_mixin_names),
                    )
                    .is_none()
                });

                let mut imported_modules = (*self.imported_modules).borrow_mut();
                let mut forwarded_modules = (*self.forwarded_modules).borrow_mut();

                imported_modules.extend(forwarded.borrow().iter().map(Rc::clone));
                forwarded_modules.extend(forwarded.borrow().iter().map(Rc::clone));
            } else {
                self.nested_forwarded_modules
                    .get_or_insert_with(|| {
                        Rc::new(RefCell::new(
                            (0..self.scopes.len())
                                .map(|_| Rc::new(RefCell::new(Vec::new())))
                                .collect(),
                        ))
                    })
                    .borrow_mut()
                    .last_mut()
                    .unwrap()
                    .borrow_mut()
                    .extend(forwarded.borrow().iter().map(Rc::clone));
            }

            // Remove existing member definitions that are now shadowed by the
            // forwarded modules.
            for variable in &forwarded_var_names {
                (*self.scopes.variables)
                    .borrow_mut()
                    .last_mut()
                    .unwrap()
                    .borrow_mut()
                    .remove(variable);
            }
            self.scopes.last_variable_index = None;

            for func in &forwarded_fn_names {
                (*self.scopes.functions)
                    .borrow_mut()
                    .last_mut()
                    .unwrap()
                    .borrow_mut()
                    .remove(func);
            }
            for mixin in &forwarded_mixin_names {
                (*self.scopes.mixins)
                    .borrow_mut()
                    .last_mut()
                    .unwrap()
                    .borrow_mut()
                    .remove(mixin);
            }

        }
    }

    pub fn to_implicit_configuration(&self) -> Configuration {
        let mut configuration = FxHashMap::default();

        // Match dart-sass: iterate scope levels from global (0) to innermost,
        // interleaving module variables and scope-local variables per level.
        // At each level, module variables are added first, then scope variables
        // (which can overwrite them). Inner scopes overwrite outer scopes.
        let variables = (*self.scopes.variables).borrow();
        let nested_forwarded = self.nested_forwarded_modules.as_ref();

        for (i, scope_vars) in variables.iter().enumerate() {
            // Add module variables for this scope level.
            if i == 0 {
                // Global scope: use imported_modules
                for module in self.imported_modules.borrow().iter() {
                    let m = (*module).borrow();
                    for (name, value) in m.scope().variables.iter() {
                        configuration.insert(name, ConfiguredValue::implicit(value));
                    }
                }
            } else if let Some(forwarded) = nested_forwarded {
                // Non-global scope: use nested_forwarded_modules[i]
                // (grass includes an entry for each scope level including global)
                let forwarded_ref = forwarded.borrow();
                if let Some(modules) = forwarded_ref.get(i) {
                    for module in modules.borrow().iter() {
                        let m = (*module).borrow();
                        for (name, value) in m.scope().variables.iter() {
                            configuration.insert(name, ConfiguredValue::implicit(value));
                        }
                    }
                }
            }

            // Add scope-local variables (overwrite module vars at same level).
            let entries = (**scope_vars).borrow();
            for (key, value) in entries.iter() {
                configuration.insert(*key, ConfiguredValue::implicit(value.clone()));
            }
        }

        Configuration::implicit(configuration)
    }

    pub fn forward_module(
        &mut self,
        module: Rc<RefCell<Module>>,
        rule: AstForwardRule,
    ) -> SassResult<()> {
        let new_span = rule.span;
        let view = ForwardedModule::if_necessary(module, rule);

        // Check for conflicts with previously forwarded modules.
        // Uses dart-sass's approach: maintain maps of name → source identity
        // for all previously forwarded members, and check the new module against
        // them. This avoids O(n²) module-pair comparisons.
        self.assert_no_conflicts(&view, new_span)?;

        (*self.forwarded_modules).borrow_mut().push(view);
        Ok(())
    }

    pub fn insert_mixin(&mut self, name: Identifier, mixin: Mixin) {
        self.scopes.insert_mixin(name, mixin);
    }

    pub fn mixin_exists(&self, name: Identifier, span: Span) -> SassResult<bool> {
        if self.scopes.mixin_exists(name) {
            return Ok(true);
        }
        Ok(self
            .get_mixin_from_global_modules(name, span)?
            .is_some())
    }

    pub fn get_mixin(
        &self,
        name: Spanned<Identifier>,
        namespace: Option<Spanned<Identifier>>,
    ) -> SassResult<Mixin> {
        if let Some(namespace) = namespace {
            let modules = (*self.modules).borrow();
            let module = modules.get(namespace.node, namespace.span)?;
            return (*module).borrow().get_mixin(name);
        }

        match self.scopes.get_mixin(name) {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(v) = self.get_mixin_from_global_modules(name.node, name.span)? {
                    return Ok(v);
                }

                Err(e)
            }
        }
    }

    pub fn insert_fn(&mut self, func: SassFunction) {
        self.scopes.insert_fn(func);
    }

    pub fn fn_exists(&self, name: Identifier, span: Span) -> SassResult<bool> {
        if self.scopes.fn_exists(name) {
            return Ok(true);
        }
        Ok(self
            .get_function_from_global_modules(name, span)?
            .is_some())
    }

    pub fn get_fn(
        &self,
        name: Identifier,
        namespace: Option<Spanned<Identifier>>,
        span: Span,
    ) -> SassResult<Option<SassFunction>> {
        if let Some(namespace) = namespace {
            let modules = (*self.modules).borrow();
            let module = modules.get(namespace.node, namespace.span)?;
            return Ok((*module).borrow().get_fn(name));
        }

        match self.scopes.get_fn(name) {
            Some(v) => Ok(Some(v)),
            None => self.get_function_from_global_modules(name, span),
        }
    }

    pub fn var_exists(
        &self,
        name: Identifier,
        namespace: Option<Spanned<Identifier>>,
        span: Span,
    ) -> SassResult<bool> {
        if let Some(namespace) = namespace {
            let modules = (*self.modules).borrow();
            let module = modules.get(namespace.node, namespace.span)?;
            return Ok((*module).borrow().var_exists(name));
        }

        if self.scopes.var_exists(name) {
            return Ok(true);
        }
        Ok(self
            .get_variable_from_global_modules(name, span)?
            .is_some())
    }

    pub fn global_var_exists(&self, name: Identifier, span: Span) -> SassResult<bool> {
        if (*self.global_vars()).borrow().contains_key(&name) {
            return Ok(true);
        }
        Ok(self
            .get_variable_from_global_modules(name, span)?
            .is_some())
    }

    pub fn get_var(
        &mut self,
        name: Spanned<Identifier>,
        namespace: Option<Spanned<Identifier>>,
    ) -> SassResult<Value> {
        if let Some(namespace) = namespace {
            let modules = (*self.modules).borrow();
            let module = modules.get(namespace.node, namespace.span)?;
            return (*module).borrow().get_var(name);
        }

        match self.scopes.get_var(name) {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(v) = self.get_variable_from_global_modules(name.node, name.span)? {
                    Ok(v)
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn insert_var(
        &mut self,
        name: Spanned<Identifier>,
        namespace: Option<Spanned<Identifier>>,
        value: Value,
        is_global: bool,
        in_semi_global_scope: bool,
    ) -> SassResult<()> {
        if let Some(namespace) = namespace {
            let mut modules = (*self.modules).borrow_mut();
            let module = modules.get_mut(namespace.node, namespace.span)?;
            (*module).borrow_mut().update_var(name, value)?;
            return Ok(());
        }

        if is_global || self.at_root() {
            // If this module doesn't already contain a variable named [name], try
            // setting it in a global module.
            if !self.scopes.global_var_exists(name.node) {
                let module_with_name = self.from_one_module(name.node, "variable", name.span, |module| {
                    if module.borrow().var_exists(*name) {
                        Some(Rc::clone(module))
                    } else {
                        None
                    }
                })?;

                if let Some(module_with_name) = module_with_name {
                    module_with_name.borrow_mut().update_var(name, value)?;
                    return Ok(());
                }
            }

            self.scopes.insert_var(0, name.node, value);
            return Ok(());
        }

        let index = self.scopes.find_var(name.node);

        // If the variable isn't found in any local scope, check nested forwarded
        // modules. An @import in a nested scope makes forwarded members available,
        // and assigning to them should update the original module's variable.
        if index.is_none() {
            if let Some(ref nfm) = self.nested_forwarded_modules {
                for modules in nfm.borrow().iter().rev() {
                    for module in modules.borrow().iter().rev() {
                        if module.borrow().var_exists(name.node) {
                            module.borrow_mut().update_var(name, value)?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        let mut index = index.unwrap_or(self.scopes.len() - 1);

        if !in_semi_global_scope && index == 0 {
            index = self.scopes.len() - 1;
        }

        self.scopes.last_variable_index = Some((name.node, index));

        self.scopes.insert_var(index, name.node, value);

        Ok(())
    }

    pub fn at_root(&self) -> bool {
        self.scopes.len() == 1
    }

    pub fn scopes_mut(&mut self) -> &mut Scopes {
        &mut self.scopes
    }

    /// Enter a new scope, managing both variable scopes and nested forwarded modules.
    pub fn scope_enter(&mut self) {
        self.scopes.enter_new_scope();
        if let Some(ref nfm) = self.nested_forwarded_modules {
            nfm.borrow_mut()
                .push(Rc::new(RefCell::new(Vec::new())));
        }
    }

    /// Exit the current scope, managing both variable scopes and nested forwarded modules.
    pub fn scope_exit(&mut self) {
        self.scopes.exit_scope();
        if let Some(ref nfm) = self.nested_forwarded_modules {
            nfm.borrow_mut().pop();
        }
    }

    pub fn global_vars(&self) -> Rc<RefCell<FxHashMap<Identifier, Value>>> {
        self.scopes.global_variables()
    }

    pub fn global_mixins(&self) -> Rc<RefCell<FxHashMap<Identifier, Mixin>>> {
        self.scopes.global_mixins()
    }

    pub fn global_functions(&self) -> Rc<RefCell<FxHashMap<Identifier, SassFunction>>> {
        self.scopes.global_functions()
    }

    /// Check that `new_module` doesn't conflict with any already-forwarded modules,
    /// then add its members to the cached source identity map.
    fn assert_no_conflicts(
        &mut self,
        new_module: &Rc<RefCell<Module>>,
        new_span: Span,
    ) -> SassResult<()> {
        let cache = &self.forwarded_member_sources;
        let cache_empty = cache.variables.is_empty()
            && cache.functions.is_empty()
            && cache.mixins.is_empty();

        if cache_empty {
            // First forwarded module: no conflicts possible, but still need to
            // collect sources for future checks. Use the scope keys directly
            // (cheaper than full tree walk) since we only need names, not identities.
            // We'll compute identities lazily on the first actual collision.
            //
            // Actually, we need identities to compare against future modules.
            // Use the full tree walk.
            let new_sources = collect_source_identities(new_module);
            self.forwarded_member_sources = new_sources;
            return Ok(());
        }

        // Batch-compute source identities for the new module.
        let new_sources = collect_source_identities(new_module);

        // Check against cached existing sources.
        for (name, new_source) in &new_sources.variables {
            if let Some(&existing_source) = self.forwarded_member_sources.variables.get(name) {
                if *new_source != existing_source {
                    return Err((
                        format!("Two forwarded modules both define a variable named ${name}."),
                        new_span,
                    )
                        .into());
                }
            }
        }
        for (name, new_source) in &new_sources.functions {
            if let Some(&existing_source) = self.forwarded_member_sources.functions.get(name) {
                if *new_source != existing_source {
                    return Err((
                        format!("Two forwarded modules both define a function named {name}."),
                        new_span,
                    )
                        .into());
                }
            }
        }
        for (name, new_source) in &new_sources.mixins {
            if let Some(&existing_source) = self.forwarded_member_sources.mixins.get(name) {
                if *new_source != existing_source {
                    return Err((
                        format!("Two forwarded modules both define a mixin named {name}."),
                        new_span,
                    )
                        .into());
                }
            }
        }

        // Merge new sources into the cache.
        for (name, source) in new_sources.variables {
            self.forwarded_member_sources.variables.entry(name).or_insert(source);
        }
        for (name, source) in new_sources.functions {
            self.forwarded_member_sources.functions.entry(name).or_insert(source);
        }
        for (name, source) in new_sources.mixins {
            self.forwarded_member_sources.mixins.entry(name).or_insert(source);
        }

        Ok(())
    }

    fn get_variable_from_global_modules(&self, name: Identifier, span: Span) -> SassResult<Option<Value>> {
        self.from_one_module(name, "variable", span, |module| {
            (**module).borrow().get_var_no_err(name)
        })
    }

    fn get_function_from_global_modules(&self, name: Identifier, span: Span) -> SassResult<Option<SassFunction>> {
        self.from_one_module(name, "function", span, |module| (**module).borrow().get_fn(name))
    }

    fn get_mixin_from_global_modules(&self, name: Identifier, span: Span) -> SassResult<Option<Mixin>> {
        self.from_one_module(name, "mixin", span, |module| {
            (**module).borrow().get_mixin_no_err(name)
        })
    }

    pub fn add_module(
        &mut self,
        namespace: Option<Identifier>,
        module: Rc<RefCell<Module>>,
        span: Span,
    ) -> SassResult<()> {
        match namespace {
            Some(namespace) => {
                (*self.modules)
                    .borrow_mut()
                    .insert(namespace, module, span)?;
            }
            None => {
                for name in (*self.scopes.global_variables()).borrow().keys() {
                    if (*module).borrow().var_exists(*name) {
                        return Err((
                            format!("This module and the new module both define a variable named \"${name}\".", name = name)
                        , span).into());
                    }
                }

                if !self.global_modules.iter().any(|m| Rc::ptr_eq(m, &module)) {
                    self.global_modules.push(module);
                }
            }
        }

        Ok(())
    }

    pub fn to_module_with_upstream(
        self,
        extension_store: ExtensionStore,
        upstream: Vec<Rc<RefCell<Module>>>,
    ) -> Rc<RefCell<Module>> {
        debug_assert!(self.at_root());

        Rc::new(RefCell::new(Module::new_env_with_upstream(
            self,
            extension_store,
            upstream,
        )))
    }

    fn from_one_module<T>(
        &self,
        _name: Identifier,
        ty: &str,
        span: Span,
        callback: impl Fn(&Rc<RefCell<Module>>) -> Option<T>,
    ) -> SassResult<Option<T>> {
        if let Some(nested_forwarded_modules) = &self.nested_forwarded_modules {
            for modules in nested_forwarded_modules.borrow().iter().rev() {
                for module in modules.borrow().iter().rev() {
                    if let Some(value) = callback(module) {
                        return Ok(Some(value));
                    }
                }
            }
        }

        for module in self.imported_modules.borrow().iter() {
            if let Some(value) = callback(module) {
                return Ok(Some(value));
            }
        }

        let mut value: Option<T> = None;

        for module in self.global_modules.iter() {
            let value_in_module = match callback(module) {
                Some(v) => v,
                None => continue,
            };

            if value.is_some() {
                return Err((
                    format!("This {ty} is available from multiple global modules."),
                    span,
                )
                    .into());
            }

            value = Some(value_in_module);
        }

        Ok(value)
    }
}

/// Batch-collect source identities for ALL members of a module in one tree walk.
/// Returns maps of name → source_ptr for variables, functions, and mixins.
/// Names are as they appear from outside the module (with prefixes applied).
fn collect_source_identities(module: &Rc<RefCell<Module>>) -> ForwardedMemberSources {
    let mut result = ForwardedMemberSources::default();
    collect_inner(module, &mut result);
    result
}

fn collect_inner(
    module: &Rc<RefCell<Module>>,
    result: &mut ForwardedMemberSources,
) {
    let borrowed = module.borrow();

    match &*borrowed {
        Module::Forwarded(fwd) => {
            let has_prefix = fwd.forward_rule.prefix.is_some();
            let has_filter = fwd.forward_rule.shown_variables.is_some()
                || fwd.forward_rule.shown_mixins_and_functions.is_some()
                || fwd.forward_rule.hidden_variables.as_ref().is_some_and(|s| !s.is_empty())
                || fwd.forward_rule.hidden_mixins_and_functions.as_ref().is_some_and(|s| !s.is_empty());

            let inner = Rc::clone(&fwd.inner);

            if !has_prefix && !has_filter {
                // Fast path: no prefix or show/hide — just pass through
                drop(borrowed);
                collect_inner(&inner, result);
            } else {
                let prefix = fwd.forward_rule.prefix.clone();

                // Get visible keys (already prefixed by the scope's MapView chain)
                let scope = borrowed.scope();
                let visible_var_keys: FxHashSet<Identifier> =
                    scope.variables.keys().into_iter().collect();
                let visible_fn_keys: FxHashSet<Identifier> =
                    scope.functions.keys().into_iter().collect();
                let visible_mixin_keys: FxHashSet<Identifier> =
                    scope.mixins.keys().into_iter().collect();

                drop(borrowed);

                // Recurse into the inner module (gets un-prefixed names)
                let mut inner_result = ForwardedMemberSources::default();
                collect_inner(&inner, &mut inner_result);

                // Apply this forward's prefix and filter by visibility
                for (inner_name, source) in inner_result.variables {
                    let outer_name = match &prefix {
                        Some(p) => Identifier::from(format!("{p}{inner_name}")),
                        None => inner_name,
                    };
                    if visible_var_keys.contains(&outer_name) {
                        result.variables.entry(outer_name).or_insert(source);
                    }
                }
                for (inner_name, source) in inner_result.functions {
                    let outer_name = match &prefix {
                        Some(p) => Identifier::from(format!("{p}{inner_name}")),
                        None => inner_name,
                    };
                    if visible_fn_keys.contains(&outer_name) {
                        result.functions.entry(outer_name).or_insert(source);
                    }
                }
                for (inner_name, source) in inner_result.mixins {
                    let outer_name = match &prefix {
                        Some(p) => Identifier::from(format!("{p}{inner_name}")),
                        None => inner_name,
                    };
                    if visible_mixin_keys.contains(&outer_name) {
                        result.mixins.entry(outer_name).or_insert(source);
                    }
                }
            }
        }
        Module::Shadowed(shd) => {
            let scope = borrowed.scope();
            let visible_var_keys: FxHashSet<Identifier> =
                scope.variables.keys().into_iter().collect();
            let visible_fn_keys: FxHashSet<Identifier> =
                scope.functions.keys().into_iter().collect();
            let visible_mixin_keys: FxHashSet<Identifier> =
                scope.mixins.keys().into_iter().collect();

            let inner = Rc::clone(&shd.inner);
            drop(borrowed);

            let mut inner_result = ForwardedMemberSources::default();
            collect_inner(&inner, &mut inner_result);

            for (name, source) in inner_result.variables {
                if visible_var_keys.contains(&name) {
                    result.variables.entry(name).or_insert(source);
                }
            }
            for (name, source) in inner_result.functions {
                if visible_fn_keys.contains(&name) {
                    result.functions.entry(name).or_insert(source);
                }
            }
            for (name, source) in inner_result.mixins {
                if visible_mixin_keys.contains(&name) {
                    result.mixins.entry(name).or_insert(source);
                }
            }
        }
        Module::Environment { env, .. } => {
            let source_ptr = Rc::as_ptr(module);

            // Collect from forwarded modules first (they take precedence for identity)
            let forwarded = env.forwarded_modules.borrow();
            for fwd_module in forwarded.iter() {
                collect_inner(fwd_module, result);
            }
            drop(forwarded);

            // Add locally defined PUBLIC members (not already claimed by forwarded modules)
            let local_vars = env.scopes.global_variables();
            for name in (*local_vars).borrow().keys() {
                if name.is_public() {
                    result.variables.entry(*name).or_insert(source_ptr);
                }
            }
            let local_fns = env.scopes.global_functions();
            for name in (*local_fns).borrow().keys() {
                if name.is_public() {
                    result.functions.entry(*name).or_insert(source_ptr);
                }
            }
            let local_mixins = env.scopes.global_mixins();
            for name in (*local_mixins).borrow().keys() {
                if name.is_public() {
                    result.mixins.entry(*name).or_insert(source_ptr);
                }
            }
        }
        Module::Builtin { .. } => {
            let source_ptr = Rc::as_ptr(module);
            let scope = borrowed.scope();
            for name in scope.variables.keys() {
                result.variables.entry(name).or_insert(source_ptr);
            }
            for name in scope.functions.keys() {
                result.functions.entry(name).or_insert(source_ptr);
            }
            for name in scope.mixins.keys() {
                result.mixins.entry(name).or_insert(source_ptr);
            }
        }
    }
}
