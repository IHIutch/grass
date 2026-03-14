use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fmt,
    iter::FromIterator,
    mem,
    path::{Path, PathBuf},
    rc::Rc,
};

use codemap::{CodeMap, Span, Spanned};
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};

/// IndexSet using FxHash instead of SipHash for faster hashing.
type FxIndexSet<V> = indexmap::IndexSet<V, FxBuildHasher>;

use crate::{
    ast::*,
    builtin::{
        meta::if_arguments,
        modules::{
            declare_module_color, declare_module_list, declare_module_map, declare_module_math,
            declare_module_meta, declare_module_selector, declare_module_string, Module,
        },
        GLOBAL_FUNCTIONS,
    },
    common::{unvendor, BinaryOp, Identifier, ListSeparator, QuoteKind, UnaryOp},
    error::{SassError, SassResult},
    interner::InternedString,
    lexer::Lexer,
    parse::{
        AtRootQueryParser, CssParser, KeyframesSelectorParser, SassParser, ScssParser,
        StylesheetParser,
    },
    selector::{
        ComplexSelectorComponent, ExtendRule, ExtendedSelector, Extension, ExtensionStore,
        SelectorList, SelectorParser, SimpleSelector,
    },
    utils::{to_sentence, trim_ascii},
    value::{
        ArgList, CalculationArg, CalculationName, Number, SassCalculation, SassFunction, SassMap,
        SassNumber, UserDefinedFunction, Value,
    },
    ContextFlags, InputSyntax, Options,
};

use super::{
    bin_op::{add, cmp, div, mul, rem, single_eq, sub},
    css_tree::{CssTree, CssTreeIdx},
    env::Environment,
};

/// Result of evaluating an if() condition.
/// Sass atoms evaluate to True/False; CSS atoms remain as CSS.
enum ConditionResult {
    True,
    False,
    Css(IfCondition<'static>),
}

/// Check if a condition tree contains any sass() atoms (crossing paren boundaries).
fn condition_has_sass(cond: &IfCondition<'static>) -> bool {
    match cond {
        IfCondition::Atom(IfConditionAtom::Sass(_, _)) => true,
        IfCondition::Atom(_) => false,
        IfCondition::Else => false,
        IfCondition::Not(inner, _) | IfCondition::Paren(inner) => condition_has_sass(inner),
        IfCondition::And(ops) | IfCondition::Or(ops) => ops.iter().any(condition_has_sass),
    }
}

/// Check if a condition tree has raw substitutions (not crossing paren boundaries).
fn condition_has_raw(cond: &IfCondition<'static>) -> bool {
    match cond {
        IfCondition::Atom(IfConditionAtom::CssRaw(_, _)) => true,
        IfCondition::Atom(IfConditionAtom::Interp(_, _)) => true,
        IfCondition::Atom(_) => false,
        IfCondition::Else => false,
        IfCondition::Not(inner, _) => condition_has_raw(inner),
        IfCondition::Paren(_) => false, // Don't cross paren boundary
        IfCondition::And(ops) | IfCondition::Or(ops) => ops.iter().any(condition_has_raw),
    }
}

/// Unwrap a Paren wrapper — used when simplifying And/Or to a single operand.
fn unwrap_paren(cond: IfCondition<'static>) -> IfCondition<'static> {
    match cond {
        IfCondition::Paren(inner) => *inner,
        other => other,
    }
}

pub(crate) trait UserDefinedCallable {
    fn name(&self) -> Identifier;
    fn arguments(&self) -> &ArgumentDeclaration<'static>;
}

impl UserDefinedCallable for AstFunctionDecl<'static> {
    fn name(&self) -> Identifier {
        self.name.node
    }

    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.arguments
    }
}

impl UserDefinedCallable for Rc<AstFunctionDecl<'static>> {
    fn name(&self) -> Identifier {
        self.name.node
    }

    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.arguments
    }
}

impl UserDefinedCallable for AstMixin<'static> {
    fn name(&self) -> Identifier {
        self.name
    }

    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.args
    }
}

impl UserDefinedCallable for Rc<CallableContentBlock> {
    fn name(&self) -> Identifier {
        Identifier::from("@content")
    }

    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.content.args
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CallableContentBlock {
    content: AstContentBlock<'static>,
    env: Environment,
}

/// Evaluation context of the current execution
#[derive(Debug)]
pub struct Visitor<'a> {
    pub(crate) declaration_name: Option<String>,
    pub(crate) flags: ContextFlags,
    pub(crate) env: Environment,
    pub(crate) style_rule_ignoring_at_root: Option<ExtendedSelector>,
    /// The original (pre-extension) selector for the current style rule.
    /// Used by `&` in value context, matching dart-sass's `originalSelector`.
    pub(crate) original_selector: Option<SelectorList>,
    // avoid emitting duplicate warnings for the same span
    pub(crate) warnings_emitted: FxHashSet<Span>,
    pub(crate) media_queries: Option<Vec<MediaQuery>>,
    pub(crate) media_query_sources: Option<FxIndexSet<MediaQuery>>,
    pub(crate) extender: ExtensionStore,

    /// Modules loaded via @use during the current module's evaluation.
    /// Used to track upstream dependencies for per-module @extend scoping.
    pub(crate) upstream_modules: Vec<Rc<RefCell<Module>>>,

    /// Maps module URLs to their root-level CSS tree indices.
    /// Used to clone module CSS when the same module is loaded via @import.
    module_css_indices: FxHashMap<PathBuf, Vec<CssTreeIdx>>,

    /// Modules that were first loaded inside an @import context.
    /// When these modules are later @use'd in a non-import context, their
    /// CSS must be cloned so extends from the @import don't leak.
    modules_loaded_in_import: FxHashSet<PathBuf>,

    /// When true, cached modules should have their CSS cloned (not shared)
    /// so that @extend mutations are isolated per-import context.
    in_import_context: bool,

    /// Shared clone state across all module clones within the same @import.
    /// Prevents double-cloning when diamond dependencies share upstream modules.
    import_selector_map: FxHashMap<usize, ExtendedSelector>,
    import_cloned_modules: FxHashMap<usize, Rc<RefCell<Module>>>,
    import_cloned_css: FxHashSet<CssTreeIdx>,

    /// The complete file path of the current file being visited. Imports are
    /// resolved relative to this path
    pub current_import_path: PathBuf,
    pub(crate) is_plain_css: bool,
    plain_css_style_rule_depth: u32,
    pub(crate) modules: FxHashMap<PathBuf, Rc<RefCell<Module>>>,
    /// Reverse map from module Arc pointer → URL for O(1) lookup in collect_css_indices_transitive.
    module_ptr_to_url: FxHashMap<usize, PathBuf>,
    /// Configuration used when each module was first loaded via execute().
    /// Used to detect "was already loaded, so it can't be configured" errors.
    module_configurations: FxHashMap<PathBuf, Option<Rc<RefCell<Configuration>>>>,
    pub(crate) active_modules: FxHashSet<PathBuf>,
    css_tree: CssTree,
    parent: Option<CssTreeIdx>,
    pub(crate) configuration: Rc<RefCell<Configuration>>,
    combined_import_section: Vec<CssStmt>,
    pending_import_items: Vec<CssStmt>,
    in_module_import_section: bool,
    module_depth: usize,
    /// Number of trailing import-section items (comments) flushed to css_tree
    /// at the top level. These may need to be moved before out-of-order imports
    /// in finish().
    import_section_tree_count: usize,
    /// Whether any out-of-order imports were added to combined_import_section.
    has_out_of_order_imports: bool,
    pub options: &'a Options<'a>,
    pub(crate) map: &'a mut CodeMap,
    pub(crate) arena: &'a bumpalo::Bump,
    // todo: remove
    empty_span: Span,
    import_cache: FxHashMap<PathBuf, StyleSheet<'static>>,
    /// As a simple heuristic, we don't cache the results of an import unless it
    /// has been seen in the past. In the majority of cases, files are imported
    /// at most once.
    files_seen: FxHashSet<PathBuf>,
    /// Cache for resolved import paths, keyed by (context_dir, requested path, for_import flag).
    /// Avoids redundant filesystem probing for the same import path from the same context.
    import_path_cache: FxHashMap<(PathBuf, PathBuf, bool), SassResult<Option<PathBuf>>>,
    /// Cache for canonicalized paths to avoid repeated syscalls.
    canonicalize_cache: FxHashMap<PathBuf, PathBuf>,
}

impl<'a> Visitor<'a> {
    pub fn new(
        path: &Path,
        options: &'a Options<'a>,
        map: &'a mut CodeMap,
        arena: &'a bumpalo::Bump,
        empty_span: Span,
    ) -> Self {
        let mut flags = ContextFlags::empty();
        flags.set(ContextFlags::IN_SEMI_GLOBAL_SCOPE, true);

        let extender = ExtensionStore::new(empty_span);

        let current_import_path = path.to_path_buf();

        Self {
            declaration_name: None,
            style_rule_ignoring_at_root: None,
            original_selector: None,
            flags,
            warnings_emitted: FxHashSet::default(),
            media_queries: None,
            media_query_sources: None,
            env: Environment::new(),
            extender,
            upstream_modules: Vec::new(),
            module_css_indices: FxHashMap::default(),
            modules_loaded_in_import: FxHashSet::default(),
            in_import_context: false,
            import_selector_map: FxHashMap::default(),
            import_cloned_modules: FxHashMap::default(),
            import_cloned_css: FxHashSet::default(),
            css_tree: CssTree::new(),
            parent: None,
            current_import_path,
            configuration: Rc::new(RefCell::new(Configuration::empty())),
            is_plain_css: false,
            plain_css_style_rule_depth: 0,
            combined_import_section: Vec::new(),
            pending_import_items: Vec::new(),
            in_module_import_section: true,
            module_depth: 0,
            import_section_tree_count: 0,
            has_out_of_order_imports: false,
            modules: FxHashMap::default(),
            module_ptr_to_url: FxHashMap::default(),
            module_configurations: FxHashMap::default(),
            active_modules: FxHashSet::default(),
            options,
            empty_span,
            map,
            arena,
            import_cache: FxHashMap::default(),
            files_seen: FxHashSet::default(),
            import_path_cache: FxHashMap::default(),
            canonicalize_cache: FxHashMap::default(),
        }
    }

