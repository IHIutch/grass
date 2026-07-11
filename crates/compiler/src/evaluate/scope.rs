use std::rc::Rc;
use std::{cell::RefCell, mem};

use rustc_hash::FxHashMap;

fn new_scope_map<K, V>() -> FxHashMap<K, V> {
    FxHashMap::default()
}

use codemap::Spanned;

use crate::{
    ast::Mixin,
    builtin::GLOBAL_FUNCTIONS,
    common::Identifier,
    error::SassResult,
    value::{SassFunction, Value},
};

/// Scope stack for variable, mixin, and function lookups.
///
/// The outer Vec is owned directly (no Rc<RefCell<>>), eliminating 2 layers of
/// indirection per variable lookup compared to the previous design. The inner
/// `Rc<RefCell<FxHashMap>>` is retained because `new_closure()` must share map
/// instances with closures so mutations in enclosing scopes are visible.
#[allow(clippy::type_complexity)]
#[derive(Debug, Default, Clone)]
pub(crate) struct Scopes {
    variables: Vec<Rc<RefCell<FxHashMap<Identifier, Value>>>>,
    mixins: Vec<Rc<RefCell<FxHashMap<Identifier, Mixin>>>>,
    functions: Vec<Rc<RefCell<FxHashMap<Identifier, SassFunction>>>>,
    pub last_variable_index: Option<(Identifier, usize)>,
    /// Shared by all closure scopes for one compilation. It contains only empty
    /// maps, so it cannot form an environment -> pool -> environment cycle.
    pool: Rc<RefCell<ScopePool>>,
}

#[allow(clippy::type_complexity)]
#[derive(Debug, Default)]
struct ScopePool {
    variables: Vec<Rc<RefCell<FxHashMap<Identifier, Value>>>>,
    mixins: Vec<Rc<RefCell<FxHashMap<Identifier, Mixin>>>>,
    functions: Vec<Rc<RefCell<FxHashMap<Identifier, SassFunction>>>>,
    variable_vecs: Vec<Vec<Rc<RefCell<FxHashMap<Identifier, Value>>>>>,
    mixin_vecs: Vec<Vec<Rc<RefCell<FxHashMap<Identifier, Mixin>>>>>,
    function_vecs: Vec<Vec<Rc<RefCell<FxHashMap<Identifier, SassFunction>>>>>,
}

impl Scopes {
    pub fn new() -> Self {
        Self {
            // Globals stay lazy too; their three allocations are negligible, and this keeps one map-construction policy.
            variables: vec![Rc::new(RefCell::new(new_scope_map()))],
            mixins: vec![Rc::new(RefCell::new(new_scope_map()))],
            functions: vec![Rc::new(RefCell::new(new_scope_map()))],
            last_variable_index: None,
            pool: Rc::new(RefCell::new(ScopePool::default())),
        }
    }

    pub fn new_closure(&self) -> Self {
        debug_assert_eq!(self.len(), self.variables.len());

        let mut pool = self.pool.borrow_mut();
        let mut variables = pool.variable_vecs.pop().unwrap_or_default();
        let mut mixins = pool.mixin_vecs.pop().unwrap_or_default();
        let mut functions = pool.function_vecs.pop().unwrap_or_default();
        variables.extend(self.variables.iter().map(Rc::clone));
        mixins.extend(self.mixins.iter().map(Rc::clone));
        functions.extend(self.functions.iter().map(Rc::clone));
        drop(pool);

        Self {
            variables,
            mixins,
            functions,
            last_variable_index: self.last_variable_index,
            pool: Rc::clone(&self.pool),
        }
    }

    pub fn global_variables(&self) -> Rc<RefCell<FxHashMap<Identifier, Value>>> {
        debug_assert_eq!(self.len(), self.variables.len());
        Rc::clone(&self.variables[0])
    }

    pub fn global_functions(&self) -> Rc<RefCell<FxHashMap<Identifier, SassFunction>>> {
        Rc::clone(&self.functions[0])
    }

