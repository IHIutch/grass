use std::{
    cell::{Cell, RefCell},
    ffi::OsStr,
    fmt,
    hash::{Hash, Hasher},
    iter::FromIterator,
    mem,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use codemap::{CodeMap, Span, Spanned};
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet, FxHasher};

/// IndexSet using FxHash instead of SipHash for faster hashing.
type FxIndexSet<V> = indexmap::IndexSet<V, FxBuildHasher>;

use crate::{
    ast::*,
    builtin::{
        global_builtin_message,
        meta::if_arguments,
        modules::{
            declare_module_color, declare_module_list, declare_module_map, declare_module_math,
            declare_module_meta, declare_module_selector, declare_module_string, Module,
        },
        BuiltinFn, GLOBAL_FUNCTIONS,
    },
    common::{
        unvendor, BinaryOp, Identifier, ListSeparator, NamedArgsView, QuoteKind, SmallOrderedMap,
        UnaryOp,
    },
    error::SassResult,
    importer::{ImportResolution, ImportSource},
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
    serializer::serialize_number,
    unit::Unit,
    utils::{to_sentence, trim_ascii},
    value::{
        ArgList, CalculationArg, CalculationName, Number, SassCalculation, SassFunction, SassMap,
        SassNumber, UserDefinedFunction, Value,
    },
    ContextFlags, Deprecation, InputSyntax, Options,
};

use super::{
    bin_op::{add, cmp, div, mul, rem, single_eq, sub},
    css_tree::{CssTree, CssTreeIdx},
    env::Environment,
};

/// Maximum nesting depth allowed for recursive user-defined
/// function/mixin/content-block invocation during evaluation — see
/// `Visitor::run_user_defined_callable`. This is separate from, and much
/// tighter than, `MAX_PARSER_RECURSION_DEPTH` (parse/stylesheet.rs): a
/// recursive callable's evaluation frame (argument binding, scope setup,
/// the full `visit_stmt`/`visit_expr` chain for its body, the closure passed
/// to `run_user_defined_callable`) costs far more stack per level than a
/// parser recursion step, so the same constant would force the worse of the
/// two everywhere.
///
/// Measured unguarded crash boundaries for callable recursion (a
/// `sum($n)`-shaped function — `@if $n <= 0 { @return 0 } @return $n +
/// sum($n - 1)`, no tail-call elimination since Sass has none — on an
/// explicit small-stack thread):
///
///   - release build, 1 MiB stack (napi's real worker-thread size, and the
///     only stack size this recursion actually runs on in production —
///     napi/CLI/wasm all ship release builds): survives 120, crashes at 128.
///   - debug build, 2 MiB stack (cargo test's actual default thread stack):
///     survives 56, crashes at 64.
///
/// These two numbers are in direct tension with dart-sass compatibility:
/// `sum(40)` and `sum(100)`-style bounded recursion compile in every other
/// Sass implementation and must compile here too (grass previously rejected
/// `sum(40)`, a confirmed regression — see solo todo #123 round-2 review).
/// Supporting `sum(100)` requires a guard of at least 101, which:
///
///   - leaves only ~1.2x margin under the release+1 MiB *crash* point (128)
///     and is 8% below the highest depth directly confirmed safe there (120)
///     — nowhere near a full 2x margin, because 128 is the actual ceiling in
///     the one environment this code ships on, not an arbitrary choice.
///   - has NO margin under debug+2 MiB — that environment's own unguarded
///     ceiling (64) is below what dart-sass-compatible recursion needs, so
///     no guard value can be both dart-sass-compatible and safe on a 2 MiB
///     debug stack. debug+2 MiB is never a deployment target (only
///     `cargo test`'s own process), so this constant is sized against the
///     real release+1 MiB deployment ceiling instead. The tests in
///     crates/lib/tests/deep_nesting.rs that exercise callable recursion
///     near this limit run on an explicitly larger stack for exactly this
///     reason — see the comment there.
///
/// 110 sits 10 levels below the confirmed-safe 120 in release+1 MiB (real,
/// measured margin, short of 2x) and 9 above the 101 `sum(100)` needs.
const MAX_CALLABLE_RECURSION_DEPTH: usize = 110;

/// Guards against stack overflow from plain style-rule nesting (`a { b { c {
/// ... } } }`), independent of `MAX_CALLABLE_RECURSION_DEPTH` above (which
/// only guards function/mixin/content-block invocation). `visit_ruleset`
/// recurses through `with_parent` -> `visit_stmt` for every nested `RuleSet`
/// child with no bound of its own — see solo todo #196, filed because todo
/// #148 wrapped the *parser's* recursion guard in `crate::stack::maybe_grow`
/// and raised `MAX_PARSER_RECURSION_DEPTH`, but the full `grass::from_string`
/// pipeline (parse + evaluate + serialize) then crashed with a genuine,
/// unguarded stack overflow during evaluation at depths well below the new
/// parser limit, because this chokepoint was never protected.
///
/// Measured unguarded full-pipeline crash boundaries for `a{a{a{...}}}`-shaped
/// input (todo #196, with the parser's own stack growth already active):
///
///   - release-napi profile, 1 MiB stack (napi's real worker-thread size, the
///     actual napi deployment ceiling): survives 370, crashes at 380.
///   - debug build, 2 MiB stack (cargo test's own default thread stack):
///     survives 260, crashes at 270.
///
/// This constant's chokepoint (`visit_ruleset`) is wrapped in the same
/// `crate::stack::maybe_grow` helper todo #148 added for the parser, which
/// moves the crash boundary out dramatically (measured, todo #196, both
/// limits temporarily raised to isolate this chokepoint): confirmed safe
/// (no crash, sub-second) at depth 1500 on release-napi/1 MiB and at depth
/// 1024 on debug/2 MiB — both far past the old ~380/~270 unguarded crash
/// points above. A stack overflow reappears somewhere between depth 12,000
/// (confirmed safe, though ~46s — compile time, not stack safety, is the
/// practical limit that far out) and depth 15,000 (crashes) on
/// release-napi/1 MiB; that reappearance was not root-caused (time-boxed —
/// it's far outside any depth a real stylesheet would use, and may be an
/// entirely different unguarded recursion, e.g. recursive `Drop` of the
/// nested AST/CssTree, not this chokepoint).
///
/// 1024 is chosen with over 10x margin under the reappeared ~12-15k crash
/// zone — well past this project's usual ~30% convention, because the
/// guarded boundary is so much higher than the unguarded one that matching
/// dart-sass 1.97.3's own ~450-500 level tolerance (with headroom, not bare
/// parity) was the real binding choice, not the crash point.
const MAX_STYLE_RULE_RECURSION_DEPTH: usize = 1024;

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
        IfCondition::Paren(inner) => inner.clone(),
        other => other,
    }
}

pub(crate) trait UserDefinedCallable {
    fn arguments(&self) -> &ArgumentDeclaration<'static>;
}

impl UserDefinedCallable for AstFunctionDecl<'static> {
    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.arguments
    }
}

impl UserDefinedCallable for Rc<AstFunctionDecl<'static>> {
    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.arguments
    }
}

impl UserDefinedCallable for AstMixin<'static> {
    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.args
    }
}

impl UserDefinedCallable for Rc<CallableContentBlock> {
    fn arguments(&self) -> &ArgumentDeclaration<'static> {
        &self.content.args
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CallableContentBlock {
    // Stored owned rather than as `&'static AstContentBlock<'static>`: doing so
    // would require `visit_include_stmt`'s `include_stmt` parameter (and its
    // whole dispatch chain back through `visit_stmt_ref`) to be typed as a
    // genuinely `'static` reference rather than the anonymous elided lifetime
    // used throughout the visitor, which is out of scope for this pass. The
    // clone here is bounded by the mixin's declared-argument count, not by
    // call/loop-iteration count, unlike the `ArgumentInvocation` clones this
    // plan targets.
    content: AstContentBlock<'static>,
    env: Environment,
}

/// Key for `Visitor::import_cache`. A real filesystem path and an
/// importer-supplied canonical URL string live in separate variants so a
/// `scheme:foo`-style canonical URL can never collide with (and shadow) a
/// real file path that happens to have the same text — see
/// `ImportSource::Resolved`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ImportKey {
    Path(PathBuf),
    Url(String),
}