    /// Cached version of `fs.canonicalize()` to avoid repeated syscalls.
    fn canonicalize(&mut self, path: &Path) -> PathBuf {
        if let Some(cached) = self.canonicalize_cache.get(path) {
            return cached.clone();
        }
        let result = self
            .options
            .fs
            .canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf());
        self.canonicalize_cache
            .insert(path.to_path_buf(), result.clone());
        result
    }

    pub(crate) fn visit_stylesheet(&mut self, style_sheet: &StyleSheet<'static>) -> SassResult<()> {
        self.active_modules.insert(style_sheet.url.clone());
        let was_in_plain_css = self.is_plain_css;
        let old_plain_css_depth = self.plain_css_style_rule_depth;
        self.is_plain_css = style_sheet.is_plain_css;
        if style_sheet.is_plain_css {
            self.plain_css_style_rule_depth = 0;
        }
        let old_import_path = mem::replace(&mut self.current_import_path, style_sheet.url.clone());

        for stmt in style_sheet.body {
            let result = self.visit_stmt(stmt)?;
            debug_assert!(result.is_none());
        }

        self.current_import_path = old_import_path;
        self.is_plain_css = was_in_plain_css;
        self.plain_css_style_rule_depth = old_plain_css_depth;

        self.active_modules.remove(&style_sheet.url);

        Ok(())
    }

    pub(crate) fn finish(mut self) -> SassResult<Vec<CssStmt>> {
        self.flush_pending_imports(true);
        self.extend_modules()?;
        let mut finished_tree = self.css_tree.finish();
        if self.combined_import_section.is_empty() {
            Ok(finished_tree)
        } else {
            // If there are leading items in css_tree that came from the
            // top-level import section flush (e.g., comments before an
            // out-of-order @import), move them before combined so they
            // appear in front of the out-of-order imports (issue_469).
            if self.has_out_of_order_imports
                && self.import_section_tree_count > 0
                && self.import_section_tree_count <= finished_tree.len()
            {
                let rest = finished_tree.split_off(self.import_section_tree_count);
                let mut result = finished_tree; // import-section comments
                result.append(&mut self.combined_import_section); // imports
                result.extend(rest); // remaining CSS
                Ok(result)
            } else {
                self.combined_import_section.append(&mut finished_tree);
                Ok(self.combined_import_section)
            }
        }
    }

    /// Returns the index after the last @import in a sequence of imports and
    /// comments. Items before this index belong in the import section; items
    /// at or after belong in the CSS section.
    fn index_after_imports(items: &[CssStmt]) -> usize {
        let mut last_import: i64 = -1;
        for (i, item) in items.iter().enumerate() {
            match item {
                CssStmt::Import(..) => last_import = i as i64,
                CssStmt::Comment(..) => continue,
                _ => break,
            }
        }
        (last_import + 1) as usize
    }

    /// Flush pending import-section items: imports and their interleaved
    /// comments go to `combined_import_section`, while trailing comments
    /// (after the last import) go to the CSS tree.
    ///
    /// When `end_of_module` is true and the pending items contain no imports
    /// (only comments), all items go to `combined_import_section` to maintain
    /// correct topological ordering for comment-only modules.
    fn flush_pending_imports(&mut self, end_of_module: bool) {
        if self.pending_import_items.is_empty() {
            return;
        }
        let pending = mem::take(&mut self.pending_import_items);
        let idx = Self::index_after_imports(&pending);
        for (i, item) in pending.into_iter().enumerate() {
            if i < idx {
                self.combined_import_section.push(item);
            } else if end_of_module && idx == 0 {
                // Module had only comments, no imports — keep in combined
                // so they appear in the correct topological position.
                self.combined_import_section.push(item);
            } else {
                if !end_of_module && self.module_depth == 0 {
                    self.import_section_tree_count += 1;
                }
                self.css_tree.add_stmt(item, None);
            }
        }
    }

    /// Clone a cached module's CSS and ExtensionStore for @import isolation.
    /// Recursively clones the entire upstream module graph so that extensions
    /// flow through cloned copies independently from the originals.
    /// Uses shared clone state (import_selector_map, import_cloned_modules,
    /// import_cloned_css) to avoid double-cloning diamond dependencies.
    fn clone_module_for_import(
        &mut self,
        url: &Path,
        cached: &Rc<RefCell<Module>>,
    ) -> (Rc<RefCell<Module>>, bool) {
        // Collect ALL CSS indices transitively: this module + all upstream modules
        let mut all_css_indices = Vec::new();
        let mut visited_urls = FxHashSet::default();
        self.collect_css_indices_transitive(url, &mut all_css_indices, &mut visited_urls);

        if all_css_indices.is_empty() {
            return (Rc::clone(cached), false);
        }

        // Only clone CSS indices that haven't been cloned yet in this @import context
        for idx in &all_css_indices {
            if !self.import_cloned_css.contains(idx) {
                self.css_tree
                    .clone_subtree(*idx, CssTree::ROOT, &mut self.import_selector_map);
                self.import_cloned_css.insert(*idx);
            }
        }

        if self.import_selector_map.is_empty() {
            return (Rc::clone(cached), false);
        }

        // Recursively clone the entire module graph with remapped selectors,
        // reusing already-cloned modules from the shared state.
        let result = self.clone_module_recursive_shared(cached);

        (result, true)
    }

    /// Recursively clone a module and all its upstream modules, using the shared
    /// import_cloned_modules and import_selector_map fields to deduplicate
    /// and import_selector_map fields to deduplicate across diamond dependencies.
    fn clone_module_recursive_shared(
        &mut self,
        module: &Rc<RefCell<Module>>,
    ) -> Rc<RefCell<Module>> {
        let ptr = Rc::as_ptr(module) as usize;

        if let Some(existing) = self.import_cloned_modules.get(&ptr) {
            return Rc::clone(existing);
        }

        // Extract upstream list and check if it's an Environment module
        let (upstream, is_env) = {
            let m = module.borrow();
            match &*m {
                Module::Environment { upstream, .. } => (upstream.clone(), true),
                _ => (Vec::new(), false),
            }
        };

        if !is_env {
            return Rc::clone(module);
        }

        // Recursively clone upstream modules (borrow of module is dropped)
        let cloned_upstream: Vec<Rc<RefCell<Module>>> = upstream
            .iter()
            .map(|up| self.clone_module_recursive_shared(up))
            .collect();

        // Re-borrow to clone extension store and scope
        let m = module.borrow();
        let cloned = if let Module::Environment { extension_store, .. } = &*m {
            let cloned_store = extension_store.clone_for_import(&self.import_selector_map);
            Rc::new(RefCell::new(Module::Environment {
                scope: m.scope().clone(),
                upstream: cloned_upstream,
                extension_store: cloned_store,
                env: Environment::new(),
            }))
        } else {
            unreachable!()
        };
        drop(m);

        self.import_cloned_modules.insert(ptr, Rc::clone(&cloned));
        cloned
    }

    /// Recursively collect CSS tree indices for a module and all its upstream modules.
    fn collect_css_indices_transitive(
        &self,
        url: &Path,
        indices: &mut Vec<CssTreeIdx>,
        visited: &mut FxHashSet<PathBuf>,
    ) {
        if !visited.insert(url.to_path_buf()) {
            return;
        }

        // Add this module's CSS indices
        if let Some(css_indices) = self.module_css_indices.get(url) {
            indices.extend(css_indices);
        }

        // Recurse into upstream modules using the pre-built pointer→URL map
        if let Some(module) = self.modules.get(url) {
            let m = module.borrow();
            if let Module::Environment { upstream, .. } = &*m {
                for up in upstream {
                    let up_ptr = Rc::as_ptr(up) as usize;
                    if let Some(up_url) = self.module_ptr_to_url.get(&up_ptr) {
                        self.collect_css_indices_transitive(up_url, indices, visited);
                    }
                }
            }
        }
    }

    /// Propagate @extend rules between modules according to the @use
    /// dependency graph. Extensions flow from downstream modules (those that
    /// @use others) to upstream modules (those being @use'd).
    ///
    /// Per-module unsatisfied extend checks happen in execute().
    /// Root unsatisfied extends are checked here before propagation.
    fn extend_modules(&mut self) -> SassResult<()> {
        // If no modules were loaded, just check root's own extends.
        if self.upstream_modules.is_empty() {
            return self.extender.check_unsatisfied_extends();
        }

        // Build downstream-first topological order.
        let mut sorted: Vec<Rc<RefCell<Module>>> = Vec::new();
        let mut seen: FxHashSet<*const RefCell<Module>> = FxHashSet::default();

        fn visit_module(
            module: &Rc<RefCell<Module>>,
            sorted: &mut Vec<Rc<RefCell<Module>>>,
            seen: &mut FxHashSet<*const RefCell<Module>>,
        ) {
            let ptr = Rc::as_ptr(module);
            if !seen.insert(ptr) {
                return;
            }

            let upstream_modules: Vec<Rc<RefCell<Module>>> = {
                let m = module.borrow();
                if let Module::Environment { upstream, .. } = &*m {
                    upstream.clone()
                } else {
                    Vec::new()
                }
            };

            for up in &upstream_modules {
                visit_module(up, sorted, seen);
            }
            // Push upstream-first; we reverse after to get downstream-first order.
            sorted.push(Rc::clone(module));
        }

        for module in &self.upstream_modules {
            visit_module(module, &mut sorted, &mut seen);
        }
        // Reverse to get downstream-first order (visit_module pushes upstream-first).
        sorted.reverse();

        // Map from module pointer → list of cloned downstream ExtensionStores
        // to apply to that module.
        let mut downstream_stores: FxHashMap<*const RefCell<Module>, Vec<ExtensionStore>> =
            FxHashMap::default();

        // Collect unsatisfied extensions (dart-sass style).
        let mut unsatisfied: Vec<Extension> = Vec::new();

        // Root's unsatisfied extends: targets not in root's own selectors.
        let root_selectors = self.extender.simple_selectors();
        unsatisfied.extend(
            self.extender
                .extensions_where_target(|t| !root_selectors.contains(t)),
        );

        // Register root's extensions as downstream of root's upstream modules.
        if !self.extender.is_empty() {
            let root_store_clone = self.extender.clone();
            for upstream in &self.upstream_modules {
                let up_ptr = Rc::as_ptr(upstream);
                downstream_stores
                    .entry(up_ptr)
                    .or_default()
                    .push(root_store_clone.clone());
            }
        }

        // Process modules in downstream-first order, propagating extensions.
        for module_ref in &sorted {
            let ptr = Rc::as_ptr(module_ref);

            // Get upstream pointers before mutations.
            let upstream_ptrs = {
                let module = module_ref.borrow();
                if let Module::Environment { upstream, .. } = &*module {
                    upstream.iter().map(|u| Rc::as_ptr(u)).collect::<Vec<_>>()
                } else {
                    continue;
                }
            };

            // Collect this module's original selectors before applying downstream.
            let original_selectors = {
                let module = module_ref.borrow();
                if let Module::Environment {
                    extension_store, ..
                } = &*module
                {
                    extension_store.simple_selectors()
                } else {
                    continue;
                }
            };

            // Collect this module's unsatisfied extends.
            {
                let module = module_ref.borrow();
                if let Module::Environment {
                    extension_store, ..
                } = &*module
                {
                    unsatisfied.extend(
                        extension_store
                            .extensions_where_target(|t| !original_selectors.contains(t)),
                    );
                }
            }

            // Apply downstream extension stores to this module.
            if let Some(stores) = downstream_stores.remove(&ptr) {
                let store_refs: Vec<&ExtensionStore> = {
                    let mut v = Vec::with_capacity(stores.len());
                    v.extend(stores.iter());
                    v
                };
                let mut module = module_ref.borrow_mut();
                if let Module::Environment {
                    extension_store, ..
                } = &mut *module
                {
                    extension_store.add_extensions(&store_refs)?;
                }
            }

            // Register this module's store as downstream of its upstreams.
            {
                let module = module_ref.borrow();
                if let Module::Environment {
                    extension_store, ..
                } = &*module
                {
                    if !extension_store.is_empty() {
                        let store_clone = extension_store.clone();
                        drop(module);
                        for up_ptr in &upstream_ptrs {
                            downstream_stores
                                .entry(*up_ptr)
                                .or_default()
                                .push(store_clone.clone());
                        }
                    }
                }
            }

            // Remove now-satisfied extends: any whose target is in this
            // module's selectors. Private placeholders can never be satisfied
            // cross-module — they stay unsatisfied.
            unsatisfied.retain(|ext| {
                if let Some(ref target) = ext.target {
                    target.is_private_placeholder() || !original_selectors.contains(target)
                } else {
                    false
                }
            });
        }

        // Report first unsatisfied extend as error.
        if let Some(ext) = unsatisfied.first() {
            let target_str = ext
                .target
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_default();

            return Err((
                format!(
                    "The target selector was not found.\nUse \"@extend {} !optional\" to avoid this error.",
                    target_str
                ),
                ext.span,
            )
                .into());
        }

        Ok(())
    }

    fn visit_return_rule(&mut self, ret: AstReturn<'static>) -> SassResult<Option<Value>> {
        let val = self.visit_expr(ret.val)?;

        Ok(Some(self.without_slash(val)))
    }

    pub(crate) fn visit_stmt_arc(&mut self, stmt: &AstStmt<'static>) -> SassResult<Option<Value>> {
        self.visit_stmt_ref(stmt)
    }

    /// Visit a statement by reference, avoiding deep clones for common variants.
    /// Used by loop body and function body iteration to eliminate the Rc::unwrap_or_clone
    /// deep clone that happened on every iteration.
    pub(crate) fn visit_stmt_ref(&mut self, stmt: &AstStmt<'static>) -> SassResult<Option<Value>> {
        match stmt {
            AstStmt::SilentComment(..) => Ok(None),
            AstStmt::VariableDecl(decl) => self.visit_variable_decl_ref(decl),
            AstStmt::Return(ret) => {
                let val = self.visit_expr_ref(&ret.val)?;
                Ok(Some(self.without_slash(val)))
            }
            AstStmt::Style(style) => self.visit_style_ref(style),
            AstStmt::If(if_stmt) => self.visit_if_stmt_ref(if_stmt),
            AstStmt::LoudComment(comment) => self.visit_loud_comment_ref(comment),
            AstStmt::Warn(warn) => {
                if self.warnings_emitted.insert(warn.span) {
                    let value = self.visit_expr_ref(&warn.value)?;
                    let message = value.to_css_string(warn.span, self.options.is_compressed())?;
                    self.emit_warning(&message, warn.span);
                }
                Ok(None)
            }
            AstStmt::Debug(debug) => {
                if !self.options.quiet {
                    let message = self.visit_expr_ref(&debug.value)?;
                    let message = message.inspect(debug.span)?;
                    let loc = self.map.look_up_span(debug.span);
                    self.options.logger.debug(loc, message.as_str());
                }
                Ok(None)
            }
            AstStmt::ErrorRule(err) => {
                let value = self.visit_expr_ref(&err.value)?
                    .inspect(err.span)?;
                Err((value, err.span).into())
            }
            // For remaining variants, clone and delegate to owned visitor.
            // Cloning is cheap: arena refs are just pointer copies.
            AstStmt::RuleSet(ruleset) => self.visit_ruleset(ruleset.clone()),
            AstStmt::For(for_stmt) => self.visit_for_stmt(*for_stmt.clone()),
            AstStmt::Each(each_stmt) => self.visit_each_stmt(*each_stmt.clone()),
            AstStmt::Media(media_rule) => self.visit_media_rule(media_rule.clone()),
            AstStmt::Include(include_stmt) => self.visit_include_stmt(*include_stmt.clone()),
            AstStmt::While(while_stmt) => self.visit_while_stmt(while_stmt),
            AstStmt::FunctionDecl(func) => {
                self.visit_function_decl(func.clone());
                Ok(None)
            }
            AstStmt::Mixin(mixin) => {
                self.visit_mixin_decl(mixin.clone());
                Ok(None)
            }
            AstStmt::ContentRule(content_rule) => self.visit_content_rule(*content_rule.clone()),
            AstStmt::UnknownAtRule(unknown_at_rule) => self.visit_unknown_at_rule(*unknown_at_rule.clone()),
            AstStmt::Extend(extend_rule) => self.visit_extend_rule(extend_rule.clone()),
            AstStmt::AtRootRule(at_root_rule) => self.visit_at_root_rule(at_root_rule.clone()),
            AstStmt::ImportRule(import_rule) => self.visit_import_rule(import_rule.clone()),
            AstStmt::Use(use_rule) => {
                self.visit_use_rule(*use_rule.clone())?;
                Ok(None)
            }
            AstStmt::Forward(forward_rule) => {
                self.visit_forward_rule(*forward_rule.clone())?;
                Ok(None)
            }
            AstStmt::Supports(supports_rule) => {
                self.visit_supports_rule(*supports_rule.clone())?;
                Ok(None)
            }
        }
    }

    /// Reference-based variable declaration visitor.
    fn visit_variable_decl_ref(&mut self, decl: &AstVariableDecl<'static>) -> SassResult<Option<Value>> {
        let name = Spanned {
            node: decl.name,
            span: decl.span,
        };

        if decl.is_guarded {
            if decl.namespace.is_none() && self.env.at_root() {
                let var_override = (*self.configuration).borrow_mut().remove(decl.name);
                if !matches!(
                    var_override,
                    Some(ConfiguredValue {
                        value: Value::Null,
                        ..
                    }) | None
                ) {
                    self.env.insert_var(
                        name,
                        None,
                        var_override.unwrap().value,
                        true,
                        self.flags.in_semi_global_scope(),
                    )?;
                    return Ok(None);
                }
            }

            if self.env.var_exists(decl.name, decl.namespace, decl.span)? {
                let value = self.env.get_var(name, decl.namespace).unwrap();

                if value != Value::Null {
                    return Ok(None);
                }
            }
        }

        let value = self.visit_expr_ref(&decl.value)?;
        let value = self.without_slash(value);

        self.env.insert_var(
            name,
            decl.namespace,
            value,
            decl.is_global,
            self.flags.in_semi_global_scope(),
        )?;

        Ok(None)
    }

    /// Reference-based loud comment visitor.
    fn visit_loud_comment_ref(&mut self, comment: &AstLoudComment<'static>) -> SassResult<Option<Value>> {
        if self.flags.in_function() {
            return Ok(None);
        }

        let css_comment = CssStmt::Comment(
            self.perform_interpolation_ref(&comment.text, false)?,
            comment.span,
        );

        let at_root = self.parent.is_none() || self.parent == Some(CssTree::ROOT);
        if at_root && self.in_module_import_section {
            self.pending_import_items.push(css_comment);
        } else {
            self.add_child_to_current_parent(css_comment);
        }

        Ok(None)
    }

    /// Reference-based if-statement visitor.
    fn visit_if_stmt_ref(&mut self, if_stmt: &AstIf<'static>) -> SassResult<Option<Value>> {
        let mut matched_body: Option<&[AstStmt<'static>]> = None;
        for clause in &if_stmt.if_clauses {
            if self.visit_expr_ref(&clause.condition)?.is_truthy() {
                matched_body = Some(clause.body);
                break;
            }
        }

        if matched_body.is_none() {
            matched_body = if_stmt.else_clause;
        }

        self.env.scope_enter();

        let mut result = None;

        if let Some(stmts) = matched_body {
            for stmt in stmts {
                let val = self.visit_stmt_ref(stmt)?;
                if val.is_some() {
                    result = val;
                    break;
                }
            }
        }

        self.env.scope_exit();

        Ok(result)
    }

    /// Reference-based style rule visitor — the most common statement in loop bodies.
    fn visit_style_ref(&mut self, style: &AstStyle<'static>) -> SassResult<Option<Value>> {
        if !self.style_rule_exists()
            && !self.flags.in_unknown_at_rule()
            && !self.flags.in_keyframes()
        {
            return Err((
                "Declarations may only be used within style rules.",
                style.span,
            )
                .into());
        }

        let is_custom_property = style.is_custom_property();

        if is_custom_property && self.declaration_name.is_some() {
            return Err((
                "Declarations whose names begin with \"--\" may not be nested.",
                style.span,
            )
                .into());
        }

        let mut name = {
            let result = self.perform_interpolation_ref(&style.name, true)?;
            result
        };

        if let Some(declaration_name) = &self.declaration_name {
            name = format!("{}-{}", declaration_name, name);
        }

        if let Some(value) = style
            .value
            .as_ref()
            .map(|s| {
                SassResult::Ok(Spanned {
                    node: self.visit_expr_ref(&s.node)?,
                    span: s.span,
                })
            })
            .transpose()?
        {
            if !value.is_blank() || value.is_empty_list() || is_custom_property {
                self.add_child_to_current_parent(
                    CssStmt::Style(Style {
                        property: InternedString::get_or_intern(&name),
                        value: Box::new(value),
                        declared_as_custom_property: is_custom_property,
                        property_span: style.span,
                    }),
                );
            }
        }

        if !style.body.is_empty() {
            let old_declaration_name = self.declaration_name.take();
            self.declaration_name = Some(name);
            self.with_scope::<SassResult<()>, _>(false, true, |visitor| {
                for stmt in style.body {
                    let result = visitor.visit_stmt_ref(stmt)?;
                    debug_assert!(result.is_none());
                }
                Ok(())
            })?;
            self.declaration_name = old_declaration_name;
        }

        Ok(None)
    }

    // todo: we really don't have to return Option<Value> from all of these children
    pub(crate) fn visit_stmt(&mut self, stmt: &AstStmt<'static>) -> SassResult<Option<Value>> {
        self.visit_stmt_ref(stmt)
    }

    fn visit_forward_rule(&mut self, forward_rule: AstForwardRule<'static>) -> SassResult<()> {
        let old_config = Rc::clone(&self.configuration);
        let adjusted_config = Configuration::through_forward(Rc::clone(&old_config), &forward_rule);

        if !forward_rule.configuration.is_empty() {
            let new_configuration =
                self.add_forward_configuration(Rc::clone(&adjusted_config), &forward_rule)?;

            self.load_module(
                forward_rule.url.as_path(),
                Some(Rc::clone(&new_configuration)),
                false,
                forward_rule.span,
                |visitor, module, _| {
                    visitor.env.forward_module(Rc::clone(&module), forward_rule.clone())?;
                    visitor.upstream_modules.push(module);

                    Ok(())
                },
            )?;

            Self::remove_used_configuration(
                &adjusted_config,
                &new_configuration,
                &forward_rule
                    .configuration
                    .iter()
                    .filter(|var| !var.is_guarded)
                    .map(|var| var.name.node)
                    .collect(),
            );

            // Remove all the variables that weren't configured by this particular
            // `@forward` before checking that the configuration is empty. Errors for
            // outer `with` clauses will be thrown once those clauses finish
            // executing.
            let configured_variables: FxHashSet<Identifier> = forward_rule
                .configuration
                .iter()
                .map(|var| var.name.node)
                .collect();

            let mut to_remove = Vec::new();

            for name in (*new_configuration).borrow().values.keys() {
                if !configured_variables.contains(&name) {
                    to_remove.push(name);
                }
            }

            for name in to_remove {
                (*new_configuration).borrow_mut().remove(name);
            }

            Self::assert_configuration_is_empty(&new_configuration, false)?;
        } else {
            self.configuration = adjusted_config;
            let url = forward_rule.url.clone();
            self.load_module(
                url.as_path(),
                None,
                false,
                forward_rule.span,
                move |visitor, module, _| {
                    visitor.env.forward_module(Rc::clone(&module), forward_rule.clone())?;
                    visitor.upstream_modules.push(module);

                    Ok(())
                },
            )?;
            self.configuration = old_config;
        }

        Ok(())
    }

    #[allow(clippy::unnecessary_unwrap)]
    fn add_forward_configuration(
        &mut self,
        config: Rc<RefCell<Configuration>>,
        forward_rule: &AstForwardRule<'static>,
    ) -> SassResult<Rc<RefCell<Configuration>>> {
        let mut new_values = FxHashMap::from_iter((*config).borrow().values.iter());

        for variable in &forward_rule.configuration {
            if variable.is_guarded {
                let old_value = (*config).borrow_mut().remove(variable.name.node);

                if old_value.is_some()
                    && !matches!(
                        old_value,
                        Some(ConfiguredValue {
                            value: Value::Null,
                            ..
                        })
                    )
                {
                    new_values.insert(variable.name.node, old_value.unwrap());
                    continue;
                }
            }

            // todo: superfluous clone?
            let value = self.visit_expr(variable.expr.node.clone())?;
            let value = self.without_slash(value);

            new_values.insert(
                variable.name.node,
                ConfiguredValue::explicit(value, variable.expr.span),
            );
        }

        Ok(Rc::new(RefCell::new(
            if !(*config).borrow().is_implicit() || (*config).borrow().is_empty() {
                Configuration::explicit(new_values, forward_rule.span)
            } else {
                Configuration::implicit(new_values)
            },
        )))
    }

    /// Remove configured values from [upstream] that have been removed from
    /// [downstream], unless they match a name in [except].
    fn remove_used_configuration(
        upstream: &Rc<RefCell<Configuration>>,
        downstream: &Rc<RefCell<Configuration>>,
        except: &FxHashSet<Identifier>,
    ) {
        let mut names_to_remove = Vec::new();
        let downstream_keys = (*downstream).borrow().values.keys();
        for name in (*upstream).borrow().values.keys() {
            if except.contains(&name) {
                continue;
            }

            if !downstream_keys.contains(&name) {
                names_to_remove.push(name);
            }
        }

        for name in names_to_remove {
            (*upstream).borrow_mut().remove(name);
        }
    }

    fn parenthesize_supports_condition(
        &mut self,
        condition: AstSupportsCondition<'static>,
        operator: Option<&str>,
    ) -> SassResult<String> {
        match &condition {
            AstSupportsCondition::Negation(..) => {
                Ok(format!("({})", self.visit_supports_condition(condition)?))
            }
            AstSupportsCondition::Operation {
                operator: operator2,
                ..
            } if operator2.is_none() || operator2.as_deref() != operator => {
                Ok(format!("({})", self.visit_supports_condition(condition)?))
            }
            _ => self.visit_supports_condition(condition),
        }
    }

    fn visit_supports_condition(&mut self, condition: AstSupportsCondition<'static>) -> SassResult<String> {
        self.visit_supports_condition_ref(&condition)
    }

    fn visit_supports_condition_ref(&mut self, condition: &AstSupportsCondition<'static>) -> SassResult<String> {
        match condition {
            AstSupportsCondition::Operation {
                left,
                operator,
                right,
            } => Ok(format!(
                "{} {} {}",
                self.parenthesize_supports_condition((*left).clone(), operator.as_deref())?,
                operator.as_ref().unwrap(),
                self.parenthesize_supports_condition((*right).clone(), operator.as_deref())?
            )),
            AstSupportsCondition::Negation(inner) => Ok(format!(
                "not {}",
                self.parenthesize_supports_condition((*inner).clone(), None)?
            )),
            AstSupportsCondition::Interpolation(expr) => {
                self.evaluate_to_css(expr.clone(), QuoteKind::None, self.empty_span)
            }
            AstSupportsCondition::Declaration { name, value } => {
                let old_in_supports_decl = self.flags.in_supports_declaration();
                self.flags.set(ContextFlags::IN_SUPPORTS_DECLARATION, true);

                let is_custom_property = match name {
                    AstExpr::String(StringExpr(text, QuoteKind::None), ..) => {
                        text.initial_plain().starts_with("--")
                    }
                    _ => false,
                };

                let result = format!(
                    "({}:{}{})",
                    self.evaluate_to_css(name.clone(), QuoteKind::Quoted, self.empty_span)?,
                    if is_custom_property { "" } else { " " },
                    self.evaluate_to_css(value.clone(), QuoteKind::Quoted, self.empty_span)?,
                );

                self.flags
                    .set(ContextFlags::IN_SUPPORTS_DECLARATION, old_in_supports_decl);

                Ok(result)
            }
            AstSupportsCondition::Function { name, args } => Ok(format!(
                "{}({})",
                self.perform_interpolation(name.clone(), false)?,
                self.perform_interpolation(args.clone(), false)?
            )),
            AstSupportsCondition::Anything { contents } => Ok(format!(
                "({})",
                self.perform_interpolation(contents.clone(), false)?,
            )),
        }
    }

    fn visit_supports_rule(&mut self, supports_rule: AstSupportsRule<'static>) -> SassResult<()> {
        if self.declaration_name.is_some() {
            return Err((
                "Supports rules may not be used within nested declarations.",
                supports_rule.span,
            )
                .into());
        }

        let condition = self.visit_supports_condition(supports_rule.condition)?;

        let css_supports_rule = CssStmt::Supports(
            SupportsRule {
                params: condition,
                body: Vec::new(),
            },
            false,
        );

        let children = supports_rule.body;

        let nest_at_rule = self.is_plain_css && self.plain_css_style_rule_depth > 1;

        self.with_parent(
            css_supports_rule,
            true,
            |visitor| {
                if !visitor.style_rule_exists() || nest_at_rule {
                    for stmt in children {
                        let result = visitor.visit_stmt(stmt)?;
                        debug_assert!(result.is_none());
                    }
                } else {
                    // If we're in a style rule, copy it into the supports rule so that
                    // declarations immediately inside @supports have somewhere to go.
                    //
                    // For example, "a {@supports (a: b) {b: c}}" should produce "@supports
                    // (a: b) {a {b: c}}".
                    let selector = visitor.style_rule_ignoring_at_root.clone().unwrap();
                    let ruleset = CssStmt::RuleSet {
                        selector,
                        body: Vec::new(),
                        is_group_end: false,
                        source_span: None,
                    };

                    visitor.with_parent(
                        ruleset,
                        false,
                        |visitor| {
                            for stmt in children {
                                let result = visitor.visit_stmt(stmt)?;
                                debug_assert!(result.is_none());
                            }

                            Ok(())
                        },
                        |_| false,
                    )?;
                }

                Ok(())
            },
            if nest_at_rule {
                (|_: &CssStmt| false) as fn(&CssStmt) -> bool
            } else {
                CssStmt::is_style_rule as fn(&CssStmt) -> bool
            },
        )?;

        Ok(())
    }

    fn execute(
        &mut self,
        stylesheet: StyleSheet<'static>,
        configuration: Option<Rc<RefCell<Configuration>>>,
        names_in_errors: bool,
    ) -> SassResult<Rc<RefCell<Module>>> {
        let url = self.canonicalize(&stylesheet.url);

        if let Some(already_loaded) = self.modules.get(&url).cloned() {
            let current_configuration =
                configuration.unwrap_or_else(|| Rc::clone(&self.configuration));

            if !current_configuration.borrow().is_implicit() {
                // Check if this is the same configuration (Rc identity on original)
                let same_original = self
                    .module_configurations
                    .get(&url)
                    .and_then(|existing| existing.as_ref())
                    .map_or(false, |existing| {
                        let existing_orig = Configuration::original_config(Rc::clone(existing));
                        let current_orig =
                            Configuration::original_config(Rc::clone(&current_configuration));
                        Rc::ptr_eq(&existing_orig, &current_orig)
                    });

                if !same_original {
                    // Check if module has !default vars matching the config keys
                    let config_keys: FxHashSet<Identifier> =
                        current_configuration.borrow().values.keys().into_iter().collect();
                    let could_be_configured = stylesheet
                        .configurable_variables
                        .iter()
                        .any(|v| config_keys.contains(v));

                    if could_be_configured {
                        let msg = if names_in_errors {
                            format!(
                                "{} was already loaded, so it can't be configured using \"with\".",
                                url.to_string_lossy()
                            )
                        } else {
                            "This module was already loaded, so it can't be configured using \"with\"."
                                .to_owned()
                        };

                        return Err((
                            msg,
                            current_configuration.borrow().span.unwrap_or(self.empty_span),
                        )
                            .into());
                    }
                }
            }

            // Clone CSS for extend isolation in two cases:
            // 1. We're in an @import context loading a cached module
            // 2. We're in a @use context but the module was first loaded
            //    inside an @import (so the original CSS belongs to the @import)
            if self.in_import_context || self.modules_loaded_in_import.contains(&url) {
                let (cloned_module, has_clones) = self.clone_module_for_import(&url, &already_loaded);
                if has_clones {
                    return Ok(cloned_module);
                }
            }

            return Ok(already_loaded);
        }

        let mut env = Environment::new();

        // Pre-declare global variable slots for any `!global` declarations found
        // during parsing. This ensures the module exposes the same members
        // regardless of control flow, defaulting to `null` if never assigned.
        for name in &stylesheet.pre_declared_global_variables {
            env.scopes.insert_var(0, *name, Value::Null);
        }

        // Save the configuration Rc for tracking before it's moved into the closure.
        let config_for_tracking = configuration.as_ref().map(Rc::clone);

        // Create a fresh ExtensionStore for this module (per-module scoping).
        let mut module_extension_store = ExtensionStore::new(self.empty_span);
        let mut module_upstream: Vec<Rc<RefCell<Module>>> = Vec::new();

        self.with_environment::<SassResult<()>, _>(env.new_closure(), |visitor| {
            let old_parent = visitor.parent;
            let old_style_rule = visitor.style_rule_ignoring_at_root.take();
            let old_original_selector = visitor.original_selector.take();
            let old_media_queries = visitor.media_queries.take();
            let old_declaration_name = visitor.declaration_name.take();
            let old_in_unknown_at_rule = visitor.flags.in_unknown_at_rule();
            let old_at_root_excluding_style_rule = visitor.flags.at_root_excluding_style_rule();
            let old_in_keyframes = visitor.flags.in_keyframes();
            let old_configuration = if let Some(new_config) = configuration {
                Some(mem::replace(&mut visitor.configuration, new_config))
            } else {
                None
            };
            visitor.parent = None;
            visitor.flags.set(ContextFlags::IN_UNKNOWN_AT_RULE, false);
            visitor
                .flags
                .set(ContextFlags::AT_ROOT_EXCLUDING_STYLE_RULE, false);
            visitor.flags.set(ContextFlags::IN_KEYFRAMES, false);

            // Each module starts with a fresh import section.
            let old_pending_imports = mem::take(&mut visitor.pending_import_items);
            let old_in_module_import_section = visitor.in_module_import_section;
            visitor.in_module_import_section = true;
            visitor.module_depth += 1;

            // Swap in this module's ExtensionStore so all @extend rules and
            // selector registrations go into the module's own store.
            mem::swap(&mut visitor.extender, &mut module_extension_store);
            let old_upstream = mem::take(&mut visitor.upstream_modules);

            // Snapshot ROOT children count to track which CSS this module adds.
            let root_children_before = visitor.css_tree.child_count(CssTree::ROOT);

            visitor.visit_stylesheet(&stylesheet)?;

            // Flush any remaining pending imports from this module.
            visitor.flush_pending_imports(true);

            // Record this module's root-level CSS indices for potential cloning.
            let new_css_indices: Vec<CssTreeIdx> = visitor
                .css_tree
                .root_children_from(root_children_before)
                .into_iter()
                .filter(|idx| !visitor.css_tree.is_hidden(*idx))
                .collect();
            visitor.module_css_indices.insert(url.clone(), new_css_indices.clone());

            // When this module is being evaluated inside a nested @import
            // (i.e., `a { @import "file-that-uses-modules" }`), the module's
            // CSS was emitted at ROOT with parent=None. We need to resolve
            // module CSS selectors with the enclosing parent selector so that
            // they appear nested under the parent in the output.
            if visitor.in_import_context {
                if let Some(ref parent_selector) = old_style_rule {
                    let parent_list = parent_selector.as_selector_list().clone();
                    for idx in &new_css_indices {
                        let needs_resolution = {
                            let stmt = visitor.css_tree.get(*idx);
                            matches!(&*stmt, Some(CssStmt::RuleSet { .. }))
                        };
                        if needs_resolution {
                            let mut stmt = visitor.css_tree.get_mut(*idx);
                            if let Some(CssStmt::RuleSet {
                                ref mut selector,
                                ref mut is_group_end,
                                ..
                            }) = &mut *stmt
                            {
                                let old_list = selector.as_selector_list().clone();
                                let resolved = old_list.resolve_parent_selectors(
                                    Some(parent_list.clone()),
                                    true,
                                )?;
                                selector.set_inner(resolved);
                                // Clear group_end since these are conceptually
                                // children of the enclosing style rule, flattened
                                // to top level. Blank-line insertion should be
                                // controlled by the enclosing context, not the
                                // module's internal evaluation.
                                *is_group_end = false;
                            }
                        }
                    }
                }
            }

            // Swap back the parent's ExtensionStore and capture the module's.
            mem::swap(&mut visitor.extender, &mut module_extension_store);
            module_upstream = mem::replace(&mut visitor.upstream_modules, old_upstream);

            // Restore import section state for the parent module.
            visitor.module_depth -= 1;
            visitor.pending_import_items = old_pending_imports;
            visitor.in_module_import_section = old_in_module_import_section;

            visitor.parent = old_parent;
            visitor.style_rule_ignoring_at_root = old_style_rule;
            visitor.original_selector = old_original_selector;
            visitor.media_queries = old_media_queries;
            visitor.declaration_name = old_declaration_name;
            visitor
                .flags
                .set(ContextFlags::IN_UNKNOWN_AT_RULE, old_in_unknown_at_rule);
            visitor.flags.set(
                ContextFlags::AT_ROOT_EXCLUDING_STYLE_RULE,
                old_at_root_excluding_style_rule,
            );
            visitor
                .flags
                .set(ContextFlags::IN_KEYFRAMES, old_in_keyframes);
            if let Some(old_config) = old_configuration {
                visitor.configuration = old_config;
            }

            Ok(())
        })?;

        // Build module with its own extension store and upstream deps.
        let module = env.to_module_with_upstream(module_extension_store, module_upstream);

        self.module_ptr_to_url.insert(Rc::as_ptr(&module) as usize, url.clone());
        self.modules.insert(url.clone(), Rc::clone(&module));
        self.module_configurations
            .insert(url.clone(), config_for_tracking);

        // Track modules loaded in @import context so that later @use
        // references know to clone the CSS for extend isolation.
        if self.in_import_context {
            self.modules_loaded_in_import.insert(url);
        }

        Ok(module)
    }

    /// Evaluate a stylesheet for `meta.load-css()`, routing through `execute()`
    /// so modules are cached. Clones CSS from the loaded module's full transitive
    /// dependency tree (like dart-sass's `_combineCss(clone: true)`), applies
    /// extends to the cloned selectors, and emits the result.
    pub(crate) fn load_css_inner(
        &mut self,
        stylesheet: StyleSheet<'static>,
        configuration: Option<Rc<RefCell<Configuration>>>,
    ) -> SassResult<()> {
        let canonical_url = self.canonicalize(&stylesheet.url);
        let is_plain_css = stylesheet.is_plain_css;

        if self.active_modules.contains(&canonical_url) {
            return Err((
                "Module loop: this module is already being loaded.",
                self.empty_span,
            )
                .into());
        }

        self.active_modules.insert(canonical_url.clone());

        // Save parent context — execute() clears these, but we need them
        // to resolve parent selectors on the emitted CSS afterwards.
        let old_style_rule = self.style_rule_ignoring_at_root.clone();

        let root_children_before = self.css_tree.child_count(CssTree::ROOT);

        let module = self.execute(stylesheet, configuration.clone(), true)?;

        self.active_modules.remove(&canonical_url);

        // Ensure hidden templates exist for all modules in the transitive tree.
        // On first load, module_css_indices point to visible nodes at ROOT;
        // we create hidden copies so the originals are preserved for the root's
        // own output, and clones come from pristine templates.
        self.ensure_hidden_templates_for_module(&module);

        // On first load, execute() emitted CSS directly at ROOT. Hide that
        // output — we'll emit cloned CSS instead (so extends don't bleed back
        // to the original selectors via Rc<RefCell> sharing).
        let execute_children = self.css_tree.root_children_from(root_children_before);
        for idx in &execute_children {
            self.css_tree.hide(*idx);
        }

        // Collect CSS indices from the loaded module's FULL transitive dependency
        // tree (templates point to hidden copies after ensure_hidden_templates).
        let all_css_indices = self.collect_transitive_css_indices(&module);

        // Clone all transitive CSS into ROOT, creating new ExtendedSelectors.
        // For plain CSS files, `&` is a CSS nesting selector that must be
        // preserved literally (not resolved to the parent). We wrap such
        // subtrees in a RuleSet with the parent selector instead.
        let mut selector_map = FxHashMap::default();
        let mut wrapper_indices: FxHashSet<CssTreeIdx> = FxHashSet::default();
        for idx in &all_css_indices {
            let needs_wrapper = is_plain_css && old_style_rule.is_some() && {
                let stmt = self.css_tree.get(*idx);
                if let Some(CssStmt::RuleSet { ref selector, .. }) = &*stmt {
                    selector.as_selector_list().contains_parent_selector()
                } else {
                    false
                }
            };

            if needs_wrapper {
                let parent_list =
                    old_style_rule.as_ref().unwrap().as_selector_list().clone();
                let wrapper_selector = ExtendedSelector::new(parent_list);
                let wrapper = CssStmt::RuleSet {
                    selector: wrapper_selector,
                    body: Vec::new(),
                    is_group_end: false,
                    source_span: None,
                };
                let wrapper_idx = self.css_tree.add_child(wrapper, CssTree::ROOT);
                wrapper_indices.insert(wrapper_idx);
                self.css_tree
                    .clone_subtree(*idx, wrapper_idx, &mut selector_map);
            } else {
                self.css_tree
                    .clone_subtree(*idx, CssTree::ROOT, &mut selector_map);
            }
        }

        // Apply the loaded module's extensions to the CLONED selectors only.
        // This matches dart-sass's approach of cloning CSS before extending.
        Self::extend_cloned_selectors(&module, &selector_map)?;

        // Resolve cloned CSS selectors with the caller's parent selector.
        let cloned_start = root_children_before + execute_children.len();
        if let Some(ref parent_selector) = old_style_rule {
            let parent_list = parent_selector.as_selector_list().clone();
            let cloned_children = self.css_tree.root_children_from(cloned_start);
            for idx in &cloned_children {
                // Skip wrapper RuleSets created for plain CSS `&` nesting —
                // their selector is already the parent.
                if wrapper_indices.contains(idx) {
                    continue;
                }
                let needs_resolution = {
                    let stmt = self.css_tree.get(*idx);
                    matches!(&*stmt, Some(CssStmt::RuleSet { .. }))
                };
                if needs_resolution {
                    let mut stmt = self.css_tree.get_mut(*idx);
                    if let Some(CssStmt::RuleSet {
                        ref mut selector,
                        ref mut is_group_end,
                        ..
                    }) = &mut *stmt
                    {
                        let old_list = selector.as_selector_list().clone();
                        let resolved = old_list
                            .resolve_parent_selectors(Some(parent_list.clone()), true)?;
                        selector.set_inner(resolved);
                        *is_group_end = false;
                    }
                }
            }
        }

        // Register cloned CSS selectors in the caller's extension store,
        // so that @extend rules in the caller can target them.
        {
            let cloned_children = self.css_tree.root_children_from(cloned_start);
            for idx in &cloned_children {
                let stmt = self.css_tree.get(*idx);
                if let Some(CssStmt::RuleSet { ref selector, .. }) = &*stmt {
                    self.extender.register_existing_selector(selector)?;
                }
            }
        }

        if let Some(configuration) = configuration {
            Self::assert_configuration_is_empty(&configuration, true)?;
        }

        Ok(())
    }

    /// Collect deduplicated CSS tree indices from the full transitive dependency
    /// tree of a module, in upstream-first topological order.
    fn collect_transitive_css_indices(
        &self,
        module: &Rc<RefCell<Module>>,
    ) -> Vec<CssTreeIdx> {
        // Build reverse mapping: module pointer → URL for looking up CSS indices.
        let ptr_to_url: FxHashMap<*const RefCell<Module>, PathBuf> = self
            .modules
            .iter()
            .map(|(url, m)| (Rc::as_ptr(m), url.clone()))
            .collect();

        let mut sorted: Vec<Rc<RefCell<Module>>> = Vec::new();
        let mut seen: FxHashSet<*const RefCell<Module>> = FxHashSet::default();

        fn visit_module(
            module: &Rc<RefCell<Module>>,
            sorted: &mut Vec<Rc<RefCell<Module>>>,
            seen: &mut FxHashSet<*const RefCell<Module>>,
        ) {
            let ptr = Rc::as_ptr(module);
            if !seen.insert(ptr) {
                return;
            }
            let upstream: Vec<Rc<RefCell<Module>>> = {
                let m = module.borrow();
                if let Module::Environment { upstream, .. } = &*m {
                    upstream.clone()
                } else {
                    Vec::new()
                }
            };
            for up in &upstream {
                visit_module(up, sorted, seen);
            }
            sorted.push(Rc::clone(module));
        }

        visit_module(module, &mut sorted, &mut seen);

        // Collect CSS indices from each module, deduplicating.
        let mut all_indices = Vec::new();
        let mut seen_indices: FxHashSet<CssTreeIdx> = FxHashSet::default();

        for module_ref in &sorted {
            let ptr = Rc::as_ptr(module_ref);
            if let Some(url) = ptr_to_url.get(&ptr) {
                if let Some(indices) = self.module_css_indices.get(url) {
                    for idx in indices {
                        if seen_indices.insert(*idx) {
                            all_indices.push(*idx);
                        }
                    }
                }
            }
        }

        all_indices
    }

    /// Ensure all modules in the transitive dependency tree have hidden template
    /// copies of their CSS. On first load, module_css_indices points to the
    /// original CSS at ROOT. We create hidden copies so the originals are
    /// preserved for the root's output, and future clones come from templates.
    fn ensure_hidden_templates_for_module(&mut self, module: &Rc<RefCell<Module>>) {
        // Build reverse mapping: module pointer → URL
        let ptr_to_url: FxHashMap<*const RefCell<Module>, PathBuf> = self
            .modules
            .iter()
            .map(|(url, m)| (Rc::as_ptr(m), url.clone()))
            .collect();

        let mut sorted: Vec<Rc<RefCell<Module>>> = Vec::new();
        let mut seen: FxHashSet<*const RefCell<Module>> = FxHashSet::default();

        fn visit_module(
            module: &Rc<RefCell<Module>>,
            sorted: &mut Vec<Rc<RefCell<Module>>>,
            seen: &mut FxHashSet<*const RefCell<Module>>,
        ) {
            let ptr = Rc::as_ptr(module);
            if !seen.insert(ptr) {
                return;
            }
            let upstream: Vec<Rc<RefCell<Module>>> = {
                let m = module.borrow();
                if let Module::Environment { upstream, .. } = &*m {
                    upstream.clone()
                } else {
                    Vec::new()
                }
            };
            for up in &upstream {
                visit_module(up, sorted, seen);
            }
            sorted.push(Rc::clone(module));
        }

        visit_module(module, &mut sorted, &mut seen);

        // First pass: create ONE hidden copy per unique original index.
        let mut original_to_hidden: FxHashMap<CssTreeIdx, CssTreeIdx> = FxHashMap::default();
        let mut selector_map = FxHashMap::default();

        for module_ref in &sorted {
            let ptr = Rc::as_ptr(module_ref);
            if let Some(url) = ptr_to_url.get(&ptr) {
                if let Some(indices) = self.module_css_indices.get(url) {
                    for &idx in indices {
                        if !self.css_tree.is_hidden(idx)
                            && !original_to_hidden.contains_key(&idx)
                        {
                            let hidden_idx =
                                self.css_tree.clone_subtree_hidden(idx, &mut selector_map);
                            original_to_hidden.insert(idx, hidden_idx);
                        }
                    }
                }
            }
        }

        if original_to_hidden.is_empty() {
            return;
        }

        // Second pass: update module_css_indices to point to hidden copies.
        for module_ref in &sorted {
            let ptr = Rc::as_ptr(module_ref);
            if let Some(url) = ptr_to_url.get(&ptr) {
                if let Some(indices) = self.module_css_indices.get(url).cloned() {
                    let new_indices: Vec<CssTreeIdx> = indices
                        .iter()
                        .map(|idx| original_to_hidden.get(idx).copied().unwrap_or(*idx))
                        .collect();
                    if new_indices != indices {
                        self.module_css_indices.insert(url.clone(), new_indices);
                    }
                }
            }
        }
    }

    /// Apply extensions from a loaded module's dependency tree to cloned selectors.
    /// Operates only on the cloned ExtendedSelectors (via selector_map), leaving
    /// original module selectors untouched.
    fn extend_cloned_selectors(
        module: &Rc<RefCell<Module>>,
        selector_map: &FxHashMap<usize, ExtendedSelector>,
    ) -> SassResult<()> {
        // Get the loaded module's extensions.
        let extensions = {
            let m = module.borrow();
            match &*m {
                Module::Environment { extension_store, .. } => {
                    if extension_store.is_empty() {
                        return Ok(());
                    }
                    extension_store.clone()
                }
                _ => return Ok(()),
            }
        };

        // Create a temporary extension store with the module's extensions
        // and register cloned selectors in it. The registration process
        // will apply matching extensions via set_inner on the clones.
        let mut temp_store = extensions;
        for (_old_ptr, new_selector) in selector_map {
            temp_store.register_existing_selector(new_selector)?;
        }

        // Check for unsatisfied extends.
        temp_store.check_unsatisfied_extends()?;

        Ok(())
    }

    pub(crate) fn load_module(
        &mut self,
        url: &Path,
        configuration: Option<Rc<RefCell<Configuration>>>,
        names_in_errors: bool,
        span: Span,
        callback: impl Fn(&mut Self, Rc<RefCell<Module>>, StyleSheet<'static>) -> SassResult<()>,
    ) -> SassResult<()> {
        let builtin = match url.to_string_lossy().as_ref() {
            "sass:color" => Some(declare_module_color()),
            "sass:list" => Some(declare_module_list()),
            "sass:map" => Some(declare_module_map()),
            "sass:math" => Some(declare_module_math()),
            "sass:meta" => Some(declare_module_meta()),
            "sass:selector" => Some(declare_module_selector()),
            "sass:string" => Some(declare_module_string()),
            _ => None,
        };

        if let Some(builtin) = builtin {
            if let Some(ref configuration) = configuration {
                if !(**configuration).borrow().is_implicit() {
                    let msg = if names_in_errors {
                        format!(
                            "Built-in module {} can't be configured.",
                            url.to_string_lossy()
                        )
                    } else {
                        "Built-in modules can't be configured.".to_owned()
                    };

                    return Err((
                        msg,
                        (**configuration).borrow().span.unwrap(),
                    )
                        .into());
                }
            }

            callback(
                self,
                Rc::new(RefCell::new(builtin)),
                StyleSheet::new(false, url.to_path_buf()),
            )?;
            return Ok(());
        }

        // todo: decide on naming convention for style_sheet vs stylesheet
        let stylesheet = self.load_style_sheet(url.to_string_lossy().as_ref(), false, span)?;

        let canonical_url = self.canonicalize(&stylesheet.url);

        if self.active_modules.contains(&canonical_url) {
            return Err(("Module loop: this module is already being loaded.", span).into());
        }

        self.active_modules.insert(canonical_url.clone());

        // Flush pre-module comments into the combined import section before
        // loading the module, so comments before @use/@forward appear before
        // the module's own imports in the output.
        self.combined_import_section
            .append(&mut self.pending_import_items);

        let module = self.execute(stylesheet.clone(), configuration, names_in_errors)?;

        self.active_modules.remove(&canonical_url);

        callback(self, module, stylesheet)?;

        Ok(())
    }

    fn visit_use_rule(&mut self, use_rule: AstUseRule<'static>) -> SassResult<()> {
        let configuration = if use_rule.configuration.is_empty() {
            Rc::new(RefCell::new(Configuration::empty()))
        } else {
            let mut values = FxHashMap::default();

            for var in use_rule.configuration {
                let value = self.visit_expr(var.expr.node)?;
                let value = self.without_slash(value);
                values.insert(
                    var.name.node,
                    ConfiguredValue::explicit(value, var.name.span.merge(var.expr.span)),
                );
            }

            Rc::new(RefCell::new(Configuration::explicit(values, use_rule.span)))
        };

        let span = use_rule.span;

        let namespace = use_rule
            .namespace
            .as_ref()
            .map(|s| Identifier::from(s.trim_start_matches("sass:")));

        self.load_module(
            &use_rule.url,
            Some(Rc::clone(&configuration)),
            false,
            span,
            |visitor, module, _| {
                visitor.env.add_module(namespace, Rc::clone(&module), span)?;
                visitor.upstream_modules.push(module);

                Ok(())
            },
        )?;

        Self::assert_configuration_is_empty(&configuration, false)?;

        Ok(())
    }

    pub(crate) fn assert_configuration_is_empty(
        config: &Rc<RefCell<Configuration>>,
        name_in_error: bool,
    ) -> SassResult<()> {
        let config = (**config).borrow();
        // By definition, implicit configurations are allowed to only use a subset
        // of their values.
        if config.is_empty() || config.is_implicit() {
            return Ok(());
        }

        let Spanned { node: name, span } = config.first().unwrap();

        let msg = if name_in_error {
            format!(
                "${name} was not declared with !default in the @used module.",
                name = name
            )
        } else {
            "This variable was not declared with !default in the @used module.".to_owned()
        };

        Err((msg, span).into())
    }

    fn visit_import_rule(&mut self, import_rule: AstImportRule<'static>) -> SassResult<Option<Value>> {
        for import in import_rule.imports {
            match import {
                AstImport::Sass(dynamic_import) => {
                    self.visit_dynamic_import_rule(&dynamic_import)?;
                }
                AstImport::Plain(static_import) => self.visit_static_import_rule(static_import)?,
            }
        }

        Ok(None)
    }

    /// Searches the current directory of the file then searches in `load_paths` directories
    /// if the import has not yet been found.
    ///
    /// <https://sass-lang.com/documentation/at-rules/import#finding-the-file>
    /// <https://sass-lang.com/documentation/at-rules/import#load-paths>
    #[allow(clippy::cognitive_complexity, clippy::redundant_clone)]
    pub fn find_import(
        &mut self,
        path: &Path,
        for_import: bool,
        span: Span,
    ) -> SassResult<Option<PathBuf>> {
        // Cache key must include the import context (parent dir of current file)
        // because the same relative path resolves differently from different files
        let context_dir = self
            .current_import_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        let cache_key = (context_dir, path.to_path_buf(), for_import);
        if let Some(result) = self.import_path_cache.get(&cache_key) {
            return result.clone();
        }

        let result = self.find_import_uncached(path, for_import, span);
        self.import_path_cache.insert(cache_key, result.clone());
        result
    }

    /// Normalize a path by resolving `.` and `..` components without
    /// touching the filesystem (unlike `std::fs::canonicalize`).
    fn normalize_path(path: &Path) -> PathBuf {
        use std::path::Component;
        let mut result = PathBuf::new();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    if !result.pop() {
                        result.push(component);
                    }
                }
                Component::CurDir => {}
                _ => result.push(component),
            }
        }
        result
    }

    fn find_import_uncached(
        &self,
        path: &Path,
        for_import: bool,
        span: Span,
    ) -> SassResult<Option<PathBuf>> {
        let path_buf = if path.is_absolute() {
            Self::normalize_path(path)
        } else {
            Self::normalize_path(
                &self
                    .current_import_path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .join(path),
            )
        };

        let context_dir = self
            .current_import_path
            .parent()
            .unwrap_or_else(|| Path::new(""));

        // Build candidate list for a single path (original + partial with _ prefix)
        fn path_candidates(path: PathBuf) -> Vec<PathBuf> {
            let dirname = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
            let basename = path.file_name().unwrap_or_else(|| OsStr::new(".."));
            let partial = dirname.join(format!("_{}", basename.to_str().unwrap()));
            vec![path, partial]
        }

        // Build non-css candidates for conflict detection.
        // Order: partial first within each extension, sass before scss.
        // Returns (import_candidates, regular_candidates) — import candidates
        // take priority; conflicts are checked within each group separately.
        fn non_css_candidates_for_conflict(
            path: &Path,
            for_import: bool,
        ) -> (Vec<PathBuf>, Vec<PathBuf>) {
            let mut import_candidates = Vec::new();
            if for_import {
                let sass_import = path.with_extension("import.sass");
                let scss_import = path.with_extension("import.scss");
                let dirname = sass_import
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf();
                let sass_basename =
                    sass_import.file_name().unwrap_or_else(|| OsStr::new(".."));
                let scss_basename =
                    scss_import.file_name().unwrap_or_else(|| OsStr::new(".."));
                import_candidates.push(dirname.join(format!(
                    "_{}",
                    sass_basename.to_str().unwrap()
                )));
                import_candidates.push(sass_import);
                import_candidates.push(dirname.join(format!(
                    "_{}",
                    scss_basename.to_str().unwrap()
                )));
                import_candidates.push(scss_import);
            }

            let sass_path = path.with_extension("sass");
            let scss_path = path.with_extension("scss");
            let dirname = sass_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf();
            let sass_basename = sass_path.file_name().unwrap_or_else(|| OsStr::new(".."));
            let scss_basename = scss_path.file_name().unwrap_or_else(|| OsStr::new(".."));
            let mut regular_candidates = Vec::with_capacity(4);
            // Order: _other.sass, other.sass, _other.scss, other.scss
            regular_candidates
                .push(dirname.join(format!("_{}", sass_basename.to_str().unwrap())));
            regular_candidates.push(sass_path);
            regular_candidates
                .push(dirname.join(format!("_{}", scss_basename.to_str().unwrap())));
            regular_candidates.push(scss_path);

            (import_candidates, regular_candidates)
        }

        // Check for load conflicts among candidates in a directory.
        // Returns an error if multiple files match, otherwise returns
        // the first match (or None).
        let check_conflicts =
            |candidates: &[PathBuf], context_dir: &Path, span: Span| -> SassResult<Option<PathBuf>> {
                let existing: Vec<&PathBuf> = candidates
                    .iter()
                    .filter(|p| self.options.fs.is_file(p))
                    .collect();

                if existing.len() > 1 {
                    let mut msg = "It's not clear which file to import. Found:\n".to_string();
                    for p in &existing {
                        let rel = p
                            .strip_prefix(context_dir)
                            .unwrap_or(p);
                        msg.push_str(&format!("  {}\n", rel.display()));
                    }
                    // Remove trailing newline
                    msg.pop();
                    return Err((msg, span).into());
                }

                Ok(existing.into_iter().next().cloned())
            };

        // Resolve candidates with conflict detection: check import candidates first
        // (if for_import), then regular candidates. Import candidates take priority
        // and never conflict with regular candidates.
        let resolve_with_conflicts = |base_path: &Path,
                                       for_import: bool,
                                       context_dir: &Path,
                                       span: Span|
         -> SassResult<Option<PathBuf>> {
            let (import_candidates, regular_candidates) =
                non_css_candidates_for_conflict(base_path, for_import);

            // Check import candidates first (they take priority)
            if !import_candidates.is_empty() {
                if let Some(found) = check_conflicts(&import_candidates, context_dir, span)? {
                    return Ok(Some(found));
                }
            }

            // Then check regular candidates
            if let Some(found) = check_conflicts(&regular_candidates, context_dir, span)? {
                return Ok(Some(found));
            }

            // Fall back to CSS candidates
            let mut css_candidates = Vec::new();
            if for_import {
                css_candidates.extend(path_candidates(base_path.with_extension("import.css")));
            }
            css_candidates.extend(path_candidates(base_path.with_extension("css")));
            if let Some(found) = check_conflicts(&css_candidates, context_dir, span)? {
                return Ok(Some(found));
            }
            Ok(None)
        };

        if path_buf.extension() == Some(OsStr::new("scss"))
            || path_buf.extension() == Some(OsStr::new("sass"))
            || path_buf.extension() == Some(OsStr::new("css"))
        {
            let extension = path_buf.extension().unwrap();
            let mut candidates = Vec::new();
            if for_import {
                candidates.extend(path_candidates(
                    path_buf.with_extension(format!(".import{}", extension.to_str().unwrap())),
                ));
            }
            candidates.extend(path_candidates(path_buf));
            return Ok(self.options.fs.resolve_first_existing(&candidates));
        }

        // Check base path with conflict detection
        if let Some(found) = resolve_with_conflicts(&path_buf, for_import, context_dir, span)? {
            return Ok(Some(found));
        }

        // Also check index files
        if self.options.fs.is_dir(&path_buf) {
            if let Some(found) =
                resolve_with_conflicts(&path_buf.join("index"), for_import, context_dir, span)?
            {
                return Ok(Some(found));
            }
        }

        // Check load paths
        for load_path in &self.options.load_paths {
            let lp_buf = Self::normalize_path(&load_path.join(path));

            if let Some(found) =
                resolve_with_conflicts(&lp_buf, for_import, context_dir, span)?
            {
                return Ok(Some(found));
            }

            if self.options.fs.is_dir(&lp_buf) {
                if let Some(found) = resolve_with_conflicts(
                    &lp_buf.join("index"),
                    for_import,
                    context_dir,
                    span,
                )? {
                    return Ok(Some(found));
                }
            }
        }

        Ok(None)
    }

    fn parse_file(
        &mut self,
        lexer: Lexer,
        path: &Path,
        empty_span: Span,
    ) -> SassResult<StyleSheet<'static>> {
        let result = match InputSyntax::for_path(path) {
            InputSyntax::Scss => ScssParser::new(lexer, self.options, empty_span, path, self.arena).__parse(),
            InputSyntax::Sass => SassParser::new(lexer, self.options, empty_span, path, self.arena).__parse(),
            InputSyntax::Css => CssParser::new(lexer, self.options, empty_span, path, self.arena).__parse(),
        }?;
        // Safety: the arena lives for the entire compilation (stored in Visitor).
        Ok(unsafe { crate::ast::erase_stylesheet_lifetime(result) })
    }

    fn import_like_node(
        &mut self,
        url: &str,
        for_import: bool,
        span: Span,
    ) -> SassResult<StyleSheet<'static>> {
        if let Some(name) = self.find_import(url.as_ref(), for_import, span)? {
            let name = self.canonicalize(&name);
            if let Some(style_sheet) = self.import_cache.get(&name) {
                return Ok(style_sheet.clone());
            }

            let file = self.map.add_file(
                name.to_string_lossy().into(),
                String::from_utf8(self.options.fs.read(&name)?)?,
            );

            let old_is_use_allowed = self.flags.is_use_allowed();
            self.flags.set(ContextFlags::IS_USE_ALLOWED, true);

            let style_sheet =
                self.parse_file(Lexer::new_from_file(&file), &name, file.span.subspan(0, 0))?;

            self.flags
                .set(ContextFlags::IS_USE_ALLOWED, old_is_use_allowed);

            if self.files_seen.contains(&name) {
                self.import_cache.insert(name, style_sheet.clone());
            } else {
                self.files_seen.insert(name);
            }

            return Ok(style_sheet);
        }

        Err(("Can't find stylesheet to import.", span).into())
    }

    pub(crate) fn load_style_sheet(
        &mut self,
        url: &str,
        // default=false
        for_import: bool,
        span: Span,
    ) -> SassResult<StyleSheet<'static>> {
        // todo: import cache
        self.import_like_node(url, for_import, span)
    }

    fn visit_dynamic_import_rule(&mut self, dynamic_import: &AstSassImport) -> SassResult<()> {
        let stylesheet = self.load_style_sheet(&dynamic_import.url, true, dynamic_import.span)?;

        let url = stylesheet.url.clone();

        if self.active_modules.contains(&url) {
            return Err(("This file is already being loaded.", dynamic_import.span).into());
        }

        self.active_modules.insert(url.clone());

        // If the imported stylesheet doesn't use any modules, we can inject its
        // CSS directly into the current stylesheet. If it does use modules, we
        // need to put its CSS into an intermediate [ModifiableCssStylesheet] so
        // that we can hermetically resolve `@extend`s before injecting it.
        if stylesheet.uses.is_empty() && stylesheet.forwards.is_empty() {
            // Pre-declare global variable slots from the imported stylesheet.
            // Even if `!global` declarations are inside unreachable branches,
            // they create variable slots that default to `null`.
            for name in &stylesheet.pre_declared_global_variables {
                if !self.env.scopes.global_var_exists(*name) {
                    self.env.scopes.insert_var(0, *name, Value::Null);
                }
            }
            self.visit_stylesheet(&stylesheet)?;
            return Ok(());
        }

        let env = self.env.for_import();

        self.with_environment::<SassResult<()>, _>(env.clone(), |visitor| {
            let old_configuration = Rc::clone(&visitor.configuration);

            // This configuration is only used if it passes through a `@forward`
            // rule, so we avoid creating unnecessary ones for performance reasons.
            if !stylesheet.forwards.is_empty() {
                visitor.configuration = Rc::new(RefCell::new(env.to_implicit_configuration()));
            }

            // Mark import context so that cached modules clone their CSS
            // instead of sharing it (needed for @extend isolation).
            let old_in_import = visitor.in_import_context;
            visitor.in_import_context = true;

            // Clear shared clone state so all modules within this @import
            // share the same selector_map and cloned_modules (deduplicating
            // diamond dependencies).
            let old_selector_map = mem::take(&mut visitor.import_selector_map);
            let old_cloned_modules = mem::take(&mut visitor.import_cloned_modules);
            let old_cloned_css = mem::take(&mut visitor.import_cloned_css);

            visitor.visit_stylesheet(&stylesheet)?;

            visitor.import_selector_map = old_selector_map;
            visitor.import_cloned_modules = old_cloned_modules;
            visitor.import_cloned_css = old_cloned_css;
            visitor.in_import_context = old_in_import;
            visitor.configuration = old_configuration;

            Ok(())
        })?;

        // Create a dummy module with empty CSS and no extensions to make forwarded
        // members available in the current import context and to combine all the
        // CSS from modules used by [stylesheet].
        let module = env.to_dummy_module(self.empty_span);
        self.env.import_forwards(module);

        self.active_modules.remove(&url);

        Ok(())
    }

    fn visit_static_import_rule(&mut self, static_import: AstPlainCssImport<'static>) -> SassResult<()> {
        let import = self.interpolation_to_value(static_import.url, false, false)?;

        let modifiers = static_import
            .modifiers
            .map(|modifiers| self.interpolation_to_value(modifiers, false, false))
            .transpose()?;

        let node = CssStmt::Import(import, modifiers);

        if self.parent.is_some() && self.parent != Some(CssTree::ROOT) {
            self.css_tree.add_stmt(node, self.parent);
        } else if self.in_module_import_section {
            self.pending_import_items.push(node);
        } else {
            // Out-of-order import after the import section ended
            self.has_out_of_order_imports = true;
            self.combined_import_section.push(node);
        }

        Ok(())
    }

    fn visit_debug_rule(&mut self, debug_rule: AstDebugRule<'static>) -> SassResult<Option<Value>> {
        if self.options.quiet {
            return Ok(None);
        }

        let message = self.visit_expr(debug_rule.value)?;
        let message = message.inspect(debug_rule.span)?;

        let loc = self.map.look_up_span(debug_rule.span);
        self.options.logger.debug(loc, message.as_str());

        Ok(None)
    }

    fn visit_content_rule(&mut self, content_rule: AstContentRule<'static>) -> SassResult<Option<Value>> {
        let span = content_rule.args.span;
        if let Some(content) = &self.env.content {
            #[allow(mutable_borrow_reservation_conflict)]
            self.run_user_defined_callable(
                MaybeEvaledArguments::Invocation(content_rule.args),
                Rc::clone(content),
                &content.env.clone(),
                span,
                |content, visitor| {
                    let old_in_mixin = visitor.flags.in_mixin();
                    visitor.flags.set(ContextFlags::IN_MIXIN, false);
                    for stmt in content.content.body.iter() {
                        let result = visitor.visit_stmt_ref(stmt)?;
                        debug_assert!(result.is_none());
                    }
                    visitor.flags.set(ContextFlags::IN_MIXIN, old_in_mixin);

                    Ok(())
                },
            )?;
        }

        Ok(None)
    }

    fn trim_included(&self, nodes: &[CssTreeIdx]) -> CssTreeIdx {
        if nodes.is_empty() {
            return CssTree::ROOT;
        }

        let mut parent = self.parent;

        let mut innermost_contiguous: Option<usize> = None;

        for i in 0..nodes.len() {
            while parent != nodes.get(i).copied() {
                innermost_contiguous = None;

                let grandparent = self.css_tree.child_to_parent.get(&parent.unwrap()).copied();
                if grandparent.is_none() {
                    unreachable!(
                        "Expected {:?} to be an ancestor of {:?}.",
                        nodes[i], grandparent
                    )
                }
                parent = grandparent;
            }
            innermost_contiguous = innermost_contiguous.or(Some(i));

            let grandparent = self.css_tree.child_to_parent.get(&parent.unwrap()).copied();
            if grandparent.is_none() {
                unreachable!(
                    "Expected {:?} to be an ancestor of {:?}.",
                    nodes[i], grandparent
                )
            }
            parent = grandparent;
        }

        if parent != Some(CssTree::ROOT) {
            return CssTree::ROOT;
        }

        nodes[innermost_contiguous.unwrap()]
    }

    fn visit_at_root_rule(&mut self, mut at_root_rule: AstAtRootRule<'static>) -> SassResult<Option<Value>> {
        let query = match at_root_rule.query.clone() {
            Some(query) => {
                let resolved = self.perform_interpolation(query.node, true)?;

                let span = query.span;

                let query_toks = Lexer::new_from_string(&resolved, span);

                AtRootQueryParser::new(query_toks).parse()?
            }
            None => AtRootQuery::default(),
        };

        let mut current_parent_idx = self.parent;

        let mut included = Vec::new();

        while let Some(parent_idx) = current_parent_idx {
            let parent = self.css_tree.get(parent_idx);
            let grandparent_idx = match &*parent {
                Some(parent) => {
                    if !query.excludes(parent) {
                        included.push(parent_idx);
                    }
                    self.css_tree.child_to_parent.get(&parent_idx).copied()
                }
                None => break,
            };

            current_parent_idx = grandparent_idx;
        }

        let root = self.trim_included(&included);

        // If we didn't exclude any rules, we don't need to use the copies we might
        // have created.
        if Some(root) == self.parent {
            self.with_scope::<SassResult<()>, _>(false, true, |visitor| {
                for stmt in at_root_rule.body {
                    let result = visitor.visit_stmt(stmt)?;
                    debug_assert!(result.is_none());
                }

                Ok(())
            })?;
            return Ok(None);
        }

        let inner_copy = if !included.is_empty() {
            let inner_copy = self
                .css_tree
                .get(*included.first().unwrap())
                .as_ref()
                .map(CssStmt::copy_without_children);
            let mut outer_copy = self.css_tree.add_stmt(inner_copy.unwrap(), None);

            for node in &included[1..] {
                let copy = self
                    .css_tree
                    .get(*node)
                    .as_ref()
                    .map(CssStmt::copy_without_children)
                    .unwrap();

                let copy_idx = self.css_tree.add_stmt(copy, None);
                self.css_tree.link_child_to_parent(outer_copy, copy_idx);

                outer_copy = copy_idx;
            }

            Some(outer_copy)
        } else {
            let inner_copy = self
                .css_tree
                .get(root)
                .as_ref()
                .map(CssStmt::copy_without_children);
            inner_copy.map(|p| self.css_tree.add_stmt(p, None))
        };

        let body = mem::take(&mut at_root_rule.body);

        self.with_scope_for_at_root::<SassResult<()>, _>(inner_copy, &query, |visitor| {
            for stmt in body {
                let result = visitor.visit_stmt(stmt)?;
                debug_assert!(result.is_none());
            }

            Ok(())
        })?;

        // Hide ancestors that became empty after @at-root moved their children.
        // Two cases: (1) nodes like rulesets/media/supports that are naturally
        // invisible when empty, and (2) nodes that were copied by @at-root
        // (in the `included` list) and are now redundant empty shells.
        {
            let mut cleanup_idx = self.parent;
            while let Some(idx) = cleanup_idx {
                if idx == CssTree::ROOT {
                    break;
                }
                let should_hide = {
                    let stmt = self.css_tree.get(idx);
                    match &*stmt {
                        Some(s) => {
                            if !self.css_tree.is_stmt_visible(idx, s) {
                                // Naturally invisible (empty ruleset, media, supports)
                                true
                            } else if included.contains(&idx)
                                && !self.css_tree.has_visible_child(idx)
                            {
                                // Was copied by @at-root and is now empty
                                true
                            } else {
                                false
                            }
                        }
                        None => false,
                    }
                };
                if should_hide {
                    self.css_tree.hide(idx);
                    cleanup_idx = self.css_tree.child_to_parent.get(&idx).copied();
                } else {
                    break;
                }
            }
        }

        Ok(None)
    }

    fn with_scope_for_at_root<T, F: FnOnce(&mut Self) -> T>(
        &mut self,
        new_parent_idx: Option<CssTreeIdx>,
        query: &AtRootQuery,
        callback: F,
    ) -> T {
        let old_parent = self.parent;
        self.parent = new_parent_idx;

        let old_at_root_excluding_style_rule = self.flags.at_root_excluding_style_rule();

        if query.excludes_style_rules() {
            self.flags
                .set(ContextFlags::AT_ROOT_EXCLUDING_STYLE_RULE, true);
        }

        let old_media_query_info = if self.media_queries.is_some() && query.excludes_name("media") {
            Some((self.media_queries.take(), self.media_query_sources.take()))
        } else {
            None
        };

        let was_in_keyframes = if self.flags.in_keyframes() && query.excludes_name("keyframes") {
            let was = self.flags.in_keyframes();
            self.flags.set(ContextFlags::IN_KEYFRAMES, false);
            was
        } else {
            self.flags.in_keyframes()
        };

        // todo:
        // if self.flags.in_unknown_at_rule() && !included.iter().any(|parent| parent is CssAtRule)

        let res = self.with_scope(false, true, callback);

        self.parent = old_parent;

        self.flags.set(
            ContextFlags::AT_ROOT_EXCLUDING_STYLE_RULE,
            old_at_root_excluding_style_rule,
        );

        if let Some((old_media_queries, old_media_query_sources)) = old_media_query_info {
            self.media_queries = old_media_queries;
            self.media_query_sources = old_media_query_sources;
        }

        self.flags.set(ContextFlags::IN_KEYFRAMES, was_in_keyframes);

        res
    }

    fn visit_function_decl(&mut self, fn_decl: AstFunctionDecl<'static>) {
        let name = fn_decl.name.node;
        // todo: independency

        let func = SassFunction::UserDefined(UserDefinedFunction {
            function: Rc::new(fn_decl),
            name,
            env: self.env.new_closure(),
        });

        self.env.insert_fn(func);
    }

    pub(crate) fn parse_selector_from_string(
        &mut self,
        selector_text: &str,
        allows_parent: bool,
        allows_placeholder: bool,
        span: Span,
    ) -> SassResult<SelectorList> {
        let sel_toks = Lexer::new_from_string(selector_text, span);

        let mut parser = SelectorParser::new(sel_toks, allows_parent, allows_placeholder, span);
        parser.plain_css = self.is_plain_css;
        parser.parse()
    }

    fn visit_extend_rule(&mut self, extend_rule: AstExtendRule<'static>) -> SassResult<Option<Value>> {
        if !self.style_rule_exists() || self.declaration_name.is_some() {
            return Err((
                "@extend may only be used within style rules.",
                extend_rule.span,
            )
                .into());
        }

        let super_selector = self.style_rule_ignoring_at_root.clone().unwrap();

        let target_text = self.interpolation_to_value(extend_rule.value, false, true)?;

        let list = self.parse_selector_from_string(&target_text, false, true, extend_rule.span)?;

        for complex in list.components {
            if complex.components.len() != 1 || !complex.components.first().unwrap().is_compound() {
                // If the selector was a compound selector but not a simple
                // selector, emit a more explicit error.
                return Err(("complex selectors may not be extended.", extend_rule.span).into());
            }

            let compound = match complex.components.first() {
                Some(ComplexSelectorComponent::Compound(c)) => c,
                Some(..) | None => unreachable!("checked by above condition"),
            };
            if compound.components.len() != 1 {
                return Err((
                    format!(
                        "compound selectors may no longer be extended.\nConsider `@extend {}` instead.\nSee http://bit.ly/ExtendCompound for details.\n",
                        compound.components.iter().map(ToString::to_string).collect::<Vec<String>>().join(", ")
                    )
                , extend_rule.span).into());
            }

            self.extender.add_extension(
                super_selector.clone().into_selector().0,
                compound.components.first().unwrap(),
                &ExtendRule {
                    is_optional: extend_rule.is_optional,
                },
                &self.media_queries,
                extend_rule.span,
            )?;
        }

        Ok(None)
    }

    fn visit_error_rule(&mut self, error_rule: AstErrorRule<'static>) -> SassResult<Box<SassError>> {
        let value = self
            .visit_expr(error_rule.value)?
            .inspect(error_rule.span)?;

        Ok((value, error_rule.span).into())
    }

    fn merge_media_queries(
        queries1: &[MediaQuery],
        queries2: &[MediaQuery],
    ) -> Option<Vec<MediaQuery>> {
        let mut queries = Vec::with_capacity(queries1.len() * queries2.len());

        for query1 in queries1 {
            for query2 in queries2 {
                match query1.merge(query2) {
                    MediaQueryMergeResult::Empty => continue,
                    MediaQueryMergeResult::Unrepresentable => return None,
                    MediaQueryMergeResult::Success(result) => queries.push(result),
                }
            }
        }

        Some(queries)
    }

    fn visit_media_queries(
        &mut self,
        queries: Interpolation<'static>,
        span: Span,
    ) -> SassResult<Vec<CssMediaQuery>> {
        let resolved = self.perform_interpolation(queries, true)?;

        CssMediaQuery::parse_list(&resolved, span)
    }

    fn visit_media_rule(&mut self, media_rule: AstMedia<'static>) -> SassResult<Option<Value>> {
        if self.declaration_name.is_some() {
            return Err((
                "Media rules may not be used within nested declarations.",
                media_rule.span,
            )
                .into());
        }

        let queries1 = self.visit_media_queries(media_rule.query, media_rule.query_span)?;

        let nest_at_rule = self.is_plain_css && self.plain_css_style_rule_depth > 1;

        // In nested CSS, don't merge media queries — they stay as written
        let (merged_queries, merged_sources) = if nest_at_rule {
            (None, FxIndexSet::default())
        } else {
            // todo: superfluous clone?
            let queries2 = self.media_queries.clone();
            let merged = queries2
                .as_ref()
                .and_then(|queries2| Self::merge_media_queries(queries2, &queries1));

            let sources = match &merged {
                Some(merged_queries) if merged_queries.is_empty() => return Ok(None),
                Some(..) => {
                    let mut set = FxIndexSet::default();
                    set.extend(self.media_query_sources.clone().unwrap());
                    set.extend(self.media_queries.clone().unwrap());
                    set.extend(queries1.clone());
                    set
                }
                None => FxIndexSet::default(),
            };

            (merged, sources)
        };

        let children = media_rule.body;

        let query = merged_queries.clone().unwrap_or_else(|| queries1.clone());

        let media_rule = CssStmt::Media(
            MediaRule {
                query,
                body: Vec::new(),
                query_span: Some(media_rule.query_span),
            },
            false,
        );

        self.with_parent(
            media_rule,
            true,
            |visitor| {
                visitor.with_media_queries(
                    Some(merged_queries.unwrap_or(queries1)),
                    Some(merged_sources.clone()),
                    |visitor| {
                        if !visitor.style_rule_exists() || nest_at_rule {
                            for stmt in children {
                                let result = visitor.visit_stmt(stmt)?;
                                debug_assert!(result.is_none());
                            }
                        } else {
                            // If we're in a style rule, copy it into the media query so that
                            // declarations immediately inside @media have somewhere to go.
                            //
                            // For example, "a {@media screen {b: c}}" should produce
                            // "@media screen {a {b: c}}".
                            let selector = visitor.style_rule_ignoring_at_root.clone().unwrap();
                            let ruleset = CssStmt::RuleSet {
                                selector,
                                body: Vec::new(),
                                is_group_end: false,
                                source_span: None,
                            };

                            visitor.with_parent(
                                ruleset,
                                false,
                                |visitor| {
                                    for stmt in children {
                                        let result = visitor.visit_stmt(stmt)?;
                                        debug_assert!(result.is_none());
                                    }

                                    Ok(())
                                },
                                |_| false,
                            )?;
                        }

                        Ok(())
                    },
                )
            },
            {
                let merged_sources = merged_sources.clone();
                move |stmt: &CssStmt| match stmt {
                    CssStmt::RuleSet { .. } => !nest_at_rule,
                    // todo: node.queries.every(mergedSources.contains))
                    CssStmt::Media(media_rule, ..) => {
                        !merged_sources.is_empty()
                            && media_rule
                                .query
                                .iter()
                                .all(|query| merged_sources.contains(query))
                    }
                    _ => false,
                }
            },
        )?;

        Ok(None)
    }

    fn visit_unknown_at_rule(
        &mut self,
        unknown_at_rule: AstUnknownAtRule<'static>,
    ) -> SassResult<Option<Value>> {
        if self.declaration_name.is_some() {
            return Err((
                "At-rules may not be used within nested declarations.",
                unknown_at_rule.span,
            )
                .into());
        }

        let name = self.interpolation_to_value(unknown_at_rule.name, false, false)?;

        let value = unknown_at_rule
            .value
            .map(|v| self.interpolation_to_value(v, true, true))
            .transpose()?;

        if unknown_at_rule.body.is_none() {
            let stmt = CssStmt::UnknownAtRule(
                UnknownAtRule {
                    name,
                    params: value.unwrap_or_default(),
                    body: Vec::new(),
                    has_body: false,
                },
                false,
            );

            self.add_child_to_current_parent(stmt);

            return Ok(None);
        }

        let was_in_keyframes = self.flags.in_keyframes();
        let was_in_unknown_at_rule = self.flags.in_unknown_at_rule();

        let is_font_face = unvendor(&name) == "font-face";

        if unvendor(&name) == "keyframes" {
            self.flags.set(ContextFlags::IN_KEYFRAMES, true);
        } else {
            self.flags.set(ContextFlags::IN_UNKNOWN_AT_RULE, true);
        }

        let children = unknown_at_rule.body.unwrap();

        let stmt = CssStmt::UnknownAtRule(
            UnknownAtRule {
                name,
                params: value.unwrap_or_default(),
                body: Vec::new(),
                has_body: true,
            },
            false,
        );

        let nest_at_rule = self.is_plain_css && self.plain_css_style_rule_depth > 1;

        self.with_parent(
            stmt,
            true,
            |visitor| {
                if children.is_empty()
                    || !visitor.style_rule_exists()
                    || visitor.flags.in_keyframes()
                    || nest_at_rule
                    || is_font_face
                {
                    for stmt in children {
                        let result = visitor.visit_stmt(stmt)?;
                        debug_assert!(result.is_none());
                    }
                } else {
                    // If we're in a style rule, copy it into the at-rule so that
                    // declarations immediately inside it have somewhere to go.
                    //
                    // For example, "a {@foo {b: c}}" should produce "@foo {a {b: c}}".
                    let selector = visitor.style_rule_ignoring_at_root.clone().unwrap();

                    let style_rule = CssStmt::RuleSet {
                        selector,
                        body: Vec::new(),
                        is_group_end: false,
                        source_span: None,
                    };

                    visitor.with_parent(
                        style_rule,
                        false,
                        |visitor| {
                            for stmt in children {
                                let result = visitor.visit_stmt(stmt)?;
                                debug_assert!(result.is_none());
                            }

                            Ok(())
                        },
                        |_| false,
                    )?;
                }

                Ok(())
            },
            if nest_at_rule {
                (|_: &CssStmt| false) as fn(&CssStmt) -> bool
            } else {
                CssStmt::is_style_rule as fn(&CssStmt) -> bool
            },
        )?;

        self.flags.set(ContextFlags::IN_KEYFRAMES, was_in_keyframes);
        self.flags
            .set(ContextFlags::IN_UNKNOWN_AT_RULE, was_in_unknown_at_rule);

        Ok(None)
    }

    pub(crate) fn emit_warning(&mut self, message: &str, span: Span) {
        if self.options.quiet {
            return;
        }
        let loc = self.map.look_up_span(span);
        self.options.logger.warn(loc, message);
    }

    fn visit_warn_rule(&mut self, warn_rule: AstWarn<'static>) -> SassResult<()> {
        if self.warnings_emitted.insert(warn_rule.span) {
            let value = self.visit_expr(warn_rule.value)?;
            let message = value.to_css_string(warn_rule.span, self.options.is_compressed())?;
            self.emit_warning(&message, warn_rule.span);
        }

        Ok(())
    }

    fn with_media_queries<T>(
        &mut self,
        queries: Option<Vec<MediaQuery>>,
        sources: Option<FxIndexSet<MediaQuery>>,
        callback: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let old_media_queries = self.media_queries.take();
        let old_media_query_sources = self.media_query_sources.take();
        self.media_queries = queries;
        self.media_query_sources = sources;
        let result = callback(self);
        self.media_queries = old_media_queries;
        self.media_query_sources = old_media_query_sources;
        result
    }

    fn with_environment<T, F: FnOnce(&mut Self) -> T>(
        &mut self,
        env: Environment,
        callback: F,
    ) -> T {
        let mut old_env = env;
        mem::swap(&mut self.env, &mut old_env);
        let val = callback(self);
        mem::swap(&mut self.env, &mut old_env);
        val
    }

    fn add_child<F: Fn(&CssStmt) -> bool>(
        &mut self,
        node: CssStmt,
        through: Option<F>,
    ) -> CssTreeIdx {
        if self.parent.is_none() || self.parent == Some(CssTree::ROOT) {
            // End the import section when a non-comment, non-import hits ROOT.
            if self.in_module_import_section
                && !matches!(node, CssStmt::Comment(..) | CssStmt::Import(..))
            {
                self.flush_pending_imports(false);
                self.in_module_import_section = false;
            }
            return self.css_tree.add_stmt(node, self.parent);
        }

        let mut parent = self.parent.unwrap();

        if let Some(through) = through {
            while parent != CssTree::ROOT && through(self.css_tree.get(parent).as_ref().unwrap()) {
                let grandparent = self.css_tree.child_to_parent.get(&parent).copied();
                debug_assert!(
                    grandparent.is_some(),
                    "through() must return false for at least one parent of $node."
                );
                parent = grandparent.unwrap();
            }

            // If the parent has a (visible) following sibling, we shouldn't add to
            // the parent. Instead, we should create a copy and add it after the
            // interstitial sibling.
            if self.css_tree.has_following_sibling(parent) {
                let grandparent = self.css_tree.child_to_parent.get(&parent).copied().unwrap();

                // Check if the last child of the grandparent already has matching
                // media queries — if so, reuse it instead of creating a new copy.
                // This merges siblings like `h` and `k` into the same `@media`
                // block after bubbling (dart-sass#777).
                if let Some(existing) =
                    self.css_tree.last_matching_media_sibling(parent, grandparent)
                {
                    parent = existing;
                } else {
                    let parent_node = self
                        .css_tree
                        .get(parent)
                        .as_ref()
                        .map(CssStmt::copy_without_children)
                        .unwrap();
                    parent = self.css_tree.add_child(parent_node, grandparent);
                }
            }
        }

        self.css_tree.add_child(node, parent)
    }

    /// Add a leaf node (Style, Comment, bodyless at-rule) to the current parent,
    /// creating a copy of the parent if a following sibling exists (interleaved
    /// declarations).
    fn add_child_to_current_parent(&mut self, node: CssStmt) -> CssTreeIdx {
        let parent = self.parent.unwrap_or(CssTree::ROOT);

        // A non-comment, non-import statement at ROOT ends the import section.
        if parent == CssTree::ROOT && self.in_module_import_section {
            if !matches!(node, CssStmt::Comment(..) | CssStmt::Import(..)) {
                self.flush_pending_imports(false);
                self.in_module_import_section = false;
            }
        }

        // Only check interleaving inside style rules
        if self.style_rule_exists() && parent != CssTree::ROOT {
            if self.css_tree.has_following_sibling(parent) {
                let grandparent = self.css_tree.child_to_parent.get(&parent).copied().unwrap();
                let parent_copy = self
                    .css_tree
                    .get(parent)
                    .as_ref()
                    .map(CssStmt::copy_without_children)
                    .unwrap();
                let new_parent = self.css_tree.add_child(parent_copy, grandparent);
                self.parent = Some(new_parent);
                return self.css_tree.add_child(node, new_parent);
            }
        }

        self.css_tree.add_stmt(node, self.parent)
    }

    fn with_parent<F: FnOnce(&mut Self) -> SassResult<()>, FT: Fn(&CssStmt) -> bool>(
        &mut self,
        parent: CssStmt,
        // default=true
        scope_when: bool,
        callback: F,
        // todo: optional
        through: FT,
    ) -> SassResult<()> {
        let parent_idx = self.add_child(parent, Some(through));
        let old_parent = self.parent;
        self.parent = Some(parent_idx);
        let result = self.with_scope(false, scope_when, callback);
        self.parent = old_parent;
        result
    }

    fn with_scope<T, F: FnOnce(&mut Self) -> T>(
        &mut self,
        // default=false
        semi_global: bool,
        // default=true
        when: bool,
        callback: F,
    ) -> T {
        let semi_global = semi_global && self.flags.in_semi_global_scope();
        let was_in_semi_global_scope = self.flags.in_semi_global_scope();
        self.flags
            .set(ContextFlags::IN_SEMI_GLOBAL_SCOPE, semi_global);

        if !when {
            let v = callback(self);
            self.flags
                .set(ContextFlags::IN_SEMI_GLOBAL_SCOPE, was_in_semi_global_scope);

            return v;
        }

        self.env.scope_enter();

        let v = callback(self);

        self.flags
            .set(ContextFlags::IN_SEMI_GLOBAL_SCOPE, was_in_semi_global_scope);
        self.env.scope_exit();

        v
    }

    pub(crate) fn with_content<T>(
        &mut self,
        content: Option<Rc<CallableContentBlock>>,
        callback: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let old_content = self.env.content.take();
        self.env.content = content;
        let v = callback(self);
        self.env.content = old_content;
        v
    }

    fn visit_include_stmt(&mut self, include_stmt: AstInclude<'static>) -> SassResult<Option<Value>> {
        let mixin = self
            .env
            .get_mixin(include_stmt.name, include_stmt.namespace)?;

        match mixin {
            Mixin::Builtin(mixin) => {
                if include_stmt.content.is_some() {
                    return Err((
                        "Mixin doesn't accept a content block.",
                        include_stmt.span,
                    )
                        .into());
                }

                let args = self.eval_args(include_stmt.args, include_stmt.name.span)?;
                mixin(args, self)?;

                Ok(None)
            }
            Mixin::BuiltinWithContent(mixin) => {
                let args = self.eval_args(include_stmt.args, include_stmt.name.span)?;

                if let Some(content) = include_stmt.content {
                    let callable_content = Rc::new(CallableContentBlock {
                        content,
                        env: self.env.new_closure(),
                    });
                    self.with_content(Some(callable_content), |visitor| {
                        mixin(args, visitor)
                    })?;
                } else {
                    mixin(args, self)?;
                }

                Ok(None)
            }
            Mixin::UserDefined(mixin, env, defining_path) => {
                if include_stmt.content.is_some() && !mixin.has_content {
                    return Err(("Mixin doesn't accept a content block.", include_stmt.span).into());
                }

                let AstInclude { args, content, .. } = include_stmt;

                let old_in_mixin = self.flags.in_mixin();
                self.flags.set(ContextFlags::IN_MIXIN, true);

                let callable_content = content.map(|c| {
                    Rc::new(CallableContentBlock {
                        content: c,
                        env: self.env.new_closure(),
                    })
                });

                let old_import_path =
                    std::mem::replace(&mut self.current_import_path, defining_path);

                self.run_user_defined_callable::<_, (), _>(
                    MaybeEvaledArguments::Invocation(args),
                    mixin,
                    &env,
                    include_stmt.name.span,
                    |mixin, visitor| {
                        visitor.with_content(callable_content, |visitor| {
                            for stmt in mixin.body.iter() {
                                let result = visitor.visit_stmt_ref(stmt)?;
                                debug_assert!(result.is_none());
                            }
                            Ok(())
                        })
                    },
                )?;

                self.current_import_path = old_import_path;
                self.flags.set(ContextFlags::IN_MIXIN, old_in_mixin);

                Ok(None)
            }
        }
    }

    fn visit_mixin_decl(&mut self, mixin: AstMixin<'static>) {
        let defining_path = self.current_import_path.clone();
        self.env.insert_mixin(
            mixin.name,
            Mixin::UserDefined(mixin, self.env.new_closure(), defining_path),
        );
    }

    fn visit_each_stmt(&mut self, each_stmt: AstEach<'static>) -> SassResult<Option<Value>> {
        let list = self.visit_expr(each_stmt.list)?.as_list();

        // todo: not setting semi_global: true maybe means we can't assign to global scope when declared as global
        self.env.scope_enter();

        let mut result = None;

        'outer: for val in list {
            if each_stmt.variables.len() == 1 {
                let val = self.without_slash(val);
                self.env
                    .scopes_mut()
                    .insert_var_last(each_stmt.variables[0], val);
            } else {
                for (&var, val) in each_stmt.variables.iter().zip(
                    val.as_list()
                        .into_iter()
                        .chain(std::iter::once(Value::Null).cycle()),
                ) {
                    let val = self.without_slash(val);
                    self.env.scopes_mut().insert_var_last(var, val);
                }
            }

            for stmt in each_stmt.body.iter() {
                let val = self.visit_stmt_ref(stmt)?;
                if val.is_some() {
                    result = val;
                    break 'outer;
                }
            }
        }

        self.env.scope_exit();

        Ok(result)
    }

    fn visit_for_stmt(&mut self, for_stmt: AstFor<'static>) -> SassResult<Option<Value>> {
        let from_span = for_stmt.from.span;
        let to_span = for_stmt.to.span;
        let from_number = self
            .visit_expr(for_stmt.from.node)?
            .assert_number(from_span)?;
        let to_number = self.visit_expr(for_stmt.to.node)?.assert_number(to_span)?;

        if !to_number.unit().comparable(from_number.unit()) {
            // todo: better error message here
            return Err((
                "to and from values have incompatible units",
                from_span.merge(to_span),
            )
                .into());
        }

        let from = from_number.num.assert_int(from_span)?;
        let mut to = to_number
            .num
            .convert(to_number.unit(), from_number.unit())
            .assert_int(to_span)?;

        let direction = if from > to { -1 } else { 1 };

        if to == i64::MAX || to == i64::MIN {
            return Err((
                "@for loop upper bound exceeds valid integer representation (i64::MAX)",
                to_span,
            )
                .into());
        }

        if !for_stmt.is_exclusive {
            to += direction;
        }

        if from == to {
            return Ok(None);
        }

        // todo: self.with_scopes
        self.env.scope_enter();

        let mut result = None;

        let mut i = from;
        'outer: while i != to {
            self.env.scopes_mut().insert_var_last(
                for_stmt.variable.node,
                Value::Dimension(SassNumber {
                    num: Number::from(i),
                    unit: from_number.unit().clone(),
                    as_slash: None,
                }),
            );

            for stmt in for_stmt.body.iter() {
                let val = self.visit_stmt_ref(stmt)?;
                if val.is_some() {
                    result = val;
                    break 'outer;
                }
            }

            i += direction;
        }

        self.env.scope_exit();

        Ok(result)
    }

    fn visit_while_stmt(&mut self, while_stmt: &AstWhile<'static>) -> SassResult<Option<Value>> {
        self.with_scope(true, true, |visitor| {
            let mut result = None;

            'outer: while visitor
                .visit_expr(while_stmt.condition.clone())?
                .is_truthy()
            {
                for stmt in while_stmt.body.iter() {
                    let val = visitor.visit_stmt_ref(stmt)?;
                    if val.is_some() {
                        result = val;
                        break 'outer;
                    }
                }
            }

            Ok(result)
        })
    }

    fn visit_if_stmt(&mut self, if_stmt: AstIf<'static>) -> SassResult<Option<Value>> {
        let mut clause: Option<&[AstStmt<'static>]> = if_stmt.else_clause;
        for clause_to_check in &if_stmt.if_clauses {
            if self.visit_expr(clause_to_check.condition.clone())?.is_truthy() {
                clause = Some(clause_to_check.body);
                break;
            }
        }

        // todo: self.with_scope
        self.env.scope_enter();

        let mut result = None;

        if let Some(stmts) = clause {
            for stmt in stmts {
                let val = self.visit_stmt(stmt)?;
                if val.is_some() {
                    result = val;
                    break;
                }
            }
        }

        self.env.scope_exit();

        Ok(result)
    }

    fn visit_loud_comment(&mut self, comment: AstLoudComment<'static>) -> SassResult<Option<Value>> {
        if self.flags.in_function() {
            return Ok(None);
        }

        let comment = CssStmt::Comment(
            self.perform_interpolation(comment.text, false)?,
            comment.span,
        );

        // At ROOT level during the import section, accumulate comments with
        // pending imports so they can be interleaved correctly in the output.
        let at_root = self.parent.is_none() || self.parent == Some(CssTree::ROOT);
        if at_root && self.in_module_import_section {
            self.pending_import_items.push(comment);
        } else {
            self.add_child_to_current_parent(comment);
        }

        Ok(None)
    }

    fn visit_variable_decl(&mut self, decl: AstVariableDecl<'static>) -> SassResult<Option<Value>> {
        let name = Spanned {
            node: decl.name,
            span: decl.span,
        };

        if decl.is_guarded {
            if decl.namespace.is_none() && self.env.at_root() {
                let var_override = (*self.configuration).borrow_mut().remove(decl.name);
                if !matches!(
                    var_override,
                    Some(ConfiguredValue {
                        value: Value::Null,
                        ..
                    }) | None
                ) {
                    self.env.insert_var(
                        name,
                        None,
                        var_override.unwrap().value,
                        true,
                        self.flags.in_semi_global_scope(),
                    )?;
                    return Ok(None);
                }
            }

            if self.env.var_exists(decl.name, decl.namespace, decl.span)? {
                let value = self.env.get_var(name, decl.namespace).unwrap();

                if value != Value::Null {
                    return Ok(None);
                }
            }
        }

        let value = self.visit_expr(decl.value)?;
        let value = self.without_slash(value);

        self.env.insert_var(
            name,
            decl.namespace,
            value,
            decl.is_global,
            self.flags.in_semi_global_scope(),
        )?;

        Ok(None)
    }

    fn interpolation_to_value(
        &mut self,
        interpolation: Interpolation<'static>,
        // default=false
        trim: bool,
        // default=false
        warn_for_color: bool,
    ) -> SassResult<String> {
        let result = self.perform_interpolation(interpolation, warn_for_color)?;

        Ok(if trim {
            trim_ascii(&result, true).to_owned()
        } else {
            result
        })
    }

    /// Resolve interpolation by reference, cloning only string parts and
    /// evaluating expressions via visit_expr_ref.
    fn perform_interpolation_ref(
        &mut self,
        interpolation: &Interpolation<'static>,
        _warn_for_color: bool,
    ) -> SassResult<String> {
        let result = match interpolation.contents.len() {
            0 => String::new(),
            1 => match &interpolation.contents[0] {
                InterpolationPart::String(s) => s.clone(),
                InterpolationPart::Expr(e) => {
                    let span = e.span;
                    let result = self.visit_expr_ref(&e.node)?;
                    self.serialize(result, QuoteKind::None, span)?
                }
            },
            _ => interpolation
                .contents
                .iter()
                .map(|part| match part {
                    InterpolationPart::String(s) => Ok(s.clone()),
                    InterpolationPart::Expr(e) => {
                        let span = e.span;
                        let result = self.visit_expr_ref(&e.node)?;
                        self.serialize(result, QuoteKind::None, span)
                    }
                })
                .collect::<SassResult<String>>()?,
        };

        Ok(result)
    }

    fn perform_interpolation(
        &mut self,
        mut interpolation: Interpolation<'static>,
        // todo check to emit warning if this is true
        _warn_for_color: bool,
    ) -> SassResult<String> {
        let result = match interpolation.contents.len() {
            0 => String::new(),
            1 => match interpolation.contents.pop() {
                Some(InterpolationPart::String(s)) => s,
                Some(InterpolationPart::Expr(e)) => {
                    let span = e.span;
                    let result = self.visit_expr(e.node)?;
                    // todo: span for specific expr
                    self.serialize(result, QuoteKind::None, span)?
                }
                None => unreachable!(),
            },
            _ => interpolation
                .contents
                .into_iter()
                .map(|part| match part {
                    InterpolationPart::String(s) => Ok(s),
                    InterpolationPart::Expr(e) => {
                        let span = e.span;
                        let result = self.visit_expr(e.node)?;
                        // todo: span for specific expr
                        self.serialize(result, QuoteKind::None, span)
                    }
                })
                .collect::<SassResult<String>>()?,
        };

        Ok(result)
    }

    fn evaluate_to_css(
        &mut self,
        expr: AstExpr<'static>,
        quote: QuoteKind,
        span: Span,
    ) -> SassResult<String> {
        let result = self.visit_expr(expr)?;
        self.serialize(result, quote, span)
    }

    #[allow(clippy::unused_self)]
    fn without_slash(&mut self, v: Value) -> Value {
        match v {
            Value::Dimension(SassNumber { .. }) if v.as_slash().is_some() => {
                // todo: emit warning. we don't currently because it can be quite loud
                // self.emit_warning(
                //     Cow::Borrowed("Using / for division is deprecated and will be removed at some point in the future"),
                //     self.empty_span,
                // );
            }
            _ => {}
        }

        v.without_slash()
    }

    fn eval_maybe_args(
        &mut self,
        args: MaybeEvaledArguments<'static>,
        span: Span,
    ) -> SassResult<ArgumentResult> {
        match args {
            MaybeEvaledArguments::Invocation(args) => self.eval_args(args, span),
            MaybeEvaledArguments::Evaled(args) => Ok(args),
        }
    }

    fn eval_args(
        &mut self,
        arguments: ArgumentInvocation<'static>,
        span: Span,
    ) -> SassResult<ArgumentResult> {
        let mut positional = Vec::with_capacity(arguments.positional.len());

        for expr in arguments.positional {
            let val = self.visit_expr(expr)?;
            positional.push(self.without_slash(val));
        }

        let mut named = BTreeMap::new();

        for (key, expr) in arguments.named {
            let val = self.visit_expr(expr)?;
            named.insert(key, self.without_slash(val));
        }

        if arguments.rest.is_none() {
            return Ok(ArgumentResult {
                positional,
                named,
                separator: ListSeparator::Undecided,
                span,
                touched: BTreeSet::new(),
            });
        }

        let rest = self.visit_expr(arguments.rest.unwrap())?;

        let mut separator = ListSeparator::Undecided;

        match rest {
            Value::Map(rest) => self.add_rest_map(&mut named, rest)?,
            Value::List(elems, list_separator, _) => {
                positional.extend(
                    Rc::unwrap_or_clone(elems)
                        .into_iter()
                        .map(|e| self.without_slash(e)),
                );
                separator = list_separator;
            }
            Value::ArgList(arglist) => {
                // todo: superfluous clone
                for (&key, value) in arglist.keywords() {
                    named.insert(key, self.without_slash(value.clone()));
                }

                positional.extend(
                    arglist.elems.into_iter().map(|e| self.without_slash(e)),
                );
                separator = arglist.separator;
            }
            _ => {
                positional.push(self.without_slash(rest));
            }
        }

        if arguments.keyword_rest.is_none() {
            return Ok(ArgumentResult {
                positional,
                named,
                separator,
                span: arguments.span,
                touched: BTreeSet::new(),
            });
        }

        match self.visit_expr(arguments.keyword_rest.unwrap())? {
            Value::Map(keyword_rest) => {
                self.add_rest_map(&mut named, keyword_rest)?;

                Ok(ArgumentResult {
                    positional,
                    named,
                    separator,
                    span: arguments.span,
                    touched: BTreeSet::new(),
                })
            }
            v => Err((
                format!(
                    "Variable keyword arguments must be a map (was {}).",
                    v.inspect(arguments.span)?
                ),
                arguments.span,
            )
                .into()),
        }
    }

    fn add_rest_map(
        &mut self,
        named: &mut BTreeMap<Identifier, Value>,
        rest: SassMap,
    ) -> SassResult<()> {
        for (key, val) in rest {
            match key.node {
                Value::String(text, ..) => {
                    let val = self.without_slash(val);
                    named.insert(Identifier::from(text.as_str()), val);
                }
                _ => {
                    return Err((
                        // todo: we have to render the map for this error message
                        "Variable keyword argument map must have string keys.",
                        key.span,
                    )
                        .into());
                }
            }
        }

        Ok(())
    }

    pub(crate) fn run_user_defined_callable<
        F: UserDefinedCallable,
        V: fmt::Debug,
        R: FnOnce(F, &mut Self) -> SassResult<V>,
    >(
        &mut self,
        arguments: MaybeEvaledArguments<'static>,
        func: F,
        env: &Environment,
        span: Span,
        run: R,
    ) -> SassResult<V> {
        let mut evaluated = self.eval_maybe_args(arguments, span)?;

        self.with_environment(env.new_closure(), |visitor| {
            visitor.with_scope(false, true, move |visitor| {
                func.arguments().verify(
                    evaluated.positional.len(),
                    &evaluated.named,
                    evaluated.span,
                )?;

                let declared_arguments = &func.arguments().args;
                let min_len = evaluated.positional.len().min(declared_arguments.len());

                let positional_len = evaluated.positional.len();

                // Drain positional args in forward order (O(n) total vs O(n²) from remove())
                for (i, val) in evaluated.positional.drain(..min_len).enumerate() {
                    visitor.env.scopes_mut().insert_var_last(
                        declared_arguments[i].name,
                        val,
                    );
                }

                // todo: better name for var
                let additional_declared_args = if declared_arguments.len() > positional_len {
                    &declared_arguments[positional_len..declared_arguments.len()]
                } else {
                    &[]
                };

                for argument in additional_declared_args {
                    let name = argument.name;
                    let value = evaluated.named.remove(&argument.name).map_or_else(
                        || {
                            // todo: superfluous clone
                            let v = visitor.visit_expr(argument.default.clone().unwrap())?;
                            Ok(visitor.without_slash(v))
                        },
                        SassResult::Ok,
                    )?;
                    visitor.env.scopes_mut().insert_var_last(name, value);
                }

                let num_named_args = evaluated.named.len();

                let were_keywords_accessed = if let Some(rest_arg) = func.arguments().rest {
                    let rest = if !evaluated.positional.is_empty() {
                        evaluated.positional
                    } else {
                        Vec::new()
                    };

                    let were_keywords_accessed = Rc::new(Cell::new(false));
                    let arg_list = Value::ArgList(ArgList::new(
                        rest,
                        Rc::clone(&were_keywords_accessed),
                        // todo: superfluous clone
                        evaluated.named.clone(),
                        if evaluated.separator == ListSeparator::Undecided {
                            ListSeparator::Comma
                        } else {
                            evaluated.separator
                        },
                    ));

                    visitor.env.scopes_mut().insert_var_last(rest_arg, arg_list);

                    Some(were_keywords_accessed)
                } else {
                    None
                };

                let val = run(func, visitor)?;

                let were_keywords_accessed = match were_keywords_accessed {
                    Some(w) => w,
                    None => return Ok(val),
                };

                if num_named_args == 0 {
                    return Ok(val);
                }

                if (*were_keywords_accessed).get() {
                    return Ok(val);
                }

                let argument_word = if num_named_args == 1 {
                    "argument"
                } else {
                    "arguments"
                };

                let argument_names = to_sentence(
                    evaluated
                        .named
                        .keys()
                        .map(|key| format!("${key}", key = key))
                        .collect(),
                    "or",
                );

                Err((
                    format!(
                        "No {argument_word} named {argument_names}.",
                        argument_word = argument_word,
                        argument_names = argument_names
                    ),
                    span,
                )
                    .into())
            })
        })
    }

    pub(crate) fn run_function_callable(
        &mut self,
        func: SassFunction,
        arguments: ArgumentInvocation<'static>,
        span: Span,
    ) -> SassResult<Value> {
        self.run_function_callable_with_maybe_evaled(
            func,
            MaybeEvaledArguments::Invocation(arguments),
            span,
        )
    }

    pub(crate) fn run_function_callable_with_maybe_evaled(
        &mut self,
        func: SassFunction,
        arguments: MaybeEvaledArguments<'static>,
        span: Span,
    ) -> SassResult<Value> {
        match func {
            SassFunction::Builtin(func, _name) => {
                let evaluated = self.eval_maybe_args(arguments, span)?;
                let val = func.0(evaluated, self)?;
                Ok(self.without_slash(val))
            }
            SassFunction::UserDefined(UserDefinedFunction { function, env, .. }) => self
                .run_user_defined_callable(arguments, function, &env, span, |function, visitor| {
                    let old_in_mixin = visitor.flags.in_mixin();
                    visitor.flags.set(ContextFlags::IN_MIXIN, false);
                    for stmt in function.body.iter() {
                        let result = visitor.visit_stmt_ref(stmt)?;

                        if let Some(val) = result {
                            visitor.flags.set(ContextFlags::IN_MIXIN, old_in_mixin);
                            return Ok(val);
                        }
                    }
                    visitor.flags.set(ContextFlags::IN_MIXIN, old_in_mixin);

                    Err(("Function finished without @return.", span).into())
                }),
            SassFunction::Plain {
                name,
                original_name,
            } => {
                let has_named;
                let mut rest = None;
                let is_calc = name.as_str() == "calc";

                // todo: somewhat hacky solution to support plain css fns passed
                // as strings to `call(..)`
                let arguments = match arguments {
                    MaybeEvaledArguments::Invocation(args) => {
                        has_named = !args.named.is_empty() || args.keyword_rest.is_some();
                        rest = args.rest;

                        let mut result = Vec::with_capacity(args.positional.len());
                        for arg in args.positional {
                            let value = self.visit_expr(arg)?;

                            // When calc() falls back to Plain function (due to
                            // $variables in space-separated content), validate
                            // that the resolved values aren't adjacent numbers
                            // without operators (e.g., calc($c $d) where both
                            // are numbers should error).
                            if is_calc {
                                Self::validate_calc_value(&value, span)?;
                            }

                            result.push(self.serialize(value, QuoteKind::Quoted, span)?);
                        }
                        result
                    }
                    MaybeEvaledArguments::Evaled(args) => {
                        has_named = !args.named.is_empty();

                        args.positional
                            .into_iter()
                            .map(|arg| arg.to_css_string(span, self.options.is_compressed()))
                            .collect::<SassResult<Vec<_>>>()?
                    }
                };

                if has_named {
                    return Err(
                        ("Plain CSS functions don't support keyword arguments.", span).into(),
                    );
                }

                let mut buffer = format!("{}(", original_name);
                let mut first = true;

                for argument in arguments {
                    if first {
                        first = false;
                    } else {
                        buffer.push_str(", ");
                    }

                    buffer.push_str(&argument);
                }

                if let Some(rest_arg) = rest {
                    let rest = self.visit_expr(rest_arg)?;
                    if !first {
                        buffer.push_str(", ");
                    }
                    buffer.push_str(&self.serialize(rest, QuoteKind::Quoted, span)?);
                }
                buffer.push(')');

                Ok(Value::String(buffer.into(), QuoteKind::None))
            }
        }
    }

    /// Validates that a calc() argument value doesn't contain adjacent
    /// numeric values without operators (e.g., `calc($c $d)` where both
    /// resolve to numbers should error with "Missing math operator").
    fn validate_calc_value(value: &Value, span: Span) -> SassResult<()> {
        if let Value::List(items, ListSeparator::Space, _) = value {
            // Check for adjacent non-string values (numbers, dimensions)
            // without operator strings between them. A valid calc with
            // variables would have strings like "+ 2" between values.
            let mut prev_was_numeric = false;
            for item in items.iter() {
                let is_numeric = matches!(item, Value::Dimension(..));
                if is_numeric && prev_was_numeric {
                    return Err(("Missing math operator.", span).into());
                }
                prev_was_numeric = is_numeric;
            }
        }
        Ok(())
    }

    fn visit_list_expr(&mut self, list: ListExpr<'static>) -> SassResult<Value> {
        let elems = list
            .elems
            .into_iter()
            .map(|e| {
                let value = self.visit_expr(e.node)?;
                Ok(value)
            })
            .collect::<SassResult<Vec<_>>>()?;

        Ok(Value::List(Rc::new(elems), list.separator, list.brackets))
    }

    fn visit_function_call_expr(&mut self, func_call: FunctionCallExpr<'static>) -> SassResult<Value> {
        let name = func_call.name;
        let original_name = func_call.original_name;

        // If the function name starts with -- AND was written with hyphens in source
        // (not underscores normalized to hyphens), treat as CSS custom function
        if name.as_str().starts_with("--") && func_call.is_css_custom_function {
            return self.run_function_callable(
                SassFunction::Plain {
                    name,
                    original_name,
                },
                func_call.arguments.clone(),
                func_call.span,
            );
        }

        let func = match self.env.get_fn(name, func_call.namespace, func_call.span)? {
            Some(func) => func,
            None => {
                // When a namespace is specified (e.g., color.foo()), don't fall through
                // to global builtins — the function must exist in the module.
                if func_call.namespace.is_some() {
                    return Err(("Undefined function.", func_call.span).into());
                }

                if let Some(f) = self.options.custom_fns.get(name.as_str()) {
                    SassFunction::Builtin(f.clone(), name)
                } else if let Some(f) = GLOBAL_FUNCTIONS.get(name.as_str()) {
                    SassFunction::Builtin(f.clone(), name)
                } else {
                    SassFunction::Plain {
                        name,
                        original_name,
                    }
                }
            }
        };

        let old_in_function = self.flags.in_function();
        self.flags.set(ContextFlags::IN_FUNCTION, true);
        let value =
            self.run_function_callable(func, func_call.arguments.clone(), func_call.span)?;
        self.flags.set(ContextFlags::IN_FUNCTION, old_in_function);

        Ok(value)
    }

    fn visit_interpolated_func_expr(&mut self, func: InterpolatedFunction<'static>) -> SassResult<Value> {
        let InterpolatedFunction {
            name,
            arguments: args,
            span,
        } = func;
        let fn_name = self.perform_interpolation(name, false)?;

        if !args.named.is_empty() || args.keyword_rest.is_some() {
            return Err(("Plain CSS functions don't support keyword arguments.", span).into());
        }

        let mut buffer = format!("{}(", fn_name);

        let mut first = true;
        for arg in args.positional.clone() {
            if first {
                first = false;
            } else {
                buffer.push_str(", ");
            }
            let evaluated = self.evaluate_to_css(arg, QuoteKind::Quoted, span)?;
            buffer.push_str(&evaluated);
        }

        if let Some(rest_arg) = args.rest {
            let rest = self.visit_expr(rest_arg)?;
            if !first {
                buffer.push_str(", ");
            }
            buffer.push_str(&self.serialize(rest, QuoteKind::None, span)?);
        }

        buffer.push(')');

        Ok(Value::String(buffer.into(), QuoteKind::None))
    }

    fn visit_parent_selector(&self) -> Value {
        // Use the original (pre-extension) selector, matching dart-sass's
        // `originalSelector` behavior. This ensures `&` in values reflects
        // the selector as written, not after @extend modifications.
        match &self.original_selector {
            Some(selector) => selector.clone().to_sass_list(),
            None => Value::Null,
        }
    }

    /// Evaluate an expression by reference.
    /// With arena allocation, all sub-expressions are behind `&'static` references,
    /// so we clone to get owned values where needed (clone is cheap for arena refs).
    fn visit_expr_ref(&mut self, expr: &AstExpr<'static>) -> SassResult<Value> {
        Ok(match expr {
            AstExpr::True => Value::True,
            AstExpr::False => Value::False,
            AstExpr::Null => Value::Null,
            AstExpr::Color(c) => Value::Color(Rc::clone(c)),
            AstExpr::Number { n, unit } => Value::Dimension(SassNumber {
                num: *n,
                unit: unit.clone(),
                as_slash: None,
            }),
            AstExpr::Variable { name, namespace } => self.env.get_var(*name, *namespace)?,
            AstExpr::ParentSelector => self.visit_parent_selector(),
            AstExpr::BinaryOp(binop) => {
                self.visit_bin_op(binop.lhs.clone(), binop.op, binop.rhs.clone(), binop.allows_slash, binop.span)?
            }
            AstExpr::Paren(inner) => self.visit_expr_ref(inner)?,
            AstExpr::UnaryOp(op, inner, span) => {
                self.visit_unary_op(*op, (*inner).clone(), *span)?
            }
            AstExpr::List(list) => self.visit_list_expr(list.clone())?,
            AstExpr::String(StringExpr(text, quote), ..) => self.visit_string(text.clone(), *quote)?,
            AstExpr::Calculation { name, args } => {
                self.visit_calculation_expr(*name, args.clone(), self.empty_span)?
            }
            AstExpr::CssIf(css_if) => self.visit_css_if((*css_if).clone())?,
            AstExpr::FunctionCall(func_call) => self.visit_function_call_expr(func_call.clone())?,
            AstExpr::If(if_expr) => self.visit_ternary((*if_expr).clone())?,
            AstExpr::InterpolatedFunction(func) => {
                self.visit_interpolated_func_expr((*func).clone())?
            }
            AstExpr::Map(map) => self.visit_map(map.clone())?,
            AstExpr::Supports(condition) => Value::String(
                self.visit_supports_condition((*condition).clone())?.into(),
                QuoteKind::None,
            ),
        })
    }

    fn visit_expr(&mut self, expr: AstExpr<'static>) -> SassResult<Value> {
        self.visit_expr_ref(&expr)
    }

    /// Check that a calculation function received the required number of arguments
    fn check_calc_args(
        args: &[CalculationArg],
        required: usize,
        name: &str,
        span: Span,
    ) -> SassResult<()> {
        if args.len() < required {
            let was_were = if args.len() == 1 { "was" } else { "were" };
            return Err((
                format!(
                    "{required} argument{} required, but only {} {was_were} passed.",
                    if required == 1 { "" } else { "s" },
                    args.len(),
                ),
                span,
            )
                .into());
        }
        Ok(())
    }

    fn visit_calculation_value(
        &mut self,
        expr: AstExpr<'static>,
        in_min_or_max: bool,
        span: Span,
    ) -> SassResult<CalculationArg> {
        Ok(match expr {
            AstExpr::Paren(inner) => {
                let result =
                    self.visit_calculation_value(inner.clone(), in_min_or_max, span)?;

                match result {
                    CalculationArg::String(text) => {
                        CalculationArg::String(format!("({})", text))
                    }
                    CalculationArg::Interpolation(text) => {
                        CalculationArg::String(format!("({})", text))
                    }
                    other => other,
                }
            }
            AstExpr::String(string_expr, _span) => {
                debug_assert!(string_expr.1 == QuoteKind::None);
                let text = self.perform_interpolation(string_expr.0.clone(), false)?;
                if string_expr.0.contents.len() == 1
                    && matches!(
                        string_expr.0.contents.first(),
                        Some(crate::ast::InterpolationPart::String(_))
                    )
                {
                    CalculationArg::String(text)
                } else {
                    CalculationArg::Interpolation(text)
                }
            }
            AstExpr::BinaryOp(binop) => {
                SassCalculation::operate_internal(
                    binop.op,
                    self.visit_calculation_value(binop.lhs.clone(), in_min_or_max, span)?,
                    self.visit_calculation_value(binop.rhs.clone(), in_min_or_max, span)?,
                    in_min_or_max,
                    !self.flags.in_supports_declaration(),
                    self.options,
                    span,
                )?
            }
            AstExpr::Number { .. }
            | AstExpr::Calculation { .. }
            | AstExpr::Variable { .. }
            | AstExpr::CssIf(..)
            | AstExpr::FunctionCall { .. }
            | AstExpr::If(..)
            | AstExpr::UnaryOp(..) => {
                let result = self.visit_expr(expr)?;
                match result {
                    Value::Dimension(SassNumber {
                        num,
                        unit,
                        as_slash,
                    }) => CalculationArg::Number(SassNumber {
                        num,
                        unit,
                        as_slash,
                    }),
                    Value::Calculation(calc) => CalculationArg::Calculation(calc),
                    Value::String(s, QuoteKind::None) => CalculationArg::String(s.into()),
                    value => {
                        return Err((
                            format!(
                                "Value {} can't be used in a calculation.",
                                value.inspect(span)?
                            ),
                            span,
                        )
                            .into())
                    }
                }
            }
            v => unreachable!("{:?}", v),
        })
    }

    fn visit_calculation_expr(
        &mut self,
        name: CalculationName,
        mut ast_args: Vec<AstExpr<'static>>,
        span: Span,
    ) -> SassResult<Value> {
        // For single-arg functions (abs, round), when calculation arg
        // resolution fails due to incompatible units (e.g. abs(1 + 1px)),
        // fall back to evaluating as the Sass math function where unitless
        // values freely combine with units.
        let single_arg_fallback = matches!(
            name,
            CalculationName::Abs | CalculationName::Round
        ) && ast_args.len() == 1;

        let resolved = ast_args
            .iter()
            .map(|arg| self.visit_calculation_value(arg.clone(), name.in_min_or_max(), span))
            .collect::<SassResult<Vec<_>>>();

        let mut args = match resolved {
            Ok(args) => args,
            Err(e) if single_arg_fallback => {
                let val = self.visit_expr(ast_args.remove(0))?;
                return match val {
                    Value::Dimension(n) if name == CalculationName::Abs => {
                        Ok(Value::Dimension(SassNumber {
                            num: n.num.abs(),
                            unit: n.unit,
                            as_slash: None,
                        }))
                    }
                    Value::Dimension(n) if name == CalculationName::Round => {
                        Ok(Value::Dimension(SassNumber {
                            num: (n.num.0.round()).into(),
                            unit: n.unit,
                            as_slash: None,
                        }))
                    }
                    _ => Err(e),
                };
            }
            Err(e) => return Err(e),
        };

        if self.flags.in_supports_declaration() {
            return Ok(Value::Calculation(SassCalculation::unsimplified(
                name, args,
            )));
        }

        match name {
            CalculationName::Calc => {
                debug_assert_eq!(args.len(), 1);
                Ok(SassCalculation::calc(args.pop().unwrap()))
            }
            CalculationName::Min => SassCalculation::min(args, self.options, span),
            CalculationName::Max => SassCalculation::max(args, self.options, span),
            CalculationName::Clamp => {
                let mut iter = args.into_iter();
                let min = iter.next().unwrap();
                let value = iter.next();
                let max = iter.next();
                SassCalculation::clamp(min, value, max, self.options, span)
            }
            CalculationName::Abs => {
                Self::check_calc_args(&args, 1, "abs", span)?;
                SassCalculation::abs(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Exp => {
                Self::check_calc_args(&args, 1, "exp", span)?;
                SassCalculation::exp(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Sign => {
                Self::check_calc_args(&args, 1, "sign", span)?;
                SassCalculation::sign(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Sin => {
                Self::check_calc_args(&args, 1, "sin", span)?;
                SassCalculation::sin(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Cos => {
                Self::check_calc_args(&args, 1, "cos", span)?;
                SassCalculation::cos(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Tan => {
                Self::check_calc_args(&args, 1, "tan", span)?;
                SassCalculation::tan(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Asin => {
                Self::check_calc_args(&args, 1, "asin", span)?;
                SassCalculation::asin(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Acos => {
                Self::check_calc_args(&args, 1, "acos", span)?;
                SassCalculation::acos(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Atan => {
                Self::check_calc_args(&args, 1, "atan", span)?;
                SassCalculation::atan(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Sqrt => {
                Self::check_calc_args(&args, 1, "sqrt", span)?;
                SassCalculation::sqrt(args.pop().unwrap(), self.options, span)
            }
            CalculationName::Atan2 => {
                Self::check_calc_args(&args, 2, "atan2", span)?;
                SassCalculation::atan2(args, self.options, span)
            }
            CalculationName::Pow => {
                Self::check_calc_args(&args, 2, "pow", span)?;
                SassCalculation::pow(args, self.options, span)
            }
            CalculationName::Log => {
                if args.is_empty() {
                    return Err((
                        "1 argument required, but only 0 were passed.",
                        span,
                    )
                        .into());
                }
                SassCalculation::log(args, self.options, span)
            }
            CalculationName::Hypot => {
                if args.is_empty() {
                    return Err((
                        "hypot() must have at least one argument.",
                        span,
                    )
                        .into());
                }
                SassCalculation::hypot(args, self.options, span)
            }
            CalculationName::Mod => {
                Self::check_calc_args(&args, 2, "mod", span)?;
                SassCalculation::calc_mod(args, self.options, span)
            }
            CalculationName::Rem => {
                Self::check_calc_args(&args, 2, "rem", span)?;
                SassCalculation::calc_rem(args, self.options, span)
            }
            CalculationName::CalcSize => {
                Self::check_calc_args(&args, 1, "calc-size", span)?;
                Ok(SassCalculation::calc_size(args))
            }
            CalculationName::Round => {
                // round() can have 1-3 args. With 2-3 args, first might be a strategy keyword.
                let strategy = if args.len() >= 2 {
                    let s = match &args[0] {
                        CalculationArg::String(s)
                        | CalculationArg::Interpolation(s) => {
                            let lower = s.to_ascii_lowercase();
                            if matches!(lower.as_str(), "nearest" | "up" | "down" | "to-zero") {
                                Some(lower)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if s.is_some() {
                        args.remove(0);
                    }
                    s
                } else {
                    None
                };
                SassCalculation::round(args, strategy, self.options, span)
            }
        }
    }

    fn visit_unary_op(&mut self, op: UnaryOp, expr: AstExpr<'static>, span: Span) -> SassResult<Value> {
        let operand = self.visit_expr(expr)?;

        match op {
            UnaryOp::Plus => operand.unary_plus(self, span),
            UnaryOp::Neg => operand.unary_neg(self, span),
            UnaryOp::Div => operand.unary_div(self, span),
            UnaryOp::Not => Ok(operand.unary_not()),
        }
    }

    fn visit_ternary(&mut self, if_expr: Ternary<'static>) -> SassResult<Value> {
        // When rest args are present, evaluate all args eagerly (can't do lazy
        // evaluation since rest values are already evaluated)
        if if_expr.0.rest.is_some() {
            let span = if_expr.0.span;
            let mut args = self.eval_args(if_expr.0, span)?;
            args.max_args(3)?;
            let value = if args.get_err(0, "condition")?.is_truthy() {
                args.get_err(1, "if-true")?
            } else {
                args.get_err(2, "if-false")?
            };
            return Ok(self.without_slash(value));
        }

        if_arguments().verify(if_expr.0.positional.len(), &if_expr.0.named, if_expr.0.span)?;

        let mut positional = if_expr.0.positional;
        let mut named = if_expr.0.named;

        let condition = if positional.is_empty() {
            named.remove(&Identifier::from("condition")).unwrap()
        } else {
            positional.remove(0)
        };

        let if_true = if positional.is_empty() {
            named.remove(&Identifier::from("if_true")).unwrap()
        } else {
            positional.remove(0)
        };

        let if_false = if positional.is_empty() {
            named.remove(&Identifier::from("if_false")).unwrap()
        } else {
            positional.remove(0)
        };

        let value = if self.visit_expr(condition)?.is_truthy() {
            self.visit_expr(if_true)?
        } else {
            self.visit_expr(if_false)?
        };

        Ok(self.without_slash(value))
    }

    fn visit_css_if(&mut self, css_if: CssIfExpression<'static>) -> SassResult<Value> {
        // Validate: sass() and raw substitutions cannot coexist in same condition
        for clause in &css_if.clauses {
            self.check_no_sass_with_raw(&clause.condition, css_if.span)?;
        }

        // Evaluate each clause
        for clause in &css_if.clauses {
            match self.eval_if_condition(&clause.condition)? {
                ConditionResult::True => {
                    let value = self.visit_expr(clause.value.clone())?;
                    return Ok(self.without_slash(value));
                }
                ConditionResult::False => continue,
                ConditionResult::Css(remaining) => {
                    // This clause has CSS parts that can't be evaluated.
                    // Collect remaining clauses as CSS output.
                    return self.build_css_if_output(&remaining, clause, &css_if);
                }
            }
        }

        // No clause matched, no else → null
        Ok(Value::Null)
    }

    fn build_css_if_output(
        &mut self,
        first_remaining: &IfCondition<'static>,
        first_clause: &IfClause<'static>,
        css_if: &CssIfExpression<'static>,
    ) -> SassResult<Value> {
        let mut parts = Vec::new();

        // Add the first remaining clause
        let cond_str = self.serialize_if_condition(first_remaining)?;
        let val_str = self.evaluate_to_css(first_clause.value.clone(), QuoteKind::None, css_if.span)?;
        parts.push(format!("{}: {}", cond_str, val_str));

        // Find remaining clauses after the first CSS one
        let first_idx = css_if
            .clauses
            .iter()
            .position(|c| std::ptr::eq(c, first_clause))
            .unwrap_or(0);

        for clause in &css_if.clauses[first_idx + 1..] {
            match &clause.condition {
                IfCondition::Else => {
                    let val_str = self.evaluate_to_css(
                        clause.value.clone(),
                        QuoteKind::None,
                        css_if.span,
                    )?;
                    parts.push(format!("else: {}", val_str));
                }
                other => {
                    match self.eval_if_condition(other)? {
                        ConditionResult::True => {
                            // Sass condition that's true — this becomes the value
                            let val_str = self.evaluate_to_css(
                                clause.value.clone(),
                                QuoteKind::None,
                                css_if.span,
                            )?;
                            // Replace all remaining with just this value
                            parts.push(format!("else: {}", val_str));
                            break;
                        }
                        ConditionResult::False => {
                            // Sass condition that's false — skip this clause
                            continue;
                        }
                        ConditionResult::Css(remaining) => {
                            let cond_str = self.serialize_if_condition(&remaining)?;
                            let val_str = self.evaluate_to_css(
                                clause.value.clone(),
                                QuoteKind::None,
                                css_if.span,
                            )?;
                            parts.push(format!("{}: {}", cond_str, val_str));
                        }
                    }
                }
            }
        }

        let output = format!("if({})", parts.join("; "));
        Ok(Value::String(output.into(), QuoteKind::None))
    }

    fn eval_if_condition(&mut self, condition: &IfCondition<'static>) -> SassResult<ConditionResult> {
        match condition {
            IfCondition::Else => Ok(ConditionResult::True),
            IfCondition::Atom(atom) => self.eval_if_atom(atom),
            IfCondition::Not(inner, _span) => {
                match self.eval_if_condition(inner)? {
                    ConditionResult::True => Ok(ConditionResult::False),
                    ConditionResult::False => Ok(ConditionResult::True),
                    ConditionResult::Css(inner_cond) => {
                        Ok(ConditionResult::Css(IfCondition::Not(
                            Box::new(inner_cond),
                            *_span,
                        )))
                    }
                }
            }
            IfCondition::Paren(inner) => {
                match self.eval_if_condition(inner)? {
                    ConditionResult::True => Ok(ConditionResult::True),
                    ConditionResult::False => Ok(ConditionResult::False),
                    ConditionResult::Css(inner_cond) => {
                        Ok(ConditionResult::Css(IfCondition::Paren(Box::new(inner_cond))))
                    }
                }
            }
            IfCondition::And(operands) => {
                let mut remaining_css = Vec::new();
                for op in operands {
                    match self.eval_if_condition(op)? {
                        ConditionResult::True => {
                            // True AND x → continue checking
                        }
                        ConditionResult::False => {
                            // False AND anything → false (short-circuit)
                            return Ok(ConditionResult::False);
                        }
                        ConditionResult::Css(css_cond) => {
                            remaining_css.push(css_cond);
                        }
                    }
                }
                if remaining_css.is_empty() {
                    Ok(ConditionResult::True)
                } else if remaining_css.len() == 1 {
                    // Unwrap Paren if the sole remaining was in a group
                    Ok(ConditionResult::Css(unwrap_paren(remaining_css.pop().unwrap())))
                } else {
                    Ok(ConditionResult::Css(IfCondition::And(remaining_css)))
                }
            }
            IfCondition::Or(operands) => {
                let mut remaining_css = Vec::new();
                for op in operands {
                    match self.eval_if_condition(op)? {
                        ConditionResult::True => {
                            // True OR anything → true (short-circuit)
                            return Ok(ConditionResult::True);
                        }
                        ConditionResult::False => {
                            // False OR x → continue checking
                        }
                        ConditionResult::Css(css_cond) => {
                            remaining_css.push(css_cond);
                        }
                    }
                }
                if remaining_css.is_empty() {
                    Ok(ConditionResult::False)
                } else if remaining_css.len() == 1 {
                    Ok(ConditionResult::Css(unwrap_paren(remaining_css.pop().unwrap())))
                } else {
                    Ok(ConditionResult::Css(IfCondition::Or(remaining_css)))
                }
            }
        }
    }

    /// Check that a condition doesn't mix sass() with raw substitutions.
    /// Rule: if raw substitutions exist at the current scope (not crossing paren
    /// boundaries), then sass() must not exist ANYWHERE in the tree (including
    /// inside parens). Raw inside parens does NOT conflict with sass at outer scope.
    fn check_no_sass_with_raw(
        &self,
        condition: &IfCondition<'static>,
        span: Span,
    ) -> SassResult<()> {
        let has_raw = condition_has_raw(condition);
        if has_raw {
            // Raw at this scope — check for sass anywhere (crossing paren boundaries)
            let has_sass = condition_has_sass(condition);
            if has_sass {
                return Err((
                    "if() conditions with arbitrary substitutions may not contain sass() expressions.",
                    span,
                )
                    .into());
            }
        }

        // Recurse into paren groups to check each scope independently
        self.check_parens_for_sass_raw(condition, span)
    }

    fn check_parens_for_sass_raw(
        &self,
        condition: &IfCondition<'static>,
        span: Span,
    ) -> SassResult<()> {
        match condition {
            IfCondition::Paren(inner) => {
                self.check_no_sass_with_raw(inner, span)?;
            }
            IfCondition::Not(inner, _) => {
                self.check_parens_for_sass_raw(inner, span)?;
            }
            IfCondition::And(ops) | IfCondition::Or(ops) => {
                for op in ops {
                    self.check_parens_for_sass_raw(op, span)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn eval_if_atom(&mut self, atom: &IfConditionAtom<'static>) -> SassResult<ConditionResult> {
        match atom {
            IfConditionAtom::Sass(expr, _span) => {
                let value = self.visit_expr(expr.clone())?;
                if value.is_truthy() {
                    Ok(ConditionResult::True)
                } else {
                    Ok(ConditionResult::False)
                }
            }
            IfConditionAtom::Css(interp, span)
            | IfConditionAtom::CssRaw(interp, span) => {
                // Evaluate any interpolations within the CSS text
                let text = self.perform_interpolation(interp.clone(), false)?;
                Ok(ConditionResult::Css(IfCondition::Atom(
                    IfConditionAtom::Css(
                        Interpolation::new_plain(text),
                        *span,
                    ),
                )))
            }
            IfConditionAtom::Interp(expr, span) => {
                let value = self.visit_expr(expr.clone())?;
                let text = self.serialize(value, QuoteKind::None, *span)?;
                Ok(ConditionResult::Css(IfCondition::Atom(
                    IfConditionAtom::Css(
                        Interpolation::new_plain(text),
                        *span,
                    ),
                )))
            }
        }
    }

    fn serialize_if_condition(&mut self, condition: &IfCondition<'static>) -> SassResult<String> {
        match condition {
            IfCondition::Else => Ok("else".to_string()),
            IfCondition::Atom(atom) => match atom {
                IfConditionAtom::Css(interp, _)
                | IfConditionAtom::CssRaw(interp, _) => {
                    Ok(interp.as_plain().unwrap_or("").to_string())
                }
                IfConditionAtom::Sass(_, _) => {
                    unreachable!("sass atoms should have been evaluated")
                }
                IfConditionAtom::Interp(_, _) => {
                    unreachable!("interpolation atoms should have been evaluated")
                }
            },
            IfCondition::Not(inner, _) => {
                let inner_str = self.serialize_if_condition(inner)?;
                Ok(format!("not {}", inner_str))
            }
            IfCondition::Paren(inner) => {
                let inner_str = self.serialize_if_condition(inner)?;
                Ok(format!("({})", inner_str))
            }
            IfCondition::And(operands) => {
                let parts: Vec<String> = operands
                    .iter()
                    .map(|op| self.serialize_if_condition(op))
                    .collect::<SassResult<_>>()?;
                Ok(parts.join(" and "))
            }
            IfCondition::Or(operands) => {
                let parts: Vec<String> = operands
                    .iter()
                    .map(|op| self.serialize_if_condition(op))
                    .collect::<SassResult<_>>()?;
                Ok(parts.join(" or "))
            }
        }
    }

    fn visit_string(&mut self, mut text: Interpolation<'static>, quote: QuoteKind) -> SassResult<Value> {
        // Don't use [performInterpolation] here because we need to get the raw text
        // from strings, rather than the semantic value.
        let old_in_supports_declaration = self.flags.in_supports_declaration();
        self.flags.set(ContextFlags::IN_SUPPORTS_DECLARATION, false);

        let result = match text.contents.len() {
            0 => String::new(),
            1 => match text.contents.pop() {
                Some(InterpolationPart::String(s)) => s,
                Some(InterpolationPart::Expr(Spanned { node, span })) => {
                    match self.visit_expr(node)? {
                        Value::String(s, ..) => s.to_string(),
                        e => self.serialize(e, QuoteKind::None, span)?,
                    }
                }
                None => unreachable!(),
            },
            _ => text
                .contents
                .into_iter()
                .map(|part| match part {
                    InterpolationPart::String(s) => Ok(s),
                    InterpolationPart::Expr(Spanned { node, span }) => {
                        match self.visit_expr(node)? {
                            Value::String(s, ..) => Ok(s.to_string()),
                            e => self.serialize(e, QuoteKind::None, span),
                        }
                    }
                })
                .collect::<SassResult<String>>()?,
        };

        self.flags.set(
            ContextFlags::IN_SUPPORTS_DECLARATION,
            old_in_supports_declaration,
        );

        Ok(Value::String(result.into(), quote))
    }

    fn visit_map(&mut self, map: AstSassMap<'static>) -> SassResult<Value> {
        let mut sass_map = SassMap::new();

        for pair in map.0 {
            let key_span = pair.0.span;
            let key = self.visit_expr(pair.0.node)?;
            let value = self.visit_expr(pair.1)?;

            if sass_map.get_ref(&key).is_some() {
                return Err(("Duplicate key.", key_span).into());
            }

            sass_map.insert(
                Spanned {
                    node: key,
                    span: key_span,
                },
                value,
            );
        }

        Ok(Value::Map(sass_map))
    }

    fn visit_bin_op(
        &mut self,
        lhs: AstExpr<'static>,
        op: BinaryOp,
        rhs: AstExpr<'static>,
        allows_slash: bool,
        span: Span,
    ) -> SassResult<Value> {
        let left = self.visit_expr(lhs)?;

        Ok(match op {
            BinaryOp::SingleEq => {
                let right = self.visit_expr(rhs)?;
                single_eq(&left, &right, self.options, span)?
            }
            BinaryOp::Or => {
                if left.is_truthy() {
                    left
                } else {
                    self.visit_expr(rhs)?
                }
            }
            BinaryOp::And => {
                if left.is_truthy() {
                    self.visit_expr(rhs)?
                } else {
                    left
                }
            }
            BinaryOp::Equal => {
                let right = self.visit_expr(rhs)?;
                Value::bool(left == right)
            }
            BinaryOp::NotEqual => {
                let right = self.visit_expr(rhs)?;
                Value::bool(left != right)
            }
            BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual
            | BinaryOp::LessThan
            | BinaryOp::LessThanEqual => {
                let right = self.visit_expr(rhs)?;
                cmp(&left, &right, self.options, span, op)?
            }
            BinaryOp::Plus => {
                let right = self.visit_expr(rhs)?;
                add(left, right, self.options, span)?
            }
            BinaryOp::Minus => {
                let right = self.visit_expr(rhs)?;
                sub(left, right, self.options, span)?
            }
            BinaryOp::Mul => {
                let right = self.visit_expr(rhs)?;
                mul(left, right, self.options, span)?
            }
            BinaryOp::Div => {
                let right = self.visit_expr(rhs)?;

                let left_is_number = matches!(left, Value::Dimension { .. });
                let right_is_number = matches!(right, Value::Dimension { .. });

                if left_is_number && right_is_number && allows_slash {
                    let result = div(left.clone(), right.clone(), self.options, span)?;
                    return result.with_slash(
                        left.assert_number(span)?,
                        right.assert_number(span)?,
                        span,
                    );
                } else if left_is_number && right_is_number {
                    // todo: emit warning here. it prints too frequently, so we do not currently
                    // self.emit_warning(
                    //     Cow::Borrowed(format!(
                    //         "Using / for division outside of calc() is deprecated"
                    //     )),
                    //     span,
                    // );
                }

                div(left, right, self.options, span)?
            }
            BinaryOp::Rem => {
                let right = self.visit_expr(rhs)?;
                rem(left, right, self.options, span)?
            }
        })
    }

    // todo: superfluous taking `expr` by value
    fn serialize(&mut self, mut expr: Value, quote: QuoteKind, span: Span) -> SassResult<String> {
        if quote == QuoteKind::None {
            expr = expr.unquote();
        }

        expr.to_css_string(span, self.options.is_compressed())
    }

    pub(crate) fn visit_ruleset(&mut self, ruleset: AstRuleSet<'static>) -> SassResult<Option<Value>> {
        if self.declaration_name.is_some() {
            return Err((
                "Style rules may not be used within nested declarations.",
                ruleset.span,
            )
                .into());
        }

        let AstRuleSet {
            selector: ruleset_selector,
            body: ruleset_body,
            ..
        } = ruleset;

        let selector_text = self.interpolation_to_value(ruleset_selector, true, true)?;

        if self.flags.in_keyframes() {
            if self.flags.in_keyframes_rule() {
                return Err((
                    "Style rules may not be used within keyframe blocks.",
                    ruleset.selector_span,
                )
                    .into());
            }

            let span = ruleset.selector_span;
            let sel_toks = Lexer::new_from_string(&selector_text, span);
            let parsed_selector =
                KeyframesSelectorParser::new(sel_toks).parse_keyframes_selector()?;

            let keyframes_ruleset = CssStmt::KeyframesRuleSet(KeyframesRuleSet {
                selector: parsed_selector,
                body: Vec::new(),
            });

            let was_in_keyframes_rule = self.flags.in_keyframes_rule();
            self.flags
                .set(ContextFlags::IN_KEYFRAMES_RULE, true);

            self.with_parent(
                keyframes_ruleset,
                true,
                |visitor| {
                    for stmt in ruleset_body {
                        let result = visitor.visit_stmt(stmt)?;
                        debug_assert!(result.is_none());
                    }

                    Ok(())
                },
                CssStmt::is_style_rule,
            )?;

            self.flags
                .set(ContextFlags::IN_KEYFRAMES_RULE, was_in_keyframes_rule);

            return Ok(None);
        }

        let mut parsed_selector = self.parse_selector_from_string(
            &selector_text,
            true, // allows_parent: always true (CSS nesting uses &)
            !self.is_plain_css,
            ruleset.selector_span,
        )?;

        // In plain CSS, reject & with suffix (&b) but allow & alone, &.class, .b&, etc.
        if self.is_plain_css {
            for complex in &parsed_selector.components {
                for component in &complex.components {
                    if let ComplexSelectorComponent::Compound(compound) = component {
                        for simple in &compound.components {
                            if let SimpleSelector::Parent(Some(_)) = simple {
                                return Err((
                                    "Parent selectors can't have suffixes in plain CSS.",
                                    ruleset.selector_span,
                                )
                                    .into());
                            }
                        }
                    }
                }

                // Reject leading combinators at the top level in plain CSS
                if self.plain_css_style_rule_depth == 0 {
                    if let Some(ComplexSelectorComponent::Combinator(..)) =
                        complex.components.first()
                    {
                        return Err((
                            "Top-level leading combinators aren't allowed in plain CSS.",
                            ruleset.selector_span,
                        )
                            .into());
                    }
                }

                // Reject trailing combinators in plain CSS
                if let Some(ComplexSelectorComponent::Combinator(..)) =
                    complex.components.last()
                {
                    return Err((
                        "expected selector.",
                        ruleset.selector_span,
                    )
                        .into());
                }
            }
        }

        // In plain CSS, skip parent resolution for nested rules (depth > 0)
        // and for selectors containing & at any depth. At depth 0 without &,
        // still resolve to handle @import context (e.g., a {@import "plain.css"}).
        let skip_resolution = self.is_plain_css
            && (self.plain_css_style_rule_depth > 0
                || parsed_selector.contains_parent_selector());

        if !skip_resolution {
            parsed_selector = parsed_selector.resolve_parent_selectors(
                self.style_rule_ignoring_at_root
                    .as_ref()
                    // todo: this clone should be superfluous(?)
                    .map(|x| x.as_selector_list().clone()),
                !self.flags.at_root_excluding_style_rule(),
            )?;
        }

        // Save the original (pre-extension) selector for `&` in value context.
        // This matches dart-sass's `originalSelector` on style rules.
        let original_selector = parsed_selector.clone();

        // todo: _mediaQueries
        let selector = self
            .extender
            .add_selector(parsed_selector, &self.media_queries)?;

        let rule = CssStmt::RuleSet {
            selector: selector.clone(),
            body: Vec::new(),
            is_group_end: false,
            source_span: Some(ruleset.span),
        };

        let old_at_root_excluding_style_rule = self.flags.at_root_excluding_style_rule();

        self.flags
            .set(ContextFlags::AT_ROOT_EXCLUDING_STYLE_RULE, false);

        let old_style_rule_ignoring_at_root = self.style_rule_ignoring_at_root.take();
        let old_original_selector = self.original_selector.take();
        self.style_rule_ignoring_at_root = Some(selector);
        self.original_selector = Some(original_selector);

        if self.is_plain_css {
            self.plain_css_style_rule_depth += 1;
        }

        // When resolution was skipped, the selector stays as-is, so the rule
        // must be a child of its parent (CSS nesting), not walked up.
        let nest_in_parent = skip_resolution;

        self.with_parent(
            rule,
            true,
            |visitor| {
                for stmt in ruleset_body {
                    let result = visitor.visit_stmt(stmt)?;
                    debug_assert!(result.is_none());
                }

                Ok(())
            },
            if nest_in_parent {
                (|_: &CssStmt| false) as fn(&CssStmt) -> bool
            } else {
                CssStmt::is_style_rule as fn(&CssStmt) -> bool
            },
        )?;

        if self.is_plain_css {
            self.plain_css_style_rule_depth -= 1;
        }

        self.style_rule_ignoring_at_root = old_style_rule_ignoring_at_root;
        self.original_selector = old_original_selector;
        self.flags.set(
            ContextFlags::AT_ROOT_EXCLUDING_STYLE_RULE,
            old_at_root_excluding_style_rule,
        );

        self.set_group_end();

        Ok(None)
    }

    fn set_group_end(&mut self) -> Option<()> {
        if !self.style_rule_exists() {
            let children = self
                .css_tree
                .parent_to_child
                .get(&self.parent.unwrap_or(CssTree::ROOT))?;
            let child = *children.last()?;
            self.css_tree
                .get_mut(child)
                .as_mut()
                .map(CssStmt::set_group_end)?;
        }

        Some(())
    }

    fn style_rule_exists(&self) -> bool {
        !self.flags.at_root_excluding_style_rule() && self.style_rule_ignoring_at_root.is_some()
    }

    pub(crate) fn visit_style(&mut self, style: AstStyle<'static>) -> SassResult<Option<Value>> {
        if !self.style_rule_exists()
            && !self.flags.in_unknown_at_rule()
            && !self.flags.in_keyframes()
        {
            return Err((
                "Declarations may only be used within style rules.",
                style.span,
            )
                .into());
        }

        let is_custom_property = style.is_custom_property();

        if is_custom_property && self.declaration_name.is_some() {
            return Err((
                "Declarations whose names begin with \"--\" may not be nested.",
                style.span,
            )
                .into());
        }

        let mut name = self.interpolation_to_value(style.name, false, true)?;

        if let Some(declaration_name) = &self.declaration_name {
            name = format!("{}-{}", declaration_name, name);
        }

        if let Some(value) = style
            .value
            .map(|s| {
                SassResult::Ok(Spanned {
                    node: self.visit_expr(s.node)?,
                    span: s.span,
                })
            })
            .transpose()?
        {
            // If the value is an empty list, preserve it, because converting it to CSS
            // will throw an error that we want the user to see.
            if !value.is_blank() || value.is_empty_list() || is_custom_property {
                // todo: superfluous clones?
                self.add_child_to_current_parent(
                    CssStmt::Style(Style {
                        property: InternedString::get_or_intern(&name),
                        value: Box::new(value),
                        declared_as_custom_property: is_custom_property,
                        property_span: style.span,
                    }),
                );
            }
        }

        let children = style.body;

        if !children.is_empty() {
            let old_declaration_name = self.declaration_name.take();
            self.declaration_name = Some(name);
            self.with_scope::<SassResult<()>, _>(false, true, |visitor| {
                for stmt in children {
                    let result = visitor.visit_stmt(stmt)?;
                    debug_assert!(result.is_none());
                }

                Ok(())
            })?;
            self.declaration_name = old_declaration_name;
        }

        Ok(None)
    }
}