    pub fn global_mixins(&self) -> Rc<RefCell<FxHashMap<Identifier, Mixin>>> {
        Rc::clone(&self.mixins[0])
    }

    pub fn find_var(&mut self, name: Identifier) -> Option<usize> {
        debug_assert_eq!(self.len(), self.variables.len());

        match self.last_variable_index {
            Some((prev_name, idx)) if prev_name == name => return Some(idx),
            _ => {}
        };

        for (idx, scope) in self.variables.iter().enumerate().rev() {
            if scope.borrow().contains_key(&name) {
                self.last_variable_index = Some((name, idx));
                return Some(idx);
            }
        }

        None
    }

    pub fn len(&self) -> usize {
        self.variables.len()
    }

    const MAX_POOL_SIZE: usize = 32;

    pub fn enter_new_scope(&mut self) {
        debug_assert_eq!(self.len(), self.variables.len());
        let mut pool = self.pool.borrow_mut();
        let var = pool
            .variables
            .pop()
            .unwrap_or_else(|| Rc::new(RefCell::new(new_scope_map())));
        let mixin = pool
            .mixins
            .pop()
            .unwrap_or_else(|| Rc::new(RefCell::new(new_scope_map())));
        let func = pool
            .functions
            .pop()
            .unwrap_or_else(|| Rc::new(RefCell::new(new_scope_map())));
        drop(pool);
        self.variables.push(var);
        self.mixins.push(mixin);
        self.functions.push(func);
    }

    pub fn exit_scope(&mut self) {
        debug_assert_eq!(self.len(), self.variables.len());

        // The pool borrow must never span content drops: cascading Environment drops can re-enter this pool.
        if let Some(scope) = self.variables.pop() {
            if Rc::strong_count(&scope) == 1 {
                scope.borrow_mut().clear();
                let mut pool = self.pool.borrow_mut();
                if pool.variables.len() < Self::MAX_POOL_SIZE {
                    pool.variables.push(scope);
                }
            }
        }
        if let Some(scope) = self.mixins.pop() {
            if Rc::strong_count(&scope) == 1 {
                scope.borrow_mut().clear();
                let mut pool = self.pool.borrow_mut();
                if pool.mixins.len() < Self::MAX_POOL_SIZE {
                    pool.mixins.push(scope);
                }
            }
        }
        if let Some(scope) = self.functions.pop() {
            if Rc::strong_count(&scope) == 1 {
                scope.borrow_mut().clear();
                let mut pool = self.pool.borrow_mut();
                if pool.functions.len() < Self::MAX_POOL_SIZE {
                    pool.functions.push(scope);
                }
            }
        }

        self.last_variable_index = None;
    }

    /// Direct access to variable Vec for env.rs forward/import operations
    pub fn variables(&self) -> &Vec<Rc<RefCell<FxHashMap<Identifier, Value>>>> {
        &self.variables
    }

    /// Mutable access to variable Vec for env.rs forward/import operations
    pub fn variables_mut(&mut self) -> &mut Vec<Rc<RefCell<FxHashMap<Identifier, Value>>>> {
        &mut self.variables
    }

    /// Direct access to function Vec for env.rs forward/import operations
    pub fn functions_mut(&mut self) -> &mut Vec<Rc<RefCell<FxHashMap<Identifier, SassFunction>>>> {
        &mut self.functions
    }

    /// Direct access to mixin Vec for env.rs forward/import operations
    pub fn mixins_mut(&mut self) -> &mut Vec<Rc<RefCell<FxHashMap<Identifier, Mixin>>>> {
        &mut self.mixins
    }
}

impl Drop for Scopes {
    fn drop(&mut self) {
        // Clear content before borrowing the pool: cascading Environment drops can re-enter this pool.
        self.variables.clear();
        self.mixins.clear();
        self.functions.clear();

        let mut pool = self.pool.borrow_mut();
        if pool.variable_vecs.len() < Self::MAX_POOL_SIZE {
            pool.variable_vecs.push(mem::take(&mut self.variables));
        }

        if pool.mixin_vecs.len() < Self::MAX_POOL_SIZE {
            pool.mixin_vecs.push(mem::take(&mut self.mixins));
        }

        if pool.function_vecs.len() < Self::MAX_POOL_SIZE {
            pool.function_vecs.push(mem::take(&mut self.functions));
        }
    }
}