type ImportPathCacheEntry = (PathBuf, PathBuf, bool, SassResult<Option<ImportSource>>);
type PreModuleComments = FxHashMap<PathBuf, Vec<CssStmt>>;

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
    // avoid emitting duplicate deprecation warnings for the same deprecation + span. This
    // matters in practice: a deprecation site inside a function/mixin body can be evaluated
    // many times (e.g. once per @each iteration). This is the fast-path check in
    // `emit_deprecation` — first-time call sites stop here without ever building a message.
    pub(crate) deprecation_warnings_emitted: FxHashSet<(Deprecation, Span)>,
    // Second-level dedup, matching dart-sass's `_warningsEmitted` key of (message, span):
    // a call site already recorded above that's revisited with a DIFFERENT message (e.g.
    // bogus-combinators' interpolated selector text, or the same source line evaluated with
    // different operands across loop iterations) must still warn again. Only consulted once
    // a (deprecation, span) pair has already fired once, so the common one-shot case above
    // never pays for this.
    pub(crate) deprecation_messages_emitted: FxHashSet<(Span, String)>,
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
    /// Comments collected before module loads in the current Sass module
    /// environment. Nested environments share this map when it already
    /// exists, matching Dart Sass's pre-module comment scope.
    pre_module_comments: Option<Rc<RefCell<PreModuleComments>>>,
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
    import_cache: FxHashMap<ImportKey, Rc<StyleSheet<'static>>>,
    /// Memoized immutable builtin modules for this compilation.
    builtin_module_cache: FxHashMap<&'static str, Module>,
    /// Cache for resolved import paths, bucketed by a hash of (containing URL, requested path,
    /// for_import flag). Each bucket retains the full tuple for collision verification, so the
    /// hit path avoids allocating either PathBuf.
    import_path_cache: FxHashMap<u64, Vec<ImportPathCacheEntry>>,
    /// Cache for canonicalized paths to avoid repeated syscalls.
    canonicalize_cache: FxHashMap<PathBuf, PathBuf>,
    /// Cache of directory listings, used to batch existence probes for many
    /// import candidates sharing the same parent directory into a single
    /// directory read. Wrapped in `RefCell` so it can be populated from the
    /// `&self`-only candidate-resolution helpers. `None` means the directory
    /// couldn't be listed (or the embedder's `Fs` doesn't support batching).
    dir_listing_cache: RefCell<FxHashMap<PathBuf, Option<Rc<crate::fs::DirListing>>>>,
    /// Cache of parsed argument declarations for closure-backed
    /// (`BuiltinFn::Dynamic`) custom functions, keyed by their raw `(...)`
    /// signature text. Parsing happens lazily against this `Visitor`'s own
    /// `arena`/`map` (see `parse_dynamic_signature`), so the cache lives
    /// here rather than on `Builtin`/`Options` — a `Builtin` can outlive
    /// any single compilation (e.g. a reused `Options` across `--watch`
    /// recompiles), but the parsed declaration must not outlive the arena
    /// it borrows from.
    dynamic_signature_cache: FxHashMap<Arc<str>, Rc<ArgumentDeclaration<'static>>>,
    /// Nesting depth of user-defined function/mixin/content-block invocations.
    /// Guards against stack overflow from unbounded recursion (e.g. a
    /// function that calls itself with no terminating `@if`); see
    /// `run_user_defined_callable`.
    recursion_depth: usize,
    /// Nesting depth of plain style-rule bodies (`a { b { c { ... } } }`).
    /// This is a *separate* recursion source from `recursion_depth` above —
    /// callable invocation and style-rule nesting can each contribute depth
    /// independently (a mixin body that nests style rules pays into both) —
    /// see `MAX_STYLE_RULE_RECURSION_DEPTH` and solo todo #196.
    style_rule_recursion_depth: usize,
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

        let mut env = Environment::new();
        if options.source_map {
            env.scopes.enable_span_tracking();
        }

        let extender = ExtensionStore::new(empty_span);

        let current_import_path = path.to_path_buf();

        Self {
            declaration_name: None,
            style_rule_ignoring_at_root: None,
            original_selector: None,
            flags,
            warnings_emitted: FxHashSet::default(),
            deprecation_warnings_emitted: FxHashSet::default(),
            deprecation_messages_emitted: FxHashSet::default(),
            media_queries: None,
            media_query_sources: None,
            env,
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
            pre_module_comments: None,
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
            builtin_module_cache: FxHashMap::default(),
            import_path_cache: FxHashMap::default(),
            canonicalize_cache: FxHashMap::default(),
            dir_listing_cache: RefCell::new(FxHashMap::default()),
            dynamic_signature_cache: FxHashMap::default(),
            recursion_depth: 0,
            style_rule_recursion_depth: 0,
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

    /// Cached directory listing, used to batch existence probes for many
    /// import candidates in the same directory into a single directory read.
    /// Works from `&self` (via `RefCell`) so it can be used inside the
    /// `&self`-only candidate-resolution helpers in `find_import_uncached`.
    fn dir_listing(&self, dir: &Path) -> Option<Rc<crate::fs::DirListing>> {
        if let Some(cached) = self.dir_listing_cache.borrow().get(dir) {
            return cached.clone();
        }
        let listing = self.options.fs.dir_listing(dir).map(Rc::new);
        self.dir_listing_cache
            .borrow_mut()
            .insert(dir.to_path_buf(), listing.clone());
        listing
    }

    /// Like `self.options.fs.is_file(path)`, but consults the cached
    /// directory listing first to avoid a filesystem call when existence (or
    /// absence) can be proven from an already-read directory listing. Falls
    /// back to a direct `is_file` call whenever the listing is unavailable or
    /// ambiguous (symlinks, case-only variants) — see `DirListing::probe_is_file`.
    fn is_file_fast(&self, path: &Path) -> bool {
        let (dir, name) = match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => (dir, name),
            _ => return self.options.fs.is_file(path),
        };
        match self.dir_listing(dir) {
            Some(listing) => listing
                .probe_is_file(name)
                .unwrap_or_else(|| self.options.fs.is_file(path)),
            None if self.dir_known_absent(dir) => false,
            None => self.options.fs.is_file(path),
        }
    }

    /// Like `is_file_fast`, but for directories.
    fn is_dir_fast(&self, path: &Path) -> bool {
        let (dir, name) = match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => (dir, name),
            _ => return self.options.fs.is_dir(path),
        };
        match self.dir_listing(dir) {
            Some(listing) => listing
                .probe_is_dir(name)
                .unwrap_or_else(|| self.options.fs.is_dir(path)),
            None if self.dir_known_absent(dir) => false,
            None => self.options.fs.is_dir(path),
        }
    }

    /// Whether `dir` is *provably* absent from already-cached directory
    /// listings alone: some ancestor's listing shows `dir`'s next path
    /// component doesn't exist in any case variant (the same
    /// definite-absence standard as `DirListing::probe_is_dir`), which makes
    /// every path below it absent too. Used when `dir_listing(dir)` returned
    /// `None`, so candidate probes into directories that don't exist (the
    /// common relative-resolution miss before load-path fallback) don't each
    /// pay a filesystem check. Returns `false` whenever absence can't be
    /// proven (unlistable-but-existing directories, symlinks, case-variant
    /// matches) — callers must then fall back to a direct filesystem check,
    /// exactly preserving prior behavior for those cases.
    fn dir_known_absent(&self, dir: &Path) -> bool {
        let (parent, name) = match (dir.parent(), dir.file_name()) {
            (Some(parent), Some(name)) => (parent, name),
            _ => return false,
        };
        match self.dir_listing(parent) {
            Some(listing) => listing.probe_is_dir(name) == Some(false),
            None => self.dir_known_absent(parent),
        }
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

        for (deprecation, span, message) in &style_sheet.parse_time_warnings {
            self.emit_deprecation(*deprecation, *span, || Ok(message.clone()))?;
        }

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

    /// Breaks the `Rc<RefCell<Module>>` reference cycles inside the
    /// `@use`/`@forward` module graph so it can actually be freed once this
    /// `Visitor` is dropped (solo todo #272).
    ///
    /// The module graph holds strong `Rc` references in every direction:
    /// `Module::Environment.upstream` points from a module to the modules it
    /// `@use`s, while `Environment.global_modules`/`forwarded_modules`/
    /// `modules`/`imported_modules`/`nested_forwarded_modules` point right
    /// back out again (namespaced lookups, `@forward`, `@import` chains).
    /// With no `Weak` references anywhere in this graph, cycles across these
    /// fields (empirically confirmed — see #272 comment #393; no single
    /// field is "the" back-edge, several overlap) mean the whole graph would
    /// otherwise leak for the life of the process, measured at ~20-27 MiB
    /// per compile (~87.5% of that walked away by this pass; the small
    /// residual is tracked separately, not caused by these fields).
    ///
    /// This walks every `Module` reachable from the roots that can hold one
    /// — the per-compile module caches on `Visitor` and the corresponding
    /// fields on `self.env` — and clears the six back-reference fields on
    /// each `Module::Environment` node once all of them have been visited
    /// (nothing downstream of `finish()` reads the module graph again; only
    /// `css_tree.finish()`/`combined_import_section` handling follows).
    ///
    /// `Environment.content` and `ForwardedModule`/`ShadowedModule.inner`
    /// are deliberately left untouched: both were tested in isolation and
    /// contribute nothing to the cycle (content is a genuine forward-owned
    /// `@content` closure; `.inner` is the wrapper's own non-cyclic
    /// ownership of the module it wraps/shadows) — clearing them would only
    /// add risk (`.inner` in particular backs `Module::scope()`) for zero
    /// measured benefit.
    fn teardown_module_graph(&mut self) {
        let mut visited: FxHashSet<*const RefCell<Module>> = FxHashSet::default();
        let mut stack: Vec<Rc<RefCell<Module>>> = Vec::new();
        // DIAG(#278/#279): dedup sets so a scope-map shared by `new_closure()`
        // (module env + every closure it spawned) is only mutated once.
        let mut seen_fn_maps: FxHashSet<*const RefCell<FxHashMap<Identifier, SassFunction>>> =
            FxHashSet::default();
        let mut seen_mixin_maps: FxHashSet<*const RefCell<FxHashMap<Identifier, Mixin>>> =
            FxHashSet::default();

        stack.extend(self.modules.values().cloned());
        stack.extend(self.import_cloned_modules.values().cloned());
        stack.extend(self.upstream_modules.iter().cloned());
        stack.extend(self.env.modules.borrow().0.values().cloned());
        stack.extend(self.env.global_modules.iter().cloned());
        stack.extend(self.env.forwarded_modules.borrow().iter().cloned());
        stack.extend(self.env.imported_modules.borrow().iter().cloned());
        if let Some(nested) = &self.env.nested_forwarded_modules {
            for inner in nested.borrow().iter() {
                stack.extend(inner.borrow().iter().cloned());
            }
        }

        while let Some(module_rc) = stack.pop() {
            if !visited.insert(Rc::as_ptr(&module_rc)) {
                continue;
            }

            let mut module = module_rc.borrow_mut();

            match &mut *module {
                Module::Environment { upstream, env, .. } => {
                    stack.extend(upstream.iter().cloned());
                    stack.extend(env.global_modules.iter().cloned());
                    stack.extend(env.forwarded_modules.borrow().iter().cloned());
                    stack.extend(env.modules.borrow().0.values().cloned());
                    stack.extend(env.imported_modules.borrow().iter().cloned());
                    if let Some(nested) = &env.nested_forwarded_modules {
                        for inner in nested.borrow().iter() {
                            stack.extend(inner.borrow().iter().cloned());
                        }
                    }

                    // DIAG(#278/#279 causal probe): `SassFunction::UserDefined`/
                    // `Mixin::UserDefined` closures share an `Rc<Environment>`
                    // captured via `Environment::new_closure()` at declaration
                    // time. `new_closure()` element-wise `Rc::clone`s
                    // `global_modules` into a brand-new, private `Vec` (unlike
                    // `modules`/`forwarded_modules`/`imported_modules`, which
                    // share the same `Rc<RefCell<..>>` as the declaring env and
                    // so get cleared above "for free"). This walk never
                    // followed `env.scopes.functions`/`.mixins` before, so
                    // every closure's private `global_modules` snapshot (and
                    // any nested-forwarded-modules copy) survived teardown
                    // untouched, keeping the modules it points at alive.
                    for map in env.scopes.functions_mut().iter() {
                        let ptr = Rc::as_ptr(map);
                        if !seen_fn_maps.insert(ptr) {
                            continue;
                        }
                        for value in map.borrow_mut().values_mut() {
                            if let SassFunction::UserDefined(udf) = value {
                                if let Some(closure_env) = Rc::get_mut(&mut udf.env) {
                                    stack.append(&mut closure_env.global_modules);
                                    stack.extend(
                                        closure_env.forwarded_modules.borrow().iter().cloned(),
                                    );
                                    stack.extend(closure_env.modules.borrow().0.values().cloned());
                                    stack.extend(
                                        closure_env.imported_modules.borrow().iter().cloned(),
                                    );
                                    if let Some(nested) = &closure_env.nested_forwarded_modules {
                                        for inner in nested.borrow().iter() {
                                            stack.extend(inner.borrow().iter().cloned());
                                        }
                                    }
                                } else {
                                    // A Value can retain a second reference to this environment.
                                    // Shared environments cannot be drained or cleared here without
                                    // mutating that other owner, so retain the old behavior: push
                                    // cloned module refs and leave the shared fields untouched.
                                    stack.extend(udf.env.global_modules.iter().cloned());
                                    stack
                                        .extend(udf.env.forwarded_modules.borrow().iter().cloned());
                                    stack.extend(udf.env.modules.borrow().0.values().cloned());
                                    stack.extend(udf.env.imported_modules.borrow().iter().cloned());
                                    if let Some(nested) = &udf.env.nested_forwarded_modules {
                                        for inner in nested.borrow().iter() {
                                            stack.extend(inner.borrow().iter().cloned());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    for map in env.scopes.mixins_mut().iter() {
                        let ptr = Rc::as_ptr(map);
                        if !seen_mixin_maps.insert(ptr) {
                            continue;
                        }
                        for value in map.borrow_mut().values_mut() {
                            if let Mixin::UserDefined(_, closure_env, _) = value {
                                if let Some(closure_env) = Rc::get_mut(closure_env) {
                                    stack.append(&mut closure_env.global_modules);
                                    stack.extend(
                                        closure_env.forwarded_modules.borrow().iter().cloned(),
                                    );
                                    stack.extend(closure_env.modules.borrow().0.values().cloned());
                                    stack.extend(
                                        closure_env.imported_modules.borrow().iter().cloned(),
                                    );
                                    if let Some(nested) = &closure_env.nested_forwarded_modules {
                                        for inner in nested.borrow().iter() {
                                            stack.extend(inner.borrow().iter().cloned());
                                        }
                                    }
                                } else {
                                    // See the function case above: clone refs from a shared
                                    // environment and do not mutate or clear its fields.
                                    stack.extend(closure_env.global_modules.iter().cloned());
                                    stack.extend(
                                        closure_env.forwarded_modules.borrow().iter().cloned(),
                                    );
                                    stack.extend(closure_env.modules.borrow().0.values().cloned());
                                    stack.extend(
                                        closure_env.imported_modules.borrow().iter().cloned(),
                                    );
                                    if let Some(nested) = &closure_env.nested_forwarded_modules {
                                        for inner in nested.borrow().iter() {
                                            stack.extend(inner.borrow().iter().cloned());
                                        }
                                    }
                                }
                            }
                        }
                    }

                    upstream.clear();
                    env.global_modules.clear();
                    env.forwarded_modules.borrow_mut().clear();
                    env.modules.borrow_mut().0.clear();
                    env.imported_modules.borrow_mut().clear();
                    env.nested_forwarded_modules = None;
                }
                Module::Forwarded(forwarded) => stack.push(Rc::clone(&forwarded.inner)),
                Module::Shadowed(shadowed) => stack.push(Rc::clone(&shadowed.inner)),
                Module::Builtin { .. } => {}
            }
        }
    }

    /// The full set of files loaded during this compile via `@use`/
    /// `@forward` (`self.modules`) and `@import` (`self.import_cache`) --
    /// independent of whether any of those files contributed an emitted CSS
    /// mapping. Unlike
    /// `SourceMapData::sources`, this includes `@use`d partials containing
    /// only variables/mixins/functions, which never produce a mapping.
    /// Deduplicated and sorted for a deterministic return order (none of
    /// the backing maps/sets iterate deterministically).
    pub(crate) fn loaded_files(&self) -> Vec<PathBuf> {
        let mut files: FxHashSet<PathBuf> = FxHashSet::default();
        files.extend(self.modules.keys().cloned());
        // `ImportKey::Url` entries are importer-supplied canonical URLs, not
        // real files on disk — they have no filesystem path to report here.
        files.extend(self.import_cache.keys().filter_map(|key| match key {
            ImportKey::Path(p) => Some(p.clone()),
            ImportKey::Url(_) => None,
        }));
        let mut files: Vec<PathBuf> = files.into_iter().collect();
        files.sort_unstable();
        files
    }

    pub(crate) fn finish(mut self) -> SassResult<Vec<CssStmt>> {
        self.flush_pending_imports(true);
        self.extend_modules()?;
        self.teardown_module_graph();
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
            } else if end_of_module && idx == 0 && self.module_depth == 0 {
                // Root had only comments, no imports — keep them in combined
                // so they remain ahead of module CSS in the import section.
                self.combined_import_section.push(item);
            } else {
                // A nested module's comment-only import section is CSS emitted
                // at this module's load position and must remain cloneable.
                if !end_of_module && self.module_depth == 0 {
                    self.import_section_tree_count += 1;
                }
                self.css_tree.add_stmt(item, None);
            }
        }
    }

    /// Emit comments associated with a module load at the current traversal
    /// position. Root comments remain in the combined import section; nested
    /// comments belong in the CSS tree beside the module edge.
    fn emit_pre_module_comments(&mut self, comments: &[CssStmt]) {
        for comment in comments {
            if self.module_depth == 0 {
                self.combined_import_section.push(comment.clone());
            } else {
                self.css_tree.add_stmt(comment.clone(), None);
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

        // Only clone CSS indices that haven't been cloned yet in this @import context.
        // Comment-only modules still need a clone even when no selector is
        // present to populate `import_selector_map`.
        let mut cloned_any = false;
        for idx in &all_css_indices {
            if !self.import_cloned_css.contains(idx) {
                self.css_tree
                    .clone_subtree(*idx, CssTree::ROOT, &mut self.import_selector_map);
                self.import_cloned_css.insert(*idx);
                cloned_any = true;
            }
        }

        if !cloned_any {
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
        let cloned = if let Module::Environment {
            extension_store, ..
        } = &*m
        {
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
                    upstream.iter().map(Rc::as_ptr).collect::<Vec<_>>()
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
                    "The target selector was not found.\nUse \"@extend {target_str} !optional\" to avoid this error."
                ),
                ext.span,
            )
                .into());
        }

        Ok(())
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
                Ok(Some(self.without_slash(val, || ret.span)?))
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
                let value = self.visit_expr_ref(&err.value)?.inspect(err.span)?;
                Err((value, err.span).into())
            }
            // Each/Media/Include/ContentRule delegate to `_ref`/borrowing visitors
            // above and no longer clone at dispatch. The remaining variants below
            // still clone and delegate to an owned visitor; that clone is NOT
            // always cheap — RuleSet/UnknownAtRule/Extend/AtRootRule clone an
            // owned `Interpolation`, For clones owned `AstExpr` bounds, and
            // FunctionDecl/Mixin/Use/Forward clone owned argument/config Vecs.
            // Converting those (declaration sites, not call sites) is out of
            // scope for this pass (see Plan 022).
            AstStmt::RuleSet(ruleset) => self.visit_ruleset(ruleset.clone()),
            AstStmt::For(for_stmt) => self.visit_for_stmt((*for_stmt).clone()),
            AstStmt::Each(each_stmt) => self.visit_each_stmt(each_stmt),
            AstStmt::Media(media_rule) => self.visit_media_rule(media_rule),
            AstStmt::Include(include_stmt) => self.visit_include_stmt(include_stmt),
            AstStmt::While(while_stmt) => self.visit_while_stmt(while_stmt),
            AstStmt::FunctionDecl(func) => {
                self.visit_function_decl(func.clone());
                Ok(None)
            }
            AstStmt::Mixin(mixin) => {
                self.visit_mixin_decl(mixin.clone());
                Ok(None)
            }
            AstStmt::ContentRule(content_rule) => self.visit_content_rule(content_rule),
            AstStmt::UnknownAtRule(unknown_at_rule) => {
                self.visit_unknown_at_rule((*unknown_at_rule).clone())
            }
            AstStmt::Extend(extend_rule) => self.visit_extend_rule(extend_rule.clone()),
            AstStmt::AtRootRule(at_root_rule) => self.visit_at_root_rule(at_root_rule.clone()),
            AstStmt::ImportRule(import_rule) => self.visit_import_rule(import_rule.clone()),
            AstStmt::Use(use_rule) => {
                self.visit_use_rule((*use_rule).clone())?;
                Ok(None)
            }
            AstStmt::Forward(forward_rule) => {
                self.visit_forward_rule((*forward_rule).clone())?;
                Ok(None)
            }
            AstStmt::Supports(supports_rule) => {
                self.visit_supports_rule((*supports_rule).clone())?;
                Ok(None)
            }
        }
    }

    /// Reference-based variable declaration visitor.
    fn visit_variable_decl_ref(
        &mut self,
        decl: &AstVariableDecl<'static>,
    ) -> SassResult<Option<Value>> {
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
                    let var_override = var_override.unwrap();
                    // dart stores the configured expression's node, so the
                    // provenance segment points into the `with (...)` clause.
                    self.env.insert_var_recording_span(
                        name,
                        None,
                        var_override.value,
                        true,
                        self.flags.in_semi_global_scope(),
                        var_override.assignment_span,
                    )?;
                    return Ok(None);
                }
            }

            if let Some(value) = self.env.try_get_var(name, decl.namespace)? {
                if value != Value::Null {
                    return Ok(None);
                }
            }
        }

        self.maybe_warn_new_global(decl.name, decl.namespace, decl.is_global, decl.span)?;

        let value = self.visit_expr_ref(&decl.value.node)?;
        let value = self.without_slash(value, || decl.span)?;

        if self.options.source_map {
            // Computed before the insert: a self-referencing `$v: $v ...`
            // chain-collapse has to see the OLD stored span.
            let decl_span = self.provenance_span(&decl.value.node, decl.value.span);
            self.env.insert_var_recording_span(
                name,
                decl.namespace,
                value,
                decl.is_global,
                self.flags.in_semi_global_scope(),
                decl_span,
            )?;
        } else {
            self.env.insert_var(
                name,
                decl.namespace,
                value,
                decl.is_global,
                self.flags.in_semi_global_scope(),
            )?;
        }

        Ok(None)
    }

    /// The span dart-sass would store as this expression's "node" when
    /// binding a variable (`Environment.setVariable(..., expressionNode)`):
    /// for a bare variable reference, the referenced variable's own stored
    /// declaration span (collapsing chains like `$w: $v` at assignment time,
    /// verified against dart 1.101.0), otherwise the expression's own span.
    /// `None` whenever source maps are off — the maps-off cost is this one
    /// branch.
    #[inline]
    fn provenance_span(&self, expr: &AstExpr<'static>, expr_span: Span) -> Option<Span> {
        if !self.options.source_map {
            return None;
        }

        Some(match expr {
            AstExpr::Variable { name, namespace } => self
                .env
                .get_var_span(*name, *namespace)
                .unwrap_or(expr_span),
            _ => expr_span,
        })
    }

    /// Reference-based loud comment visitor.
    fn visit_loud_comment_ref(
        &mut self,
        comment: &AstLoudComment<'static>,
    ) -> SassResult<Option<Value>> {
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
        for clause in if_stmt.if_clauses {
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

        let mut name = self.perform_interpolation_ref(&style.name, true)?;

        if let Some(declaration_name) = &self.declaration_name {
            name = format!("{declaration_name}-{name}");
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
                // dart maps every declaration value (`valueSpanForMap`); the
                // same-line dedup in `record_mapping` is what keeps literal/
                // arithmetic values invisible. Only a bare `$var` value pulls
                // in the variable's stored declaration span.
                let value_span_for_map = if self.options.source_map {
                    style
                        .value
                        .as_ref()
                        .and_then(|s| self.provenance_span(&s.node, s.span))
                } else {
                    None
                };
                self.add_child_to_current_parent(CssStmt::Style(Style {
                    property: InternedString::get_or_intern(&name),
                    value: Box::new(value),
                    declared_as_custom_property: is_custom_property,
                    property_span: style.span,
                    value_span_for_map,
                }));
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
                forward_rule.url,
                Some(Rc::clone(&new_configuration)),
                false,
                forward_rule.span,
                |visitor, module, _| {
                    visitor
                        .env
                        .forward_module(Rc::clone(&module), forward_rule.clone())?;
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
            let url = forward_rule.url;
            self.load_module(
                url,
                None,
                false,
                forward_rule.span,
                move |visitor, module, _| {
                    visitor
                        .env
                        .forward_module(Rc::clone(&module), forward_rule.clone())?;
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

        for variable in forward_rule.configuration {
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

            let value = self.visit_expr_ref(&variable.expr.node)?;
            let value = self.without_slash(value, || variable.expr.span)?;

            let assignment_span = self.provenance_span(&variable.expr.node, variable.expr.span);
            new_values.insert(
                variable.name.node,
                ConfiguredValue::explicit(value, variable.expr.span, assignment_span),
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

    fn visit_supports_condition(
        &mut self,
        condition: AstSupportsCondition<'static>,
    ) -> SassResult<String> {
        self.visit_supports_condition_ref(&condition)
    }

    fn visit_supports_condition_ref(
        &mut self,
        condition: &AstSupportsCondition<'static>,
    ) -> SassResult<String> {
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
                self.evaluate_to_css(expr, QuoteKind::None, self.empty_span)
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
                    self.evaluate_to_css(name, QuoteKind::Quoted, self.empty_span)?,
                    if is_custom_property { "" } else { " " },
                    self.evaluate_to_css(value, QuoteKind::Quoted, self.empty_span)?,
                );

                self.flags
                    .set(ContextFlags::IN_SUPPORTS_DECLARATION, old_in_supports_decl);

                Ok(result)
            }
            AstSupportsCondition::Function { name, args } => Ok(format!(
                "{}({})",
                self.perform_interpolation_ref(name, false)?,
                self.perform_interpolation_ref(args, false)?
            )),
            AstSupportsCondition::Anything { contents } => Ok(format!(
                "({})",
                self.perform_interpolation_ref(contents, false)?,
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

        let at_rule_span = supports_rule.at_rule_span;
        let condition = self.visit_supports_condition(supports_rule.condition)?;

        let css_supports_rule = CssStmt::Supports(
            SupportsRule {
                params: condition,
                body: Vec::new(),
                at_rule_span: Some(at_rule_span),
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
        stylesheet: Rc<StyleSheet<'static>>,
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
                    .is_some_and(|existing| {
                        let existing_orig = Configuration::original_config(Rc::clone(existing));
                        let current_orig =
                            Configuration::original_config(Rc::clone(&current_configuration));
                        Rc::ptr_eq(&existing_orig, &current_orig)
                    });

                if !same_original {
                    // Check if module has !default vars matching the config keys
                    let config_keys: FxHashSet<Identifier> = current_configuration
                        .borrow()
                        .values
                        .keys()
                        .into_iter()
                        .collect();
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
                            current_configuration
                                .borrow()
                                .span
                                .unwrap_or(self.empty_span),
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
                let (cloned_module, has_clones) =
                    self.clone_module_for_import(&url, &already_loaded);
                if has_clones {
                    return Ok(cloned_module);
                }
            }

            return Ok(already_loaded);
        }

        let mut env = Environment::new();
        if self.options.source_map {
            env.scopes.enable_span_tracking();
        }

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
            let old_pre_module_comments = visitor.pre_module_comments.clone();
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
            visitor
                .module_css_indices
                .insert(url.clone(), new_css_indices.clone());

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
                                let resolved = old_list
                                    .resolve_parent_selectors(Some(parent_list.clone()), true)?;
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
            visitor.pre_module_comments = old_pre_module_comments;
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

        self.module_ptr_to_url
            .insert(Rc::as_ptr(&module) as usize, url.clone());
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
        stylesheet: Rc<StyleSheet<'static>>,
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

        let module = self.execute(Rc::clone(&stylesheet), configuration.clone(), true)?;

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
                let parent_list = old_style_rule.as_ref().unwrap().as_selector_list().clone();
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
                        let resolved =
                            old_list.resolve_parent_selectors(Some(parent_list.clone()), true)?;
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
    fn collect_transitive_css_indices(&self, module: &Rc<RefCell<Module>>) -> Vec<CssTreeIdx> {
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
                        if !self.css_tree.is_hidden(idx) && !original_to_hidden.contains_key(&idx) {
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
                Module::Environment {
                    extension_store, ..
                } => {
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
        for new_selector in selector_map.values() {
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
        callback: impl Fn(&mut Self, Rc<RefCell<Module>>, Rc<StyleSheet<'static>>) -> SassResult<()>,
    ) -> SassResult<()> {
        let builtin_name = match url.to_string_lossy().as_ref() {
            "sass:color" => Some("sass:color"),
            "sass:list" => Some("sass:list"),
            "sass:map" => Some("sass:map"),
            "sass:math" => Some("sass:math"),
            "sass:meta" => Some("sass:meta"),
            "sass:selector" => Some("sass:selector"),
            "sass:string" => Some("sass:string"),
            _ => None,
        };

        if let Some(builtin_name) = builtin_name {
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

                    return Err((msg, (**configuration).borrow().span.unwrap()).into());
                }
            }

            let builtin = self
                .builtin_module_cache
                .entry(builtin_name)
                .or_insert_with(|| match builtin_name {
                    "sass:color" => declare_module_color(),
                    "sass:list" => declare_module_list(),
                    "sass:map" => declare_module_map(),
                    "sass:math" => declare_module_math(),
                    "sass:meta" => declare_module_meta(),
                    "sass:selector" => declare_module_selector(),
                    "sass:string" => declare_module_string(),
                    _ => unreachable!("builtin name was validated above"),
                })
                .clone();

            callback(
                self,
                Rc::new(RefCell::new(builtin)),
                Rc::new(StyleSheet::new(false, url.to_path_buf())),
            )?;
            return Ok(());
        }

        // todo: decide on naming convention for style_sheet vs stylesheet
        let stylesheet = self.load_style_sheet(url.to_string_lossy().as_ref(), false, span)?;

        let canonical_url = self.canonicalize(&stylesheet.url);

        if self.active_modules.contains(&canonical_url) {
            return Err(("Module loop: this module is already being loaded.", span).into());
        }

        let first_load = !self.modules.contains_key(&canonical_url);
        let mut pre_module_comments = Vec::new();
        self.active_modules.insert(canonical_url.clone());

        // Preserve comments before a nested module load at that module's
        // traversal position. Root-level comments remain in the combined
        // import section so they stay ahead of all module CSS.
        if first_load {
            let pending = mem::take(&mut self.pending_import_items);
            let mut remaining = Vec::with_capacity(pending.len());
            for item in pending {
                if matches!(item, CssStmt::Comment(..)) {
                    pre_module_comments.push(item);
                } else {
                    remaining.push(item);
                }
            }
            self.pending_import_items = remaining;

            // Imports stay in the current module's pending section. Comments
            // are associated with the first loaded module and emitted after
            // evaluation once we know whether that module contributes CSS.
            if !self.in_import_context {
                self.emit_pre_module_comments(&pre_module_comments);
            }
        } else if self.modules.contains_key(&canonical_url) {
            let comments = self
                .pre_module_comments
                .as_ref()
                .and_then(|comments| comments.borrow().get(&canonical_url).cloned());
            if let Some(comments) = comments {
                self.emit_pre_module_comments(&comments);
            }
        }

        let module = self.execute(Rc::clone(&stylesheet), configuration, names_in_errors)?;

        self.active_modules.remove(&canonical_url);

        if first_load && !pre_module_comments.is_empty() {
            let module_has_css = !self.collect_transitive_css_indices(&module).is_empty();
            if module_has_css {
                let comments = self
                    .pre_module_comments
                    .get_or_insert_with(|| Rc::new(RefCell::new(FxHashMap::default())))
                    .clone();
                comments
                    .borrow_mut()
                    .entry(canonical_url.clone())
                    .or_default()
                    .extend(pre_module_comments.clone());
            }
        }

        callback(self, module, stylesheet)?;

        Ok(())
    }

    fn visit_use_rule(&mut self, use_rule: AstUseRule<'static>) -> SassResult<()> {
        let configuration = if use_rule.configuration.is_empty() {
            Rc::new(RefCell::new(Configuration::empty()))
        } else {
            let mut values = FxHashMap::default();

            for var in use_rule.configuration {
                let value = self.visit_expr_ref(&var.expr.node)?;
                let value = self.without_slash(value, || var.expr.span)?;
                let assignment_span = self.provenance_span(&var.expr.node, var.expr.span);
                values.insert(
                    var.name.node,
                    ConfiguredValue::explicit(
                        value,
                        var.name.span.merge(var.expr.span),
                        assignment_span,
                    ),
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
            use_rule.url,
            Some(Rc::clone(&configuration)),
            false,
            span,
            |visitor, module, _| {
                visitor
                    .env
                    .add_module(namespace, Rc::clone(&module), span)?;
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
            format!("${name} was not declared with !default in the @used module.")
        } else {
            "This variable was not declared with !default in the @used module.".to_owned()
        };

        Err((msg, span).into())
    }

    fn visit_import_rule(
        &mut self,
        import_rule: AstImportRule<'static>,
    ) -> SassResult<Option<Value>> {
        for import in import_rule.imports {
            match import {
                AstImport::Sass(dynamic_import) => {
                    self.visit_dynamic_import_rule(dynamic_import)?;
                }
                AstImport::Plain(static_import) => {
                    self.visit_static_import_rule(static_import.clone())?
                }
            }
        }

        Ok(None)
    }

    /// Walks `self.options.importers` in registration order, returning the
    /// first result other than [`ImportResolution::NotFound`] (or `None` if
    /// every importer declined, or none are registered).
    fn resolve_via_importers(
        &self,
        path: &Path,
        for_import: bool,
        span: Span,
    ) -> SassResult<Option<ImportResolution>> {
        let url = path.to_str().unwrap_or_default();
        let containing_url = self.current_import_path.to_str();

        for importer in &self.options.importers {
            match importer.canonicalize(url, for_import, containing_url, span)? {
                ImportResolution::NotFound => continue,
                other => return Ok(Some(other)),
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
    ) -> SassResult<Option<ImportSource>> {
        // Cache key must include the full containing URL because a custom importer can resolve
        // the same requested path differently from two files in the same directory. Hashing the
        // borrowed inputs keeps the hit path allocation-free; the full tuple in each bucket
        // verifies hash collisions without changing those semantics.
        let mut hasher = FxHasher::default();
        self.current_import_path.hash(&mut hasher);
        path.hash(&mut hasher);
        for_import.hash(&mut hasher);
        let cache_hash = hasher.finish();
        if let Some(entries) = self.import_path_cache.get(&cache_hash) {
            for (cached_containing_url, cached_path, cached_for_import, result) in entries {
                if cached_containing_url == &self.current_import_path
                    && cached_path == path
                    && *cached_for_import == for_import
                {
                    return result.clone();
                }
            }
        }

        let result = self.find_import_uncached(path, for_import, span);
        self.import_path_cache.entry(cache_hash).or_default().push((
            self.current_import_path.clone(),
            path.to_path_buf(),
            for_import,
            result.clone(),
        ));
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
                    match result.components().next_back() {
                        // There's a real segment to cancel against — pop it.
                        Some(Component::Normal(_)) => {
                            result.pop();
                        }
                        // `..` above the root is a no-op; it stays clamped there
                        // rather than growing an invalid `/../..` path.
                        Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                        // Nothing to cancel against yet (empty result, or the
                        // last component is itself an unresolved `..`) — the
                        // `..` must accumulate, not silently vanish.
                        _ => result.push(component),
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
    ) -> SassResult<Option<ImportSource>> {
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

        // Build candidates for an explicit non-CSS extension. Unlike the
        // general path candidates above, partials take priority within each
        // group so conflicts are reported in Sass's order.
        fn explicit_extension_candidates(path: PathBuf) -> Vec<PathBuf> {
            let dirname = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
            let basename = path.file_name().unwrap_or_else(|| OsStr::new(".."));
            let partial = dirname.join(format!("_{}", basename.to_str().unwrap()));
            vec![partial, path]
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
                let sass_basename = sass_import.file_name().unwrap_or_else(|| OsStr::new(".."));
                let scss_basename = scss_import.file_name().unwrap_or_else(|| OsStr::new(".."));
                import_candidates
                    .push(dirname.join(format!("_{}", sass_basename.to_str().unwrap())));
                import_candidates.push(sass_import);
                import_candidates
                    .push(dirname.join(format!("_{}", scss_basename.to_str().unwrap())));
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
            regular_candidates.push(dirname.join(format!("_{}", sass_basename.to_str().unwrap())));
            regular_candidates.push(sass_path);
            regular_candidates.push(dirname.join(format!("_{}", scss_basename.to_str().unwrap())));
            regular_candidates.push(scss_path);

            (import_candidates, regular_candidates)
        }

        // Check for load conflicts among candidates in a directory.
        // Returns an error if multiple files match, otherwise returns
        // the first match (or None).
        let check_conflicts = |candidates: &[PathBuf],
                               context_dir: &Path,
                               span: Span|
         -> SassResult<Option<PathBuf>> {
            let existing: Vec<&PathBuf> =
                candidates.iter().filter(|p| self.is_file_fast(p)).collect();

            if existing.len() > 1 {
                let mut msg = "It's not clear which file to import. Found:\n".to_string();
                for p in &existing {
                    let rel = p.strip_prefix(context_dir).unwrap_or(p);
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

        // Custom importers (`Options::add_importer`) are checked ahead of
        // the default filesystem/load-path resolution below, in
        // registration order; the first one to return other than
        // `NotFound` wins. This branch costs a single `Vec::is_empty()`
        // check when no importers are registered.
        if !self.options.importers.is_empty() {
            if let Some(resolution) = self.resolve_via_importers(path, for_import, span)? {
                return match resolution {
                    ImportResolution::DelegateToPath(delegate_path) => {
                        // A `FileImporter`-style result: treat it exactly
                        // like a load path (partials/extensions/index
                        // resolution on top), not like the current file's
                        // own directory (which additionally fast-paths an
                        // explicit .scss/.sass/.css extension below) —
                        // matching the JS contract's "applies the normal
                        // partial/extension/index-file resolution on top,
                        // exactly like a load path".
                        let delegate_path = Self::normalize_path(&delegate_path);
                        if let Some(found) =
                            resolve_with_conflicts(&delegate_path, for_import, context_dir, span)?
                        {
                            return Ok(Some(ImportSource::Path(found)));
                        }
                        if self.is_dir_fast(&delegate_path) {
                            if let Some(found) = resolve_with_conflicts(
                                &delegate_path.join("index"),
                                for_import,
                                context_dir,
                                span,
                            )? {
                                return Ok(Some(ImportSource::Path(found)));
                            }
                        }
                        Ok(None)
                    }
                    ImportResolution::Resolved {
                        canonical_url,
                        contents,
                        syntax,
                    } => {
                        // A full `Importer` (canonicalize+load) result:
                        // bypasses `Fs`/path-based parsing entirely.
                        // `import_like_node` parses `contents` directly
                        // under `syntax` and caches the resulting
                        // stylesheet under `canonical_url` (see
                        // `ImportKey::Url`) rather than a filesystem path.
                        Ok(Some(ImportSource::Resolved {
                            canonical_url,
                            contents,
                            syntax,
                        }))
                    }
                    ImportResolution::NotFound => {
                        unreachable!("resolve_via_importers filters out NotFound")
                    }
                };
            }
        }

        if path_buf.extension() == Some(OsStr::new("scss"))
            || path_buf.extension() == Some(OsStr::new("sass"))
            || path_buf.extension() == Some(OsStr::new("css"))
        {
            let extension = path_buf.extension().unwrap();
            if extension == OsStr::new("scss") || extension == OsStr::new("sass") {
                if for_import {
                    let import_candidates = explicit_extension_candidates(
                        path_buf.with_extension(format!("import.{}", extension.to_str().unwrap())),
                    );
                    if let Some(found) = check_conflicts(&import_candidates, context_dir, span)? {
                        return Ok(Some(ImportSource::Path(found)));
                    }
                }

                let regular_candidates = explicit_extension_candidates(path_buf.clone());
                if let Some(found) = check_conflicts(&regular_candidates, context_dir, span)? {
                    return Ok(Some(ImportSource::Path(found)));
                }

                return Ok(None);
            }

            let mut candidates = Vec::new();
            if for_import {
                candidates.extend(path_candidates(
                    path_buf.with_extension(format!("import.{}", extension.to_str().unwrap())),
                ));
            }
            candidates.extend(path_candidates(path_buf));
            return Ok(self
                .options
                .fs
                .resolve_first_existing(&candidates)
                .map(ImportSource::Path));
        }

        // Check base path with conflict detection
        if let Some(found) = resolve_with_conflicts(&path_buf, for_import, context_dir, span)? {
            return Ok(Some(ImportSource::Path(found)));
        }

        // Also check index files
        if self.is_dir_fast(&path_buf) {
            if let Some(found) =
                resolve_with_conflicts(&path_buf.join("index"), for_import, context_dir, span)?
            {
                return Ok(Some(ImportSource::Path(found)));
            }
        }

        // Check load paths
        for load_path in &self.options.load_paths {
            let lp_buf = Self::normalize_path(&load_path.join(path));

            if let Some(found) = resolve_with_conflicts(&lp_buf, for_import, context_dir, span)? {
                return Ok(Some(ImportSource::Path(found)));
            }

            if self.is_dir_fast(&lp_buf) {
                if let Some(found) =
                    resolve_with_conflicts(&lp_buf.join("index"), for_import, context_dir, span)?
                {
                    return Ok(Some(ImportSource::Path(found)));
                }
            }
        }

        Ok(None)
    }

    /// Parses `path`, whose canonical form the caller has already computed
    /// (`import_like_node` canonicalizes every resolved import for its cache
    /// key), so the parser doesn't re-derive it with a second
    /// `fs.canonicalize` walk.
    fn parse_file(
        &mut self,
        lexer: Lexer,
        path: &Path,
        canonical_url: PathBuf,
        empty_span: Span,
    ) -> SassResult<StyleSheet<'static>> {
        self.parse_file_with_syntax(
            lexer,
            path,
            canonical_url,
            empty_span,
            InputSyntax::for_path(path),
        )
    }

    /// Like `parse_file`, but takes an explicit `syntax` instead of
    /// inferring one from `path`'s extension — used by `import_like_node`'s
    /// `ImportSource::Resolved` arm, where `path` is a synthetic
    /// (non-filesystem) canonical URL with no meaningful extension.
    fn parse_file_with_syntax(
        &mut self,
        lexer: Lexer,
        path: &Path,
        canonical_url: PathBuf,
        empty_span: Span,
        syntax: InputSyntax,
    ) -> SassResult<StyleSheet<'static>> {
        let canonical_url = Some(canonical_url);
        let result = match syntax {
            InputSyntax::Scss => ScssParser::new(lexer, self.options, empty_span, path, self.arena)
                .__parse(canonical_url),
            InputSyntax::Sass => SassParser::new(lexer, self.options, empty_span, path, self.arena)
                .__parse(canonical_url),
            InputSyntax::Css => CssParser::new(lexer, self.options, empty_span, path, self.arena)
                .__parse(canonical_url),
        }?;
        // Safety: the arena lives for the entire compilation (stored in Visitor).
        // INVARIANT: the erased-'static StyleSheet must not outlive the Visitor's arena.
        Ok(unsafe { crate::ast::erase_stylesheet_lifetime(result) })
    }

    /// Parses (and caches) the `(...)` argument-declaration text of a
    /// closure-backed [`BuiltinFn::Dynamic`] custom function's signature,
    /// using the *current compilation's own* `self.arena`/`self.map`
    /// rather than a fresh throwaway `CodeMap`. This is load-bearing, not
    /// style: `codemap`'s `Span`s are only unique within the `CodeMap`
    /// that minted them (`CodeMap::add_file` starts at `end_pos()+1`; a
    /// fresh `CodeMap::new()` starts at 0), and `CodeMap::find_file` panics
    /// on a miss. Parsing eagerly against a synthetic one-off `CodeMap` at
    /// `Options`-build time would embed spans (in default-value
    /// expressions) that alias onto the wrong location or panic when later
    /// looked up against the real compile's `self.map`.
    fn parse_dynamic_signature(
        &mut self,
        signature: &Arc<str>,
    ) -> SassResult<Rc<ArgumentDeclaration<'static>>> {
        if let Some(cached) = self.dynamic_signature_cache.get(signature) {
            return Ok(Rc::clone(cached));
        }

        let file = self
            .map
            .add_file("<custom-fn-signature>".to_string(), signature.to_string());
        let empty_span = file.span.subspan(0, 0);
        let lexer = Lexer::new_from_file(&file);
        let path = Path::new("<custom-fn-signature>");

        let declaration = ScssParser::new(lexer, self.options, empty_span, path, self.arena)
            .parse_argument_declaration()?;

        // Safety: mirrors `parse_file`'s use of `erase_stylesheet_lifetime` —
        // the arena lives for the entire compilation (stored in Visitor), and
        // this cache lives on the Visitor too, so it cannot outlive the arena.
        let declaration =
            Rc::new(unsafe { crate::ast::erase_argument_declaration_lifetime(declaration) });

        self.dynamic_signature_cache
            .insert(Arc::clone(signature), Rc::clone(&declaration));

        Ok(declaration)
    }

    /// Binds an evaluated (but unbound) [`ArgumentResult`] to `signature`'s
    /// declared parameters — positional fill → named fill of remaining
    /// declared slots → missing args fall back to declared defaults
    /// (evaluated with earlier-bound args visible as `$name` variables, so
    /// e.g. `"scale($a, $b: $a)"`-style sibling-referencing defaults work)
    /// → trailing `$rest...` collected into a `Value::ArgList`. Mirrors
    /// `run_user_defined_callable_inner`'s algorithm, but returns a
    /// declaration-ordered `ArgumentResult` (rest appended last) instead of
    /// inserting into a persistent callable scope, since
    /// `BuiltinFn::Dynamic` closures are plain Rust with no `$name`
    /// variables to bind into.
    ///
    /// Known accepted gap: unlike a real `@function` call, no
    /// unused-named-arguments-become-an-error check runs after the closure
    /// returns.
    fn bind_dynamic_args(
        &mut self,
        signature: Option<&Arc<str>>,
        mut evaluated: ArgumentResult,
        span: Span,
    ) -> SassResult<ArgumentResult> {
        let declaration = match signature {
            Some(signature) => self.parse_dynamic_signature(signature)?,
            None => return Ok(evaluated),
        };

        declaration.verify(evaluated.positional.len(), &evaluated.named, evaluated.span)?;

        self.with_scope(false, true, move |visitor| {
            let declared_arguments = &declaration.args;
            let positional_len = evaluated.positional.len();
            let min_len = positional_len.min(declared_arguments.len());

            let mut bound = Vec::with_capacity(declared_arguments.len() + 1);

            for (i, val) in evaluated.positional.drain(..min_len).enumerate() {
                visitor
                    .env
                    .scopes_mut()
                    .insert_var_last(declared_arguments[i].name, val.clone());
                bound.push(val);
            }

            let additional_declared_args = if declared_arguments.len() > positional_len {
                &declared_arguments[positional_len..]
            } else {
                &[]
            };

            for argument in additional_declared_args {
                let value = match evaluated.named.shift_remove(&argument.name) {
                    Some(v) => v,
                    None => {
                        let default = argument.default.as_ref().unwrap();
                        let v = visitor.visit_expr_ref(default)?;
                        visitor.without_slash(v, || Self::expr_span(default, span))?
                    }
                };
                visitor
                    .env
                    .scopes_mut()
                    .insert_var_last(argument.name, value.clone());
                bound.push(value);
            }

            if declaration.rest.is_some() {
                let rest = mem::take(&mut evaluated.positional);
                let were_keywords_accessed = Rc::new(Cell::new(false));
                let arg_list = ArgList::new(
                    rest,
                    were_keywords_accessed,
                    evaluated.named.clone(),
                    if evaluated.separator == ListSeparator::Undecided {
                        ListSeparator::Comma
                    } else {
                        evaluated.separator
                    },
                );
                bound.push(Value::ArgList(arg_list));
            }

            Ok(ArgumentResult {
                positional: bound,
                named: SmallOrderedMap::default(),
                separator: evaluated.separator,
                span: evaluated.span,
                touched: FxHashSet::default(),
                spans: None,
            })
        })
    }

    fn import_like_node(
        &mut self,
        url: &str,
        for_import: bool,
        span: Span,
    ) -> SassResult<Rc<StyleSheet<'static>>> {
        match self.find_import(url.as_ref(), for_import, span)? {
            Some(ImportSource::Path(name)) => {
                let name = self.canonicalize(&name);
                if let Some(style_sheet) = self.import_cache.get(&ImportKey::Path(name.clone())) {
                    return Ok(Rc::clone(style_sheet));
                }

                let file = self.map.add_file(
                    name.to_string_lossy().into(),
                    String::from_utf8(self.options.fs.read(&name)?)?,
                );

                let old_is_use_allowed = self.flags.is_use_allowed();
                self.flags.set(ContextFlags::IS_USE_ALLOWED, true);

                let style_sheet = Rc::new(self.parse_file(
                    Lexer::new_from_file(&file),
                    &name,
                    name.clone(),
                    file.span.subspan(0, 0),
                )?);

                self.flags
                    .set(ContextFlags::IS_USE_ALLOWED, old_is_use_allowed);

                self.import_cache
                    .insert(ImportKey::Path(name), Rc::clone(&style_sheet));

                Ok(style_sheet)
            }
            Some(ImportSource::Resolved {
                canonical_url,
                contents,
                syntax,
            }) => {
                let key = ImportKey::Url(canonical_url.clone());
                if let Some(style_sheet) = self.import_cache.get(&key) {
                    return Ok(Rc::clone(style_sheet));
                }

                // Synthetic, non-filesystem path used only as the parsed
                // stylesheet's `url` (diagnostics, and the `@use`/`@forward`
                // module-cache key in `Visitor::modules`) -- never read
                // from disk, `contents` is parsed directly instead.
                let synthetic_path = PathBuf::from(&canonical_url);

                let file = self.map.add_file(canonical_url.clone(), contents);

                let old_is_use_allowed = self.flags.is_use_allowed();
                self.flags.set(ContextFlags::IS_USE_ALLOWED, true);

                let style_sheet = Rc::new(self.parse_file_with_syntax(
                    Lexer::new_from_file(&file),
                    &synthetic_path,
                    synthetic_path.clone(),
                    file.span.subspan(0, 0),
                    syntax,
                )?);

                self.flags
                    .set(ContextFlags::IS_USE_ALLOWED, old_is_use_allowed);

                // Both Path and Resolved imports are cached on first sight:
                // the same canonical URL resolving to the same module is a
                // correctness requirement (design doc §1.2, "same canonical
                // URL -> same cached module"), as well as a performance win.
                self.import_cache.insert(key, Rc::clone(&style_sheet));

                Ok(style_sheet)
            }
            None => Err(("Can't find stylesheet to import.", span).into()),
        }
    }

    pub(crate) fn load_style_sheet(
        &mut self,
        url: &str,
        // default=false
        for_import: bool,
        span: Span,
    ) -> SassResult<Rc<StyleSheet<'static>>> {
        // todo: import cache
        self.import_like_node(url, for_import, span)
    }

    fn visit_dynamic_import_rule(&mut self, dynamic_import: &AstSassImport) -> SassResult<()> {
        let stylesheet = self.load_style_sheet(dynamic_import.url, true, dynamic_import.span)?;

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

    fn visit_static_import_rule(
        &mut self,
        static_import: AstPlainCssImport<'static>,
    ) -> SassResult<()> {
        let import = self.interpolation_to_value(static_import.url, false, false)?;

        let modifiers = static_import
            .modifiers
            .map(|modifiers| self.interpolation_to_value(modifiers, false, false))
            .transpose()?;

        let node = CssStmt::Import(import, modifiers, Some(static_import.span));

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

    fn visit_content_rule(
        &mut self,
        content_rule: &AstContentRule<'static>,
    ) -> SassResult<Option<Value>> {
        let span = content_rule.args.span;
        if let Some(content) = &self.env.content {
            let content = Rc::clone(content);
            self.run_user_defined_callable(
                MaybeEvaledArguments::Invocation(&content_rule.args),
                Rc::clone(&content),
                &content.env,
                span,
                content_rule.span,
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

    fn visit_at_root_rule(
        &mut self,
        mut at_root_rule: AstAtRootRule<'static>,
    ) -> SassResult<Option<Value>> {
        let query = match at_root_rule.query {
            Some(query) => {
                let resolved = self.perform_interpolation_ref(&query.node, true)?;

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
            env: Rc::new(self.env.new_closure()),
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

    fn visit_extend_rule(
        &mut self,
        extend_rule: AstExtendRule<'static>,
    ) -> SassResult<Option<Value>> {
        if !self.style_rule_exists() || self.declaration_name.is_some() {
            return Err((
                "@extend may only be used within style rules.",
                extend_rule.span,
            )
                .into());
        }

        let super_selector = self.style_rule_ignoring_at_root.clone().unwrap();

        if let Some(original_selector) = self.original_selector.clone() {
            for complex in &original_selector.components {
                if !complex.is_bogus(true) {
                    continue;
                }

                let text = complex.to_string();
                let trimmed = text.trim();
                let cant_or_shouldnt = if complex.is_useless() {
                    "can't"
                } else {
                    "shouldn't"
                };

                self.emit_deprecation(Deprecation::BogusCombinators, extend_rule.span, || {
                    Ok(format!(
                        "The selector \"{trimmed}\" is invalid CSS and {cant_or_shouldnt} be an \
                         extender.\nThis will be an error in Dart Sass 2.0.0.\n\n\
                         More info: https://sass-lang.com/d/bogus-combinators"
                    ))
                })?;
            }
        }

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
        queries: &Interpolation<'static>,
        span: Span,
    ) -> SassResult<Vec<CssMediaQuery>> {
        let resolved = self.perform_interpolation_ref(queries, true)?;

        CssMediaQuery::parse_list(&resolved, span)
    }

    fn visit_media_rule(&mut self, media_rule: &AstMedia<'static>) -> SassResult<Option<Value>> {
        if self.declaration_name.is_some() {
            return Err((
                "Media rules may not be used within nested declarations.",
                media_rule.span,
            )
                .into());
        }

        let queries1 = self.visit_media_queries(&media_rule.query, media_rule.query_span)?;

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
        let at_rule_span = media_rule.span;

        let query = merged_queries.clone().unwrap_or_else(|| queries1.clone());

        let media_rule = CssStmt::Media(
            MediaRule {
                query,
                body: Vec::new(),
                query_span: Some(media_rule.query_span),
                at_rule_span: Some(at_rule_span),
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
                    at_rule_span: Some(unknown_at_rule.span),
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

        let at_rule_span = unknown_at_rule.span;
        let children = unknown_at_rule.body.unwrap();

        let stmt = CssStmt::UnknownAtRule(
            UnknownAtRule {
                name,
                params: value.unwrap_or_default(),
                body: Vec::new(),
                has_body: true,
                at_rule_span: Some(at_rule_span),
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

    /// Like [`Visitor::emit_warning`], but for a deprecated feature.
    ///
    /// Honors `Options::fatal_deprecation` (turns the warning into an
    /// error), `Options::silence_deprecation` / `Options::quiet` (drops the
    /// warning), and `Deprecation::is_future` combined with
    /// `Options::future_deprecation` (future deprecations are dropped unless
    /// explicitly opted into).
    /// `message` is constructed lazily: building a deprecation message can be
    /// nontrivial (serializing operands, walking `as_slash` chains), and
    /// most calls end up discarded by dedup/silence/quiet/future gating
    /// before the text is ever shown. Only the two branches that actually
    /// consume the text (fatal-error, warn) invoke `message`.
    pub(crate) fn emit_deprecation(
        &mut self,
        deprecation: Deprecation,
        span: Span,
        message: impl FnOnce() -> SassResult<String>,
    ) -> SassResult<()> {
        // Mirrors dart-sass's `_warningsEmitted` dedup, which runs before fatal/silence
        // handling: a given call site only ever triggers once, even if evaluated repeatedly
        // (e.g. inside a function called from a loop).
        //
        // dart's actual dedup key is (message, span), not (deprecation, span): a
        // message that varies by evaluated content at a fixed span (e.g.
        // bogus-combinators' interpolated selector text, or the same source line
        // hit with different values across loop iterations) must still warn once
        // per distinct message, not collapse to whichever text arrived first. We
        // approximate this without paying for a message build on every call by
        // only doing the extra (span, message) check on REVISITS to a deprecation
        // whose message can actually vary per visit
        // (`Deprecation::message_may_vary_per_visit`) — profiling found that
        // checking this unconditionally on every revisit regressed Bootstrap
        // ~5% (GlobalBuiltin, called repeatedly from inside color-manipulation
        // mixins with an always-identical message, paying for a message build
        // + clone + hashset insert on every one of those revisits for no
        // behavioral difference). The first time a (deprecation, span) pair is
        // seen, this behaves exactly as before (message stays unbuilt until the
        // fatal/silence/future gates pass) regardless of the deprecation type.
        let mut message = Some(message);
        let mut prebuilt_message = None;

        if !self
            .deprecation_warnings_emitted
            .insert((deprecation, span))
        {
            if !deprecation.message_may_vary_per_visit() {
                return Ok(());
            }

            let msg = (message.take().unwrap())()?;
            if !self
                .deprecation_messages_emitted
                .insert((span, msg.clone()))
            {
                return Ok(());
            }
            prebuilt_message = Some(msg);
        }

        if self.options.fatal_deprecations.contains(&deprecation) {
            let message = match prebuilt_message.take() {
                Some(m) => m,
                None => (message.take().unwrap())()?,
            };
            return Err((
                format!(
                    "{message}\n\nThis is only an error because you've set the {} \
                     deprecation to be fatal.\nRemove this setting if you need to keep using \
                     this feature.",
                    deprecation.id()
                ),
                span,
            )
                .into());
        }

        if self.options.quiet || self.options.silence_deprecations.contains(&deprecation) {
            return Ok(());
        }

        if deprecation.is_future() && !self.options.future_deprecations.contains(&deprecation) {
            return Ok(());
        }

        let message = match prebuilt_message {
            Some(m) => m,
            None => {
                // First-ever visit to this (deprecation, span): record the
                // message now so a later revisit with the SAME text dedupes
                // against it (see the revisit branch above).
                let m = (message.take().unwrap())()?;
                self.deprecation_messages_emitted.insert((span, m.clone()));
                m
            }
        };
        let loc = self.map.look_up_span(span);
        self.options
            .logger
            .warn_deprecation(loc, &message, deprecation.id());

        Ok(())
    }

    /// Warns when a `!global` assignment would declare a variable that
    /// doesn't already exist in the global scope, matching dart-sass's
    /// `Deprecation.newGlobal`. The message differs depending on whether the
    /// assignment is already at the stylesheet root (where `!global` is
    /// redundant) or nested (where the recommendation is to pre-declare the
    /// variable at the root instead).
    ///
    /// Note: unlike dart-sass's `node.originalName`, this uses the
    /// normalized (underscore-to-hyphen) identifier in the nested-case
    /// recommendation text, since grass doesn't retain the pre-normalization
    /// spelling on `AstVariableDecl`.
    fn maybe_warn_new_global(
        &mut self,
        name: Identifier,
        namespace: Option<Spanned<Identifier>>,
        is_global: bool,
        span: Span,
    ) -> SassResult<()> {
        if !is_global || namespace.is_some() || self.env.global_var_exists(name, span)? {
            return Ok(());
        }

        let at_root = self.env.at_root();

        self.emit_deprecation(Deprecation::NewGlobal, span, || {
            Ok(if at_root {
                "As of Dart Sass 2.0.0, !global assignments won't be able to declare new \
                 variables.\n\nSince this assignment is at the root of the stylesheet, the \
                 !global flag is\nunnecessary and can safely be removed."
                    .to_string()
            } else {
                format!(
                    "As of Dart Sass 2.0.0, !global assignments won't be able to declare new \
                     variables.\n\nRecommendation: add `${name}: null` at the stylesheet root."
                )
            })
        })
    }

    /// Warns for each bogus complex selector in a style rule's (already
    /// resolved/extended) selector list, matching dart-sass's
    /// `_warnForBogusCombinators` (called once per style rule, after its
    /// children have been visited).
    ///
    /// Known, documented simplifications vs dart-sass:
    /// - Uses the whole rule's `selector_span` rather than a per-complex-
    ///   selector span (`ComplexSelector` doesn't carry its own span in
    ///   grass), so multiple distinct bogus selectors within one
    ///   comma-separated list share a span and (via `emit_deprecation`'s
    ///   per-span dedup) only the first warns.
    /// - The "valid for nesting" message omits dart's secondary `MultiSpan`
    ///   annotation ("this is not a style rule") — grass's `Logger` only
    ///   supports a single span per warning.
    /// - The invisibility gate approximates dart's recursive
    ///   `isInvisibleOtherThanBogusCombinators` (which also considers
    ///   whether every descendant is invisible) with a selector-only check.
    /// - A *trailing*-combinator-only selector (e.g. `a >`) is valid dart
    ///   CSS-nesting syntax when every one of its children is itself a
    ///   nested style rule (it gets flattened into e.g. `a > b`, which is no
    ///   longer bogus, and is never warned about) — `only_nests_style_rules`
    ///   approximates that check from the pre-evaluation AST body, so it
    ///   doesn't follow control-flow (`@if`/`@each`) wrapping a nested rule.
    ///   Leading/doubled-combinator bogus-ness persists through flattening
    ///   in dart too, so those two shapes always warn regardless of this
    ///   flag — just using the pre-flattened (this rule's own) selector text
    ///   rather than each descendant's fully-flattened one.
    fn warn_for_bogus_combinators(
        &mut self,
        selector: &SelectorList,
        original_selector: &SelectorList,
        selector_span: Span,
        only_nests_style_rules: bool,
    ) -> SassResult<()> {
        if selector.is_invisible() {
            return Ok(());
        }

        for complex in &selector.components {
            if !complex.is_bogus(true) {
                continue;
            }

            // A bogus complex selector that only arrived via `@extend` (not
            // written directly on this rule) belongs to whichever rule wrote
            // it originally — that rule already warned about it (dart
            // achieves this via per-complex-selector span dedup; grass
            // approximates it by only warning for complexes this rule itself
            // wrote).
            if !original_selector.components.contains(complex) {
                continue;
            }

            let text = complex.to_string();
            let trimmed = text.trim();

            if complex.is_useless() {
                self.emit_deprecation(Deprecation::BogusCombinators, selector_span, || {
                    Ok(format!(
                        "The selector \"{trimmed}\" is invalid CSS. It will be omitted from \
                         the generated CSS.\nThis will be an error in Dart Sass 2.0.0.\n\n\
                         More info: https://sass-lang.com/d/bogus-combinators"
                    ))
                })?;
            } else if complex.has_leading_combinator() {
                if self.is_plain_css {
                    continue;
                }

                self.emit_deprecation(Deprecation::BogusCombinators, selector_span, || {
                    Ok(format!(
                        "The selector \"{trimmed}\" is invalid CSS.\nThis will be an error in \
                         Dart Sass 2.0.0.\n\nMore info: https://sass-lang.com/d/bogus-combinators"
                    ))
                })?;
            } else if !only_nests_style_rules {
                self.emit_deprecation(Deprecation::BogusCombinators, selector_span, || {
                    Ok(format!(
                        "The selector \"{trimmed}\" is only valid for nesting and shouldn't\n\
                         have children other than style rules. It will be omitted from the \
                         generated CSS.\nThis will be an error in Dart Sass 2.0.0.\n\n\
                         More info: https://sass-lang.com/d/bogus-combinators"
                    ))
                })?;
            }
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
                if let Some(existing) = self
                    .css_tree
                    .last_matching_media_sibling(parent, grandparent)
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
        if parent == CssTree::ROOT
            && self.in_module_import_section
            && !matches!(node, CssStmt::Comment(..) | CssStmt::Import(..))
        {
            self.flush_pending_imports(false);
            self.in_module_import_section = false;
        }

        // Only check interleaving inside style rules
        if self.style_rule_exists()
            && parent != CssTree::ROOT
            && self.css_tree.has_following_sibling(parent)
        {
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

    fn visit_include_stmt(
        &mut self,
        include_stmt: &AstInclude<'static>,
    ) -> SassResult<Option<Value>> {
        let mixin = self
            .env
            .get_mixin(include_stmt.name, include_stmt.namespace)?;

        match mixin {
            Mixin::Builtin(mixin) => {
                if include_stmt.content.is_some() {
                    return Err(("Mixin doesn't accept a content block.", include_stmt.span).into());
                }

                let args = self.eval_args(
                    &include_stmt.args,
                    include_stmt.name.span,
                    include_stmt.rule_span,
                )?;
                mixin(args, self)?;

                Ok(None)
            }
            Mixin::BuiltinWithContent(mixin) => {
                let args = self.eval_args(
                    &include_stmt.args,
                    include_stmt.name.span,
                    include_stmt.rule_span,
                )?;

                if let Some(content) = include_stmt.content.clone() {
                    let callable_content = Rc::new(CallableContentBlock {
                        content,
                        env: self.env.new_closure(),
                    });
                    self.with_content(Some(callable_content), |visitor| mixin(args, visitor))?;
                } else {
                    mixin(args, self)?;
                }

                Ok(None)
            }
            Mixin::UserDefined(mixin, env, defining_path) => {
                if include_stmt.content.is_some() && !mixin.has_content {
                    return Err(("Mixin doesn't accept a content block.", include_stmt.span).into());
                }

                let args = &include_stmt.args;
                let content = include_stmt.content.clone();

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
                    include_stmt.rule_span,
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
            Mixin::UserDefined(mixin, Rc::new(self.env.new_closure()), defining_path),
        );
    }

    fn visit_each_stmt(&mut self, each_stmt: &AstEach<'static>) -> SassResult<Option<Value>> {
        let list = self.visit_expr_ref(&each_stmt.list)?.as_list();

        // dart binds each loop variable with the list expression as its
        // node, so `b: $i` maps back to the `@each` list (p12 probe).
        let each_span = self.provenance_span(&each_stmt.list, each_stmt.list_span);

        // todo: not setting semi_global: true maybe means we can't assign to global scope when declared as global
        self.env.scope_enter();

        if let Some(span) = each_span {
            for &var in &each_stmt.variables {
                self.env.scopes_mut().insert_var_last_span(var, span);
            }
        }

        let mut result = None;

        'outer: for val in list {
            if each_stmt.variables.len() == 1 {
                let val = self.without_slash(val, || each_stmt.list_span)?;
                self.env
                    .scopes_mut()
                    .insert_var_last(each_stmt.variables[0], val);
            } else {
                for (&var, val) in each_stmt.variables.iter().zip(
                    val.as_list()
                        .into_iter()
                        .chain(std::iter::once(Value::Null).cycle()),
                ) {
                    let val = self.without_slash(val, || each_stmt.list_span)?;
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
        // dart binds the loop variable with the `from` expression as its
        // node (p14 probe: `b: $i` maps to the `1` after "from"). Computed
        // here because the eval below consumes `from.node`.
        let for_provenance = self.provenance_span(&for_stmt.from.node, from_span);
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

        if let Some(span) = for_provenance {
            self.env
                .scopes_mut()
                .insert_var_last_span(for_stmt.variable.node, span);
        }

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

            'outer: while visitor.visit_expr_ref(&while_stmt.condition)?.is_truthy() {
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

    fn interpolation_to_value(
        &mut self,
        interpolation: Interpolation<'static>,
        // default=false
        trim: bool,
        // default=false
        warn_for_color: bool,
    ) -> SassResult<String> {
        let result = self.perform_interpolation_ref(&interpolation, warn_for_color)?;

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
                InterpolationPart::String(s) => (*s).to_owned(),
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
                    InterpolationPart::String(s) => Ok((*s).to_owned()),
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

    fn evaluate_to_css(
        &mut self,
        expr: &AstExpr<'static>,
        quote: QuoteKind,
        span: Span,
    ) -> SassResult<String> {
        let result = self.visit_expr_ref(expr)?;
        self.serialize(result, quote, span)
    }

    /// The dart-sass `recommendation()` helper for a slash-separated
    /// `SassNumber`: recurses through `as_slash` so e.g. a number built from
    /// `math.div(1, 2)` results in `math.div(1, 2)` rather than the plain
    /// division result.
    fn slash_recommendation(number: &SassNumber, span: Span) -> String {
        match &number.as_slash {
            Some(parts) => format!(
                "math.div({}, {})",
                Self::slash_recommendation(&parts.0, span),
                Self::slash_recommendation(&parts.1, span)
            ),
            None => Value::Dimension(number.clone())
                .to_css_string(span, false)
                .unwrap_or_else(|_| format!("{}{}", number.num.0, number.unit)),
        }
    }

    /// Best-effort AST-based reconstruction of dart-sass's `recommendation()`
    /// text for the slash-div warning (dart builds this from the original,
    /// unevaluated expression's `toString()`, so e.g. `12 / $n` recommends
    /// `math.div(12, $n)` rather than substituting `$n`'s current value).
    /// Covers the common shapes explicitly (Number, Variable, Paren, nested
    /// `/`); returns `None` for anything else so the caller falls back to the
    /// evaluated value's text.
    fn div_operand_source_text(expr: &AstExpr<'static>, span: Span) -> Option<String> {
        match expr {
            AstExpr::Number { n, unit } => Some(
                Value::Dimension(SassNumber {
                    num: *n,
                    unit: unit.clone(),
                    as_slash: None,
                })
                .to_css_string(span, false)
                .unwrap_or_else(|_| format!("{}{}", n.0, unit)),
            ),
            AstExpr::Variable { name, namespace } => {
                let ns = namespace
                    .map(|ns| format!("{}.", ns.node))
                    .unwrap_or_default();
                Some(format!("{ns}${}", name.node))
            }
            // dart-sass's `ParenthesizedExpression() => expression.expression.toString()`
            // reconstructs from the INNER expression's plain syntax, dropping the
            // parens — critically, this does NOT recurse through the math.div
            // conversion below: a parenthesized nested division prints as literal
            // `a / b`, not `math.div(a, b)` (verified: `(12 / $n) / 2` recommends
            // `math.div(12 / $n, 2)` in dart, not `math.div(math.div(12, $n), 2)`).
            AstExpr::Paren(inner) => Self::div_operand_plain_text(inner, span),
            AstExpr::BinaryOp(binop) if binop.op == BinaryOp::Div => Some(format!(
                "math.div({}, {})",
                Self::div_operand_source_text(&binop.lhs, span)?,
                Self::div_operand_source_text(&binop.rhs, span)?
            )),
            _ => None,
        }
    }

    /// Plain (non-`math.div`-converted) reconstruction used for the content of
    /// a `Paren` inside `div_operand_source_text` — see its doc comment.
    fn div_operand_plain_text(expr: &AstExpr<'static>, span: Span) -> Option<String> {
        match expr {
            AstExpr::Number { .. } | AstExpr::Variable { .. } => {
                Self::div_operand_source_text(expr, span)
            }
            AstExpr::Paren(inner) => Self::div_operand_plain_text(inner, span),
            AstExpr::BinaryOp(binop) if binop.op == BinaryOp::Div => Some(format!(
                "{} / {}",
                Self::div_operand_plain_text(&binop.lhs, span)?,
                Self::div_operand_plain_text(&binop.rhs, span)?
            )),
            _ => None,
        }
    }

    fn without_slash(&mut self, v: Value, span: impl FnOnce() -> Span) -> SassResult<Value> {
        if let Value::Dimension(number) = &v {
            if number.as_slash.is_some() {
                let span = span();
                self.emit_deprecation(Deprecation::SlashDiv, span, || {
                    Ok(format!(
                        "Using / for division is deprecated and will be removed in Dart Sass \
                         2.0.0.\n\nRecommendation: {}\n\nMore info and automated migrator: \
                         https://sass-lang.com/d/slash-div",
                        Self::slash_recommendation(number, span)
                    ))
                })?;
            }
        }

        Ok(v.without_slash())
    }

    /// Best-effort span for an expression, for `without_slash` call sites that
    /// only have a bare (already-parsed) sub-expression to work from rather
    /// than dart-sass's full AST-node provenance tracking. Falls back to the
    /// caller's own nearby span when the variant carries none directly.
    fn expr_span(expr: &AstExpr<'static>, fallback: Span) -> Span {
        match expr {
            AstExpr::BinaryOp(binop) => binop.span,
            AstExpr::String(_, span) | AstExpr::UnaryOp(.., span) => *span,
            AstExpr::FunctionCall(func) => func.span,
            AstExpr::InterpolatedFunction(func) => func.span,
            AstExpr::CalculationWithFallback(calc) => calc.span,
            AstExpr::Calculation(calc) => calc.span,
            AstExpr::CssIf(css_if) => css_if.span,
            AstExpr::Variable { name, .. } => name.span,
            _ => fallback,
        }
    }

    fn eval_maybe_args(
        &mut self,
        args: MaybeEvaledArguments<'_, 'static>,
        span: Span,
        callable_node_span: Span,
    ) -> SassResult<ArgumentResult> {
        match args {
            MaybeEvaledArguments::Invocation(args) => {
                self.eval_args(args, span, callable_node_span)
            }
            MaybeEvaledArguments::Evaled(args) => Ok(args),
        }
    }

    fn eval_args(
        &mut self,
        arguments: &ArgumentInvocation<'static>,
        span: Span,
        callable_node_span: Span,
    ) -> SassResult<ArgumentResult> {
        // Monomorphized like `Environment::insert_var_impl` (Plan 113): the
        // `REC = false` instantiation compiles every span-recording block out
        // of the hottest call path — a runtime `options.source_map` test in
        // the shared body measurably failed the maps-off instruction gate.
        if self.options.source_map {
            self.eval_args_impl::<true>(arguments, span, callable_node_span)
        } else {
            self.eval_args_impl::<false>(arguments, span, callable_node_span)
        }
    }

    fn eval_args_impl<const REC: bool>(
        &mut self,
        arguments: &ArgumentInvocation<'static>,
        span: Span,
        callable_node_span: Span,
    ) -> SassResult<ArgumentResult> {
        let mut positional = Vec::with_capacity(arguments.positional.len());

        for expr in arguments.positional {
            let val = self.visit_expr_ref(expr)?;
            positional.push(self.without_slash(val, || Self::expr_span(expr, span))?);
        }

        let mut named = SmallOrderedMap::default();

        for (key, expr) in arguments.named {
            let val = self.visit_expr_ref(expr)?;
            named.insert(
                *key,
                self.without_slash(val, || Self::expr_span(expr, span))?,
            );
        }

        // Provenance spans (dart's `positionalNodes`/`namedNodes`), computed
        // after the values like dart does. With `REC = false` this and every
        // use of `spans` below compile out entirely.
        let mut spans = if REC {
            let mut arg_spans = Box::new(ArgumentSpans {
                positional: Vec::with_capacity(positional.len()),
                named: FxHashMap::default(),
                callable_node: callable_node_span,
            });
            for (i, expr) in arguments.positional.iter().enumerate() {
                arg_spans.positional.push(
                    arguments
                        .positional_spans
                        .get(i)
                        .map(|s| self.provenance_span(expr, *s).unwrap_or(*s)),
                );
            }
            for (i, (key, expr)) in arguments.named.iter().enumerate() {
                if let Some(s) = arguments.named_spans.get(i) {
                    arg_spans
                        .named
                        .insert(*key, self.provenance_span(expr, *s).unwrap_or(*s));
                }
            }
            Some(arg_spans)
        } else {
            None
        };

        if arguments.rest.is_none() {
            return Ok(ArgumentResult {
                positional,
                named,
                separator: ListSeparator::Undecided,
                span,
                touched: FxHashSet::default(),
                spans,
            });
        }

        let rest_expr = arguments.rest.as_ref().unwrap();
        let rest = self.visit_expr_ref(rest_expr)?;

        // dart's `restNodeForSpan`: every argument expanded from `$rest...`
        // maps to the rest expression itself (chain-collapsed).
        let rest_node_span = if REC {
            spans.as_ref().map(|_| {
                let s = arguments.rest_span.unwrap_or(arguments.span);
                self.provenance_span(rest_expr, s).unwrap_or(s)
            })
        } else {
            None
        };

        let mut separator = ListSeparator::Undecided;

        match rest {
            Value::Map(rest) => self.add_rest_map(
                &mut named,
                rest,
                || Self::expr_span(rest_expr, span),
                if REC {
                    spans
                        .as_deref_mut()
                        .zip(rest_node_span)
                        .map(|(sp, rest_span)| (&mut sp.named, rest_span))
                } else {
                    None
                },
            )?,
            Value::List(elems, list_separator, _) => {
                for e in Rc::unwrap_or_clone(elems) {
                    positional.push(self.without_slash(e, || Self::expr_span(rest_expr, span))?);
                    if REC {
                        if let Some(sp) = &mut spans {
                            sp.positional.push(rest_node_span);
                        }
                    }
                }
                separator = list_separator;
            }
            Value::ArgList(arglist) => {
                // todo: superfluous clone
                for (&key, value) in arglist.keywords() {
                    named.insert(
                        key,
                        self.without_slash(value.clone(), || Self::expr_span(rest_expr, span))?,
                    );
                    if REC {
                        if let Some((sp, rest_span)) = spans.as_deref_mut().zip(rest_node_span) {
                            sp.named.insert(key, rest_span);
                        }
                    }
                }

                for e in arglist.elems {
                    positional.push(self.without_slash(e, || Self::expr_span(rest_expr, span))?);
                    if REC {
                        if let Some(sp) = &mut spans {
                            sp.positional.push(rest_node_span);
                        }
                    }
                }
                separator = arglist.separator;
            }
            _ => {
                positional.push(self.without_slash(rest, || Self::expr_span(rest_expr, span))?);
                if REC {
                    if let Some(sp) = &mut spans {
                        sp.positional.push(rest_node_span);
                    }
                }
            }
        }

        if arguments.keyword_rest.is_none() {
            return Ok(ArgumentResult {
                positional,
                named,
                separator,
                span: arguments.span,
                touched: FxHashSet::default(),
                spans,
            });
        }

        let keyword_rest_expr = arguments.keyword_rest.as_ref().unwrap();

        match self.visit_expr_ref(keyword_rest_expr)? {
            Value::Map(keyword_rest) => {
                let keyword_rest_node_span = if REC {
                    spans.as_ref().map(|_| {
                        let s = arguments.keyword_rest_span.unwrap_or(arguments.span);
                        self.provenance_span(keyword_rest_expr, s).unwrap_or(s)
                    })
                } else {
                    None
                };

                self.add_rest_map(
                    &mut named,
                    keyword_rest,
                    || Self::expr_span(keyword_rest_expr, span),
                    if REC {
                        spans
                            .as_deref_mut()
                            .zip(keyword_rest_node_span)
                            .map(|(sp, rest_span)| (&mut sp.named, rest_span))
                    } else {
                        None
                    },
                )?;

                Ok(ArgumentResult {
                    positional,
                    named,
                    separator,
                    span: arguments.span,
                    touched: FxHashSet::default(),
                    spans,
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
        named: &mut SmallOrderedMap<Identifier, Value>,
        rest: SassMap,
        span: impl Fn() -> Span,
        mut span_record: Option<(&mut FxHashMap<Identifier, Span>, Span)>,
    ) -> SassResult<()> {
        for (key, val) in rest {
            match key.node {
                Value::String(text, ..) => {
                    let val = self.without_slash(val, &span)?;
                    let name = Identifier::from(text.as_str());
                    named.insert(name, val);
                    if let Some((named_spans, rest_span)) = &mut span_record {
                        named_spans.insert(name, *rest_span);
                    }
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
        arguments: MaybeEvaledArguments<'_, 'static>,
        func: F,
        env: &Environment,
        span: Span,
        callable_node_span: Span,
        run: R,
    ) -> SassResult<V> {
        if self.recursion_depth >= MAX_CALLABLE_RECURSION_DEPTH {
            return Err(("Too much nesting.", span).into());
        }

        self.recursion_depth += 1;
        let result = self.run_user_defined_callable_inner(
            arguments,
            func,
            env,
            span,
            callable_node_span,
            run,
        );
        self.recursion_depth -= 1;

        result
    }

    fn run_user_defined_callable_inner<
        F: UserDefinedCallable,
        V: fmt::Debug,
        R: FnOnce(F, &mut Self) -> SassResult<V>,
    >(
        &mut self,
        arguments: MaybeEvaledArguments<'_, 'static>,
        func: F,
        env: &Environment,
        span: Span,
        callable_node_span: Span,
        run: R,
    ) -> SassResult<V> {
        let mut evaluated = self.eval_maybe_args(arguments, span, callable_node_span)?;

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

                // Bound-argument provenance (dart records a node per bound
                // argument). `Some` only when source maps are on, so the
                // maps-off cost here is one branch per bind loop.
                let arg_spans = evaluated.spans.take();

                // Drain positional args in forward order (O(n) total vs O(n²) from remove())
                match &arg_spans {
                    None => {
                        for (i, val) in evaluated.positional.drain(..min_len).enumerate() {
                            visitor
                                .env
                                .scopes_mut()
                                .insert_var_last(declared_arguments[i].name, val);
                        }
                    }
                    Some(spans) => {
                        for (i, val) in evaluated.positional.drain(..min_len).enumerate() {
                            let name = declared_arguments[i].name;
                            visitor.env.scopes_mut().insert_var_last(name, val);
                            let arg_span = spans
                                .positional
                                .get(i)
                                .copied()
                                .flatten()
                                .unwrap_or(spans.callable_node);
                            visitor
                                .env
                                .scopes_mut()
                                .insert_var_last_span(name, arg_span);
                        }
                    }
                }

                // todo: better name for var
                let additional_declared_args = if declared_arguments.len() > positional_len {
                    &declared_arguments[positional_len..declared_arguments.len()]
                } else {
                    &[]
                };

                for argument in additional_declared_args {
                    let name = argument.name;
                    let (value, arg_span) = match evaluated.named.shift_remove(&argument.name) {
                        Some(value) => {
                            let arg_span = arg_spans.as_ref().map(|spans| {
                                spans
                                    .named
                                    .get(&name)
                                    .copied()
                                    .unwrap_or(spans.callable_node)
                            });
                            (value, arg_span)
                        }
                        None => {
                            let default = argument.default.as_ref().unwrap();
                            let v = visitor.visit_expr_ref(default)?;
                            let value =
                                visitor.without_slash(v, || Self::expr_span(default, span))?;
                            // dart maps a defaulted parameter to its default
                            // expression (chain-collapsed; earlier parameters
                            // are already bound and consultable here).
                            let arg_span = arg_spans.as_ref().map(|spans| {
                                let fallback = argument.default_span.unwrap_or(spans.callable_node);
                                visitor
                                    .provenance_span(default, fallback)
                                    .unwrap_or(fallback)
                            });
                            (value, arg_span)
                        }
                    };
                    visitor.env.scopes_mut().insert_var_last(name, value);
                    if let Some(arg_span) = arg_span {
                        visitor
                            .env
                            .scopes_mut()
                            .insert_var_last_span(name, arg_span);
                    }
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
                    // dart binds the rest arglist to the invocation node
                    // (`@include` rule / call expression / `@content` rule).
                    if let Some(spans) = &arg_spans {
                        visitor
                            .env
                            .scopes_mut()
                            .insert_var_last_span(rest_arg, spans.callable_node);
                    }

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
                        .map(|key| format!("${key}"))
                        .collect(),
                    "or",
                );

                Err((format!("No {argument_word} named {argument_names}."), span).into())
            })
        })
    }

    pub(crate) fn run_function_callable(
        &mut self,
        func: SassFunction,
        arguments: &'static ArgumentInvocation<'static>,
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
        arguments: MaybeEvaledArguments<'_, 'static>,
        span: Span,
    ) -> SassResult<Value> {
        match func {
            SassFunction::Builtin(func, _name) => {
                let evaluated = self.eval_maybe_args(arguments, span, span)?;
                let val = match &func.0 {
                    BuiltinFn::Static(f) => f(evaluated, self)?,
                    BuiltinFn::Dynamic { f, signature } => {
                        let bound = self.bind_dynamic_args(signature.as_ref(), evaluated, span)?;
                        f(bound, self)?
                    }
                };
                self.without_slash(val, || span)
            }
            SassFunction::UserDefined(UserDefinedFunction { function, env, .. }) => self
                .run_user_defined_callable(
                    arguments,
                    function,
                    &env,
                    span,
                    span,
                    |function, visitor| {
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
                    },
                ),
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
                        rest = args.rest.as_ref();

                        let mut result = Vec::with_capacity(args.positional.len());
                        for arg in args.positional {
                            let value = self.visit_expr_ref(arg)?;

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

                let mut buffer = format!("{original_name}(");
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
                    let rest = self.visit_expr_ref(rest_arg)?;
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

    fn visit_list_expr(&mut self, list: &ListExpr<'static>) -> SassResult<Value> {
        let elems = list
            .elems
            .iter()
            .map(|e| {
                let value = self.visit_expr_ref(&e.node)?;
                Ok(value)
            })
            .collect::<SassResult<Vec<_>>>()?;

        Ok(Value::List(Rc::new(elems), list.separator, list.brackets))
    }

    fn visit_function_call_expr(
        &mut self,
        func_call: &FunctionCallExpr<'static>,
    ) -> SassResult<Value> {
        let name = func_call.name;

        // If the function name starts with -- AND was written with hyphens in source
        // (not underscores normalized to hyphens), treat as CSS custom function
        if name.as_str().starts_with("--") && func_call.is_css_custom_function {
            return self.run_function_callable(
                SassFunction::Plain {
                    name,
                    original_name: func_call.original_name.clone(),
                },
                func_call.arguments,
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
                    if let Some((module, fn_name)) = f.2 {
                        self.emit_deprecation(Deprecation::GlobalBuiltin, func_call.span, || {
                            Ok(global_builtin_message(module, fn_name))
                        })?;
                    }
                    SassFunction::Builtin(f.clone(), name)
                } else {
                    SassFunction::Plain {
                        name,
                        original_name: func_call.original_name.clone(),
                    }
                }
            }
        };

        let old_in_function = self.flags.in_function();
        self.flags.set(ContextFlags::IN_FUNCTION, true);
        let value = self.run_function_callable(func, func_call.arguments, func_call.span)?;
        self.flags.set(ContextFlags::IN_FUNCTION, old_in_function);

        Ok(value)
    }

    /// Evaluate a CSS math function call (min, sqrt, round, ...) that dart-sass
    /// allows a user-defined or module function to shadow. A user/module function
    /// of the same name (found via `get_fn`, which never resolves to grass's own
    /// global builtins) takes precedence over the calculation, matching dart's
    /// `getFunction`-then-switch order in `visitFunctionExpression`.
    fn visit_calculation_with_fallback(
        &mut self,
        node: &CalculationWithFallbackExpr<'static>,
    ) -> SassResult<Value> {
        match self.env.get_fn(node.name, None, node.span)? {
            Some(func) => {
                let old_in_function = self.flags.in_function();
                self.flags.set(ContextFlags::IN_FUNCTION, true);
                let value = self.run_function_callable(func, node.invocation, node.span);
                self.flags.set(ContextFlags::IN_FUNCTION, old_in_function);
                value
            }
            None => match &node.calculation_error {
                Some((message, span)) => Err((message.clone(), *span).into()),
                None => self.visit_expr_ref(&node.calculation),
            },
        }
    }

    fn visit_interpolated_func_expr(
        &mut self,
        func: &InterpolatedFunction<'static>,
    ) -> SassResult<Value> {
        let InterpolatedFunction {
            name,
            arguments: args,
            span,
        } = func;
        let span = *span;
        let fn_name = self.perform_interpolation_ref(name, false)?;

        if !args.named.is_empty() || args.keyword_rest.is_some() {
            return Err(("Plain CSS functions don't support keyword arguments.", span).into());
        }

        let mut buffer = format!("{fn_name}(");

        let mut first = true;
        for arg in args.positional {
            if first {
                first = false;
            } else {
                buffer.push_str(", ");
            }
            let evaluated = self.evaluate_to_css(arg, QuoteKind::Quoted, span)?;
            buffer.push_str(&evaluated);
        }

        if let Some(rest_arg) = &args.rest {
            let rest = self.visit_expr_ref(rest_arg)?;
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
            AstExpr::BinaryOp(binop) => self.visit_bin_op(
                &binop.lhs,
                binop.op,
                &binop.rhs,
                binop.allows_slash,
                binop.span,
            )?,
            AstExpr::Paren(inner) => self.visit_expr_ref(inner)?,
            AstExpr::UnaryOp(op, inner, span) => self.visit_unary_op(*op, inner, *span)?,
            AstExpr::List(list) => self.visit_list_expr(list)?,
            AstExpr::String(StringExpr(text, quote), ..) => self.visit_string(text, *quote)?,
            AstExpr::Calculation(calc) => {
                self.visit_calculation_expr(calc.name, &calc.args, calc.span)?
            }
            AstExpr::CalculationWithFallback(node) => self.visit_calculation_with_fallback(node)?,
            AstExpr::CssIf(css_if) => self.visit_css_if(css_if)?,
            AstExpr::FunctionCall(func_call) => self.visit_function_call_expr(func_call)?,
            AstExpr::If(if_expr) => self.visit_ternary(if_expr)?,
            AstExpr::InterpolatedFunction(func) => self.visit_interpolated_func_expr(func)?,
            AstExpr::Map(map) => self.visit_map(map)?,
            AstExpr::Supports(condition) => Value::String(
                self.visit_supports_condition_ref(condition)?.into(),
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
        _name: &str,
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
        expr: &AstExpr<'static>,
        in_min_or_max: bool,
        span: Span,
    ) -> SassResult<CalculationArg> {
        Ok(match expr {
            AstExpr::Paren(inner) => {
                let result = self.visit_calculation_value(inner, in_min_or_max, span)?;

                match result {
                    CalculationArg::String(text) => CalculationArg::String(format!("({text})")),
                    CalculationArg::Interpolation(text) => {
                        CalculationArg::String(format!("({text})"))
                    }
                    other => other,
                }
            }
            AstExpr::String(string_expr, _span) => {
                debug_assert!(string_expr.1 == QuoteKind::None);
                let text = self.perform_interpolation_ref(&string_expr.0, false)?;
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
            AstExpr::BinaryOp(binop) => SassCalculation::operate_internal(
                binop.op,
                self.visit_calculation_value(&binop.lhs, in_min_or_max, span)?,
                self.visit_calculation_value(&binop.rhs, in_min_or_max, span)?,
                in_min_or_max,
                !self.flags.in_supports_declaration(),
                self.options,
                span,
            )?,
            AstExpr::Number { .. }
            | AstExpr::Calculation(..)
            | AstExpr::CalculationWithFallback(..)
            | AstExpr::Variable { .. }
            | AstExpr::CssIf(..)
            | AstExpr::FunctionCall { .. }
            | AstExpr::If(..)
            | AstExpr::UnaryOp(..) => {
                let result = self.visit_expr_ref(expr)?;
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
            AstExpr::List(list) => {
                let message = if list.elems.is_empty() {
                    "This expression can't be used in a calculation."
                } else {
                    "Missing math operator."
                };
                return Err((message, span).into());
            }
            v => unreachable!("{:?}", v),
        })
    }

    fn visit_calculation_expr(
        &mut self,
        name: CalculationName,
        ast_args: &[AstExpr<'static>],
        span: Span,
    ) -> SassResult<Value> {
        // For single-arg functions (abs, round), when calculation arg
        // resolution fails due to incompatible units (e.g. abs(1 + 1px)),
        // fall back to evaluating as the Sass math function where unitless
        // values freely combine with units.
        let single_arg_fallback =
            matches!(name, CalculationName::Abs | CalculationName::Round) && ast_args.len() == 1;

        let resolved = ast_args
            .iter()
            .map(|arg| self.visit_calculation_value(arg, name.in_min_or_max(), span))
            .collect::<SassResult<Vec<_>>>();

        let mut args = match resolved {
            Ok(args) => args,
            Err(e) if single_arg_fallback => {
                let val = self.visit_expr_ref(&ast_args[0])?;
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

        if name == CalculationName::Calc && args.is_empty() {
            return Err(("Missing argument.", span).into());
        }
        if name == CalculationName::Clamp && args.is_empty() {
            return Err(("Missing argument.", span).into());
        }

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
                let arg = SassCalculation::simplify(args.pop().unwrap());
                // Mirrors dart-sass's `SassCalculation.abs`: this also covers
                // the legacy un-namespaced `abs(...)` call (routed here via
                // `visit_calculation_with_fallback` for calc-safe argument
                // shapes), not just an explicit `calc(abs(...))`.
                if let CalculationArg::Number(ref n) = arg {
                    if n.unit == Unit::Percent {
                        let number_text = serialize_number(n, self.options, span)?;
                        self.emit_deprecation(Deprecation::AbsPercent, span, || {
                            Ok(format!(
                                "Passing percentage units to the global abs() function is \
                                 deprecated.\nIn the future, this will emit a CSS abs() function \
                                 to be resolved by the browser.\nTo preserve current behavior: \
                                 math.abs({number_text})\nTo emit a CSS abs() now: \
                                 abs(#{{{number_text}}})\nMore info: \
                                 https://sass-lang.com/d/abs-percent"
                            ))
                        })?;
                    }
                }
                SassCalculation::abs(arg, self.options, span)
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
                    return Err(("1 argument required, but only 0 were passed.", span).into());
                }
                SassCalculation::log(args, self.options, span)
            }
            CalculationName::Hypot => {
                if args.is_empty() {
                    return Err(("hypot() must have at least one argument.", span).into());
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
                        CalculationArg::String(s) | CalculationArg::Interpolation(s) => {
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

    fn visit_unary_op(
        &mut self,
        op: UnaryOp,
        expr: &AstExpr<'static>,
        span: Span,
    ) -> SassResult<Value> {
        let operand = self.visit_expr_ref(expr)?;

        match op {
            UnaryOp::Plus => operand.unary_plus(self, span),
            UnaryOp::Neg => operand.unary_neg(self, span),
            UnaryOp::Div => operand.unary_div(self, span),
            UnaryOp::Not => Ok(operand.unary_not()),
        }
    }

    fn visit_ternary(&mut self, if_expr: &Ternary<'static>) -> SassResult<Value> {
        // When rest args are present, evaluate all args eagerly (can't do lazy
        // evaluation since rest values are already evaluated)
        if if_expr.0.rest.is_some() {
            let span = if_expr.0.span;
            let mut args = self.eval_args(&if_expr.0, span, span)?;
            args.max_args(3)?;
            let value = if args.get_err(0, "condition")?.is_truthy() {
                args.get_err(1, "if-true")?
            } else {
                args.get_err(2, "if-false")?
            };
            return self.without_slash(value, || span);
        }

        if_arguments(self.arena).verify(
            if_expr.0.positional.len(),
            if_expr.0.named,
            if_expr.0.span,
        )?;

        let positional = if_expr.0.positional;
        let named = if_expr.0.named;

        // Consume positional args left-to-right, falling back to named lookup
        // once positional is exhausted (mirrors ArgumentResult::get semantics
        // without needing to mutate/remove from the borrowed invocation).
        let mut next_idx = 0;

        let condition = if next_idx < positional.len() {
            let v = &positional[next_idx];
            next_idx += 1;
            v
        } else {
            NamedArgsView::get(named, &Identifier::from("condition")).unwrap()
        };

        let if_true = if next_idx < positional.len() {
            let v = &positional[next_idx];
            next_idx += 1;
            v
        } else {
            NamedArgsView::get(named, &Identifier::from("if_true")).unwrap()
        };

        let if_false = if next_idx < positional.len() {
            &positional[next_idx]
        } else {
            NamedArgsView::get(named, &Identifier::from("if_false")).unwrap()
        };

        let chosen = if self.visit_expr_ref(condition)?.is_truthy() {
            if_true
        } else {
            if_false
        };
        let value = self.visit_expr_ref(chosen)?;

        self.without_slash(value, || Self::expr_span(chosen, if_expr.0.span))
    }

    fn visit_css_if(&mut self, css_if: &CssIfExpression<'static>) -> SassResult<Value> {
        // Validate: sass() and raw substitutions cannot coexist in same condition
        for clause in &css_if.clauses {
            self.check_no_sass_with_raw(&clause.condition, css_if.span)?;
        }

        // Evaluate each clause
        for clause in &css_if.clauses {
            match self.eval_if_condition(&clause.condition)? {
                ConditionResult::True => {
                    let value = self.visit_expr_ref(&clause.value)?;
                    return self
                        .without_slash(value, || Self::expr_span(&clause.value, css_if.span));
                }
                ConditionResult::False => continue,
                ConditionResult::Css(remaining) => {
                    // This clause has CSS parts that can't be evaluated.
                    // Collect remaining clauses as CSS output.
                    return self.build_css_if_output(&remaining, clause, css_if);
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
        let val_str = self.evaluate_to_css(&first_clause.value, QuoteKind::None, css_if.span)?;
        parts.push(format!("{cond_str}: {val_str}"));

        // Find remaining clauses after the first CSS one
        let first_idx = css_if
            .clauses
            .iter()
            .position(|c| std::ptr::eq(c, first_clause))
            .unwrap_or(0);

        for clause in &css_if.clauses[first_idx + 1..] {
            match &clause.condition {
                IfCondition::Else => {
                    let val_str =
                        self.evaluate_to_css(&clause.value, QuoteKind::None, css_if.span)?;
                    parts.push(format!("else: {val_str}"));
                }
                other => {
                    match self.eval_if_condition(other)? {
                        ConditionResult::True => {
                            // Sass condition that's true — this becomes the value
                            let val_str =
                                self.evaluate_to_css(&clause.value, QuoteKind::None, css_if.span)?;
                            // Replace all remaining with just this value
                            parts.push(format!("else: {val_str}"));
                            break;
                        }
                        ConditionResult::False => {
                            // Sass condition that's false — skip this clause
                            continue;
                        }
                        ConditionResult::Css(remaining) => {
                            let cond_str = self.serialize_if_condition(&remaining)?;
                            let val_str =
                                self.evaluate_to_css(&clause.value, QuoteKind::None, css_if.span)?;
                            parts.push(format!("{cond_str}: {val_str}"));
                        }
                    }
                }
            }
        }

        let output = format!("if({})", parts.join("; "));
        Ok(Value::String(output.into(), QuoteKind::None))
    }

    fn eval_if_condition(
        &mut self,
        condition: &IfCondition<'static>,
    ) -> SassResult<ConditionResult> {
        match condition {
            IfCondition::Else => Ok(ConditionResult::True),
            IfCondition::Atom(atom) => self.eval_if_atom(atom),
            IfCondition::Not(inner, _span) => {
                match self.eval_if_condition(inner)? {
                    ConditionResult::True => Ok(ConditionResult::False),
                    ConditionResult::False => Ok(ConditionResult::True),
                    ConditionResult::Css(inner_cond) => {
                        // Safety: see `erase_ref_lifetime` — `self.arena` lives for
                        // the whole compilation, so this reference is valid for
                        // as long as any other 'static-erased AST reference.
                        let inner_cond =
                            unsafe { crate::ast::erase_ref_lifetime(self.arena.alloc(inner_cond)) };
                        Ok(ConditionResult::Css(IfCondition::Not(inner_cond, *_span)))
                    }
                }
            }
            IfCondition::Paren(inner) => match self.eval_if_condition(inner)? {
                ConditionResult::True => Ok(ConditionResult::True),
                ConditionResult::False => Ok(ConditionResult::False),
                ConditionResult::Css(inner_cond) => {
                    let inner_cond =
                        unsafe { crate::ast::erase_ref_lifetime(self.arena.alloc(inner_cond)) };
                    Ok(ConditionResult::Css(IfCondition::Paren(inner_cond)))
                }
            },
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
                    Ok(ConditionResult::Css(unwrap_paren(
                        remaining_css.pop().unwrap(),
                    )))
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
                    Ok(ConditionResult::Css(unwrap_paren(
                        remaining_css.pop().unwrap(),
                    )))
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
                let value = self.visit_expr_ref(expr)?;
                if value.is_truthy() {
                    Ok(ConditionResult::True)
                } else {
                    Ok(ConditionResult::False)
                }
            }
            IfConditionAtom::Css(interp, span) | IfConditionAtom::CssRaw(interp, span) => {
                // Evaluate any interpolations within the CSS text
                let text = self.perform_interpolation_ref(interp, false)?;
                Ok(ConditionResult::Css(IfCondition::Atom(
                    IfConditionAtom::Css(
                        unsafe {
                            crate::ast::erase_interpolation_lifetime(
                                InterpolationBuilder::new_plain(text).finish(self.arena),
                            )
                        },
                        *span,
                    ),
                )))
            }
            IfConditionAtom::Interp(expr, span) => {
                let value = self.visit_expr_ref(expr)?;
                let text = self.serialize(value, QuoteKind::None, *span)?;
                Ok(ConditionResult::Css(IfCondition::Atom(
                    IfConditionAtom::Css(
                        unsafe {
                            crate::ast::erase_interpolation_lifetime(
                                InterpolationBuilder::new_plain(text).finish(self.arena),
                            )
                        },
                        *span,
                    ),
                )))
            }
        }
    }

    // `self` is required for recursive calls; this is a 1.88 clippy gate allowance.
    #[allow(clippy::only_used_in_recursion)]
    fn serialize_if_condition(&mut self, condition: &IfCondition<'static>) -> SassResult<String> {
        match condition {
            IfCondition::Else => Ok("else".to_string()),
            IfCondition::Atom(atom) => match atom {
                IfConditionAtom::Css(interp, _) | IfConditionAtom::CssRaw(interp, _) => {
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
                Ok(format!("not {inner_str}"))
            }
            IfCondition::Paren(inner) => {
                let inner_str = self.serialize_if_condition(inner)?;
                Ok(format!("({inner_str})"))
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

    fn visit_string(
        &mut self,
        text: &Interpolation<'static>,
        quote: QuoteKind,
    ) -> SassResult<Value> {
        // Don't use [performInterpolation] here because we need to get the raw text
        // from strings, rather than the semantic value.
        let old_in_supports_declaration = self.flags.in_supports_declaration();
        self.flags.set(ContextFlags::IN_SUPPORTS_DECLARATION, false);

        let result = match text.contents.len() {
            0 => String::new(),
            1 => match &text.contents[0] {
                InterpolationPart::String(s) => (*s).to_owned(),
                InterpolationPart::Expr(Spanned { node, span }) => {
                    match self.visit_expr_ref(node)? {
                        Value::String(s, ..) => s.to_string(),
                        e => self.serialize(e, QuoteKind::None, *span)?,
                    }
                }
            },
            _ => text
                .contents
                .iter()
                .map(|part| match part {
                    InterpolationPart::String(s) => Ok((*s).to_owned()),
                    InterpolationPart::Expr(Spanned { node, span }) => {
                        match self.visit_expr_ref(node)? {
                            Value::String(s, ..) => Ok(s.to_string()),
                            e => self.serialize(e, QuoteKind::None, *span),
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

    fn visit_map(&mut self, map: &AstSassMap<'static>) -> SassResult<Value> {
        let mut sass_map = SassMap::new();

        for pair in map.0 {
            let key_span = pair.0.span;
            let key = self.visit_expr_ref(&pair.0.node)?;
            let value = self.visit_expr_ref(&pair.1)?;

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
        lhs: &AstExpr<'static>,
        op: BinaryOp,
        rhs: &AstExpr<'static>,
        allows_slash: bool,
        span: Span,
    ) -> SassResult<Value> {
        let left = self.visit_expr_ref(lhs)?;

        Ok(match op {
            BinaryOp::SingleEq => {
                let right = self.visit_expr_ref(rhs)?;
                single_eq(&left, &right, self.options, span)?
            }
            BinaryOp::Or => {
                if left.is_truthy() {
                    left
                } else {
                    self.visit_expr_ref(rhs)?
                }
            }
            BinaryOp::And => {
                if left.is_truthy() {
                    self.visit_expr_ref(rhs)?
                } else {
                    left
                }
            }
            BinaryOp::Equal => {
                let right = self.visit_expr_ref(rhs)?;
                Value::bool(left == right)
            }
            BinaryOp::NotEqual => {
                let right = self.visit_expr_ref(rhs)?;
                Value::bool(left != right)
            }
            BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual
            | BinaryOp::LessThan
            | BinaryOp::LessThanEqual => {
                let right = self.visit_expr_ref(rhs)?;
                cmp(&left, &right, self.options, span, op)?
            }
            BinaryOp::Plus => {
                let right = self.visit_expr_ref(rhs)?;
                add(left, right, self.options, span)?
            }
            BinaryOp::Minus => {
                let right = self.visit_expr_ref(rhs)?;
                sub(left, right, self.options, span)?
            }
            BinaryOp::Mul => {
                let right = self.visit_expr_ref(rhs)?;
                mul(left, right, self.options, span)?
            }
            BinaryOp::Div => {
                let right = self.visit_expr_ref(rhs)?;

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
                    // dart-sass builds this recommendation from the original
                    // (unevaluated) expression AST, so e.g. `12 / $n`
                    // recommends `math.div(12, $n)` rather than substituting
                    // $n's current value. `div_operand_source_text` covers
                    // the common shapes (Number, Variable, Paren, nested `/`)
                    // structurally, without evaluating; anything else falls
                    // back to the evaluated value's text (dart-exact for
                    // literal operands like `(1/2)`, diverges for e.g.
                    // function-call operands — narrower than dart's general
                    // AST `toString()`, see todo #159).
                    self.emit_deprecation(Deprecation::SlashDiv, span, || {
                        let left_text = match Self::div_operand_source_text(lhs, span) {
                            Some(t) => t,
                            None => left.to_css_string(span, false)?,
                        };
                        let right_text = match Self::div_operand_source_text(rhs, span) {
                            Some(t) => t,
                            None => right.to_css_string(span, false)?,
                        };
                        Ok(format!(
                            "Using / for division outside of calc() is deprecated and will be \
                             removed in Dart Sass 2.0.0.\n\nRecommendation: math.div({left_text}, \
                             {right_text}) or calc({left_text} / {right_text})\n\nMore info and \
                             automated migrator: https://sass-lang.com/d/slash-div"
                        ))
                    })?;
                }

                div(left, right, self.options, span)?
            }
            BinaryOp::Rem => {
                let right = self.visit_expr_ref(rhs)?;
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

    pub(crate) fn visit_ruleset(
        &mut self,
        ruleset: AstRuleSet<'static>,
    ) -> SassResult<Option<Value>> {
        if self.style_rule_recursion_depth >= MAX_STYLE_RULE_RECURSION_DEPTH {
            return Err(("Too much nesting.", ruleset.span).into());
        }

        self.style_rule_recursion_depth += 1;
        let result = crate::stack::maybe_grow(256 * 1024, 1024 * 1024, || {
            self.visit_ruleset_inner(ruleset)
        });
        self.style_rule_recursion_depth -= 1;

        result
    }

    fn visit_ruleset_inner(&mut self, ruleset: AstRuleSet<'static>) -> SassResult<Option<Value>> {
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
                selector_span: Some(span),
            });

            let was_in_keyframes_rule = self.flags.in_keyframes_rule();
            self.flags.set(ContextFlags::IN_KEYFRAMES_RULE, true);

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
                if let Some(ComplexSelectorComponent::Combinator(..)) = complex.components.last() {
                    return Err(("expected selector.", ruleset.selector_span).into());
                }
            }
        }

        // In plain CSS, skip parent resolution for nested rules (depth > 0)
        // and for selectors containing & at any depth. At depth 0 without &,
        // still resolve to handle @import context (e.g., a {@import "plain.css"}).
        let skip_resolution = self.is_plain_css
            && (self.plain_css_style_rule_depth > 0 || parsed_selector.contains_parent_selector());

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

        let only_nests_style_rules = !ruleset_body.is_empty()
            && ruleset_body
                .iter()
                .all(|stmt| matches!(stmt, AstStmt::RuleSet(..)));
        self.warn_for_bogus_combinators(
            &selector.as_selector_list(),
            &original_selector,
            ruleset.selector_span,
            only_nests_style_rules,
        )?;

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
}

#[cfg(test)]
mod normalize_path_tests {
    use super::Visitor;
    use std::path::Path;

    fn normalize(s: &str) -> String {
        Visitor::normalize_path(Path::new(s))
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn dot_and_dotdot_mixed() {
        assert_eq!(normalize("./a/./b"), "a/b");
    }

    #[test]
    fn dotdot_cancels_real_segment() {
        assert_eq!(normalize("a/b/../c"), "a/c");
    }

    #[test]
    fn trailing_dotdot_cancels_single_segment() {
        assert_eq!(normalize("a/.."), "");
    }

    #[test]
    fn leading_dotdot_accumulates_after_first_cancel() {
        // The first ".." cancels "a"; the second has nothing left to cancel
        // against and must accumulate rather than vanish.
        assert_eq!(normalize("a/../../b"), "../b");
    }

    #[test]
    fn bare_dotdot() {
        assert_eq!(normalize(".."), "..");
    }

    #[test]
    fn two_leading_dotdot_accumulate() {
        assert_eq!(normalize("../../a/b"), "../../a/b");
    }

    #[test]
    fn one_leading_dotdot_odd() {
        assert_eq!(normalize("../a"), "../a");
    }

    #[test]
    fn three_leading_dotdot_odd() {
        assert_eq!(normalize("../../../a"), "../../../a");
    }

    #[test]
    fn four_leading_dotdot_even() {
        assert_eq!(normalize("../../../../a"), "../../../../a");
    }

    #[test]
    fn absolute_path_dotdot_clamps_at_root() {
        // ".." above the filesystem root is a no-op, not an invalid "/.."
        assert_eq!(normalize("/../a"), "/a");
    }

    #[test]
    fn absolute_path_multiple_dotdot_clamp_at_root() {
        assert_eq!(normalize("/../../a/../b"), "/b");
    }
}