/// Variables
impl Scopes {
    pub fn insert_var(&mut self, idx: usize, name: Identifier, v: Value) -> Option<Value> {
        debug_assert_eq!(self.len(), self.variables.len());
        self.variables[idx].borrow_mut().insert(name, v)
    }

    /// Always insert this variable into the innermost scope
    ///
    /// Used, for example, for variables from `@each` and `@for`
    pub fn insert_var_last(&mut self, name: Identifier, v: Value) -> Option<Value> {
        debug_assert_eq!(self.len(), self.variables.len());
        let last_idx = self.len() - 1;
        self.last_variable_index = Some((name, last_idx));
        self.variables[last_idx].borrow_mut().insert(name, v)
    }

    pub fn get_var(&mut self, name: Spanned<Identifier>) -> SassResult<Value> {
        debug_assert_eq!(self.len(), self.variables.len());

        match self.last_variable_index {
            Some((prev_name, idx)) if prev_name == name.node => {
                return Ok(self.variables[idx].borrow()[&name.node].clone());
            }
            _ => {}
        };

        for (idx, scope) in self.variables.iter().enumerate().rev() {
            match scope.borrow().get(&name.node) {
                Some(var) => {
                    self.last_variable_index = Some((name.node, idx));
                    return Ok(var.clone());
                }
                None => continue,
            }
        }

        Err(("Undefined variable.", name.span).into())
    }

    pub fn var_exists(&self, name: Identifier) -> bool {
        debug_assert_eq!(self.len(), self.variables.len());
        for scope in self.variables.iter() {
            if scope.borrow().contains_key(&name) {
                return true;
            }
        }

        false
    }

    pub fn global_var_exists(&self, name: Identifier) -> bool {
        self.global_variables().borrow().contains_key(&name)
    }
}

/// Mixins
impl Scopes {
    pub fn insert_mixin(&mut self, name: Identifier, mixin: Mixin) {
        debug_assert_eq!(self.len(), self.variables.len());
        self.mixins
            .last_mut()
            .unwrap()
            .borrow_mut()
            .insert(name, mixin);
    }

    pub fn get_mixin(&self, name: Spanned<Identifier>) -> SassResult<Mixin> {
        debug_assert_eq!(self.len(), self.variables.len());
        for scope in self.mixins.iter().rev() {
            match scope.borrow().get(&name.node) {
                Some(mixin) => return Ok(mixin.clone()),
                None => continue,
            }
        }

        Err(("Undefined mixin.", name.span).into())
    }

    pub fn mixin_exists(&self, name: Identifier) -> bool {
        debug_assert_eq!(self.len(), self.variables.len());
        for scope in self.mixins.iter() {
            if scope.borrow().contains_key(&name) {
                return true;
            }
        }

        false
    }
}

/// Functions
impl Scopes {
    pub fn insert_fn(&mut self, func: SassFunction) {
        debug_assert_eq!(self.len(), self.variables.len());
        self.functions
            .last_mut()
            .unwrap()
            .borrow_mut()
            .insert(func.name(), func);
    }

    pub fn get_fn(&self, name: Identifier) -> Option<SassFunction> {
        debug_assert_eq!(self.len(), self.variables.len());
        for scope in self.functions.iter().rev() {
            let func = scope.borrow().get(&name).cloned();

            if func.is_some() {
                return func;
            }
        }

        None
    }

    pub fn fn_exists(&self, name: Identifier) -> bool {
        debug_assert_eq!(self.len(), self.variables.len());
        for scope in self.functions.iter() {
            if scope.borrow().contains_key(&name) {
                return true;
            }
        }

        GLOBAL_FUNCTIONS.contains_key(name.as_str())
    }
}
