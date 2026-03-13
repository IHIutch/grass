use std::{
    cell::{Ref, RefCell, RefMut},
    collections::{HashMap, HashSet},
};

use crate::ast::CssStmt;
use crate::selector::ExtendedSelector;

#[derive(Debug, Clone)]
pub(super) struct CssTree {
    // None is tombstone
    stmts: Vec<RefCell<Option<CssStmt>>>,
    pub parent_to_child: HashMap<CssTreeIdx, Vec<CssTreeIdx>>,
    pub child_to_parent: HashMap<CssTreeIdx, CssTreeIdx>,
    /// Nodes hidden from output but preserved for cloning (load-css templates).
    hidden: HashSet<CssTreeIdx>,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, PartialOrd, Ord)]
#[repr(transparent)]
pub(super) struct CssTreeIdx(usize);

impl CssTree {
    pub const ROOT: CssTreeIdx = CssTreeIdx(0);

    pub fn new() -> Self {
        let mut tree = Self {
            stmts: Vec::new(),
            parent_to_child: HashMap::new(),
            child_to_parent: HashMap::new(),
            hidden: HashSet::new(),
        };

        tree.stmts.push(RefCell::new(None));

        tree
    }

    pub fn get(&self, idx: CssTreeIdx) -> Ref<'_, Option<CssStmt>> {
        self.stmts[idx.0].borrow()
    }

    pub fn get_mut(&self, idx: CssTreeIdx) -> RefMut<'_, Option<CssStmt>> {
        self.stmts[idx.0].borrow_mut()
    }

    pub fn finish(self) -> Vec<CssStmt> {
        // Collect all hidden nodes and their descendants.
        let mut all_hidden = self.hidden.clone();
        if !all_hidden.is_empty() {
            let mut stack: Vec<CssTreeIdx> = self.hidden.iter().copied().collect();
            while let Some(idx) = stack.pop() {
                if let Some(children) = self.parent_to_child.get(&idx) {
                    for &child in children {
                        if all_hidden.insert(child) {
                            stack.push(child);
                        }
                    }
                }
            }
        }

        let mut idx = 1;

        while idx < self.stmts.len() - 1 {
            if all_hidden.contains(&CssTreeIdx(idx))
                || self.stmts[idx].borrow().is_none()
                || !self.has_children(CssTreeIdx(idx))
            {
                idx += 1;
                continue;
            }

            self.apply_children(CssTreeIdx(idx));

            idx += 1;
        }

        self.stmts
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !all_hidden.contains(&CssTreeIdx(*i)))
            .filter_map(|(_, cell)| RefCell::into_inner(cell))
            .collect()
    }

    fn apply_children(&self, parent: CssTreeIdx) {
        for &child in &self.parent_to_child[&parent] {
            if self.has_children(child) {
                self.apply_children(child);
            }

            match self.stmts[child.0].borrow_mut().take() {
                Some(child) => self.add_child_to_parent(child, parent),
                None => continue,
            };
        }
    }

    fn has_children(&self, parent: CssTreeIdx) -> bool {
        self.parent_to_child.contains_key(&parent)
    }

    fn add_child_to_parent(&self, child: CssStmt, parent_idx: CssTreeIdx) {
        RefMut::map(self.stmts[parent_idx.0].borrow_mut(), |parent| {
            match parent {
                Some(CssStmt::RuleSet { body, .. }) => body.push(child),
                Some(CssStmt::Style(..) | CssStmt::Comment(..) | CssStmt::Import(..)) | None => {
                    unreachable!()
                }
                Some(CssStmt::Media(media, ..)) => {
                    media.body.push(child);
                }
                Some(CssStmt::UnknownAtRule(at_rule, ..)) => {
                    at_rule.body.push(child);
                }
                Some(CssStmt::Supports(supports, ..)) => {
                    supports.body.push(child);
                }
                Some(CssStmt::KeyframesRuleSet(keyframes)) => {
                    keyframes.body.push(child);
                }
            };
            parent
        });
    }

    pub fn add_child(&mut self, child: CssStmt, parent_idx: CssTreeIdx) -> CssTreeIdx {
        let child_idx = self.add_stmt_inner(child);
        self.parent_to_child
            .entry(parent_idx)
            .or_default()
            .push(child_idx);
        self.child_to_parent.insert(child_idx, parent_idx);
        child_idx
    }

    pub fn link_child_to_parent(&mut self, child_idx: CssTreeIdx, parent_idx: CssTreeIdx) {
        self.parent_to_child
            .entry(parent_idx)
            .or_default()
            .push(child_idx);
        self.child_to_parent.insert(child_idx, parent_idx);
    }

    pub fn has_following_sibling(&self, child: CssTreeIdx) -> bool {
        if child == Self::ROOT {
            return false;
        }

        let parent_idx = self.child_to_parent.get(&child).unwrap();

        let parent_children = self.parent_to_child.get(parent_idx).unwrap();

        // Check if any sibling after `child` would produce visible output.
        // We skip siblings that are empty container statements (media, supports,
        // rulesets) with no children in the tree, since those won't produce
        // any output. Note: during tree building, CssStmt bodies are always
        // empty — actual children are tracked in parent_to_child, so we check
        // that map instead of is_invisible().
        let mut found_child = false;
        for &sibling in parent_children {
            if sibling == child {
                found_child = true;
                continue;
            }
            if !found_child {
                continue;
            }
            // This is a sibling after `child`. Check if it would be visible.
            let stmt = self.stmts[sibling.0].borrow();
            match &*stmt {
                None => {
                    // Tombstone — represents a moved/merged node.
                    // Conservatively treat as a visible sibling.
                    return true;
                }
                Some(s) => {
                    if self.is_stmt_visible(sibling, s) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check if a statement would produce visible output during tree building.
    /// Unlike is_invisible(), this checks the parent_to_child map for children
    /// since CssStmt bodies are not populated until finish().
    pub(crate) fn is_stmt_visible(&self, idx: CssTreeIdx, stmt: &CssStmt) -> bool {
        match stmt {
            CssStmt::Media(..) | CssStmt::Supports(..) => {
                // A media/supports rule is visible if it has any visible children
                self.has_visible_child(idx)
            }
            CssStmt::RuleSet { selector, .. } => {
                // A ruleset is visible if its selector is visible and it has visible children
                !selector.is_invisible() && self.has_visible_child(idx)
            }
            // Styles, comments, imports, unknown at-rules, keyframes are always visible
            _ => true,
        }
    }

    /// Recursively check if a node has any visible children in the tree.
    pub(crate) fn has_visible_child(&self, idx: CssTreeIdx) -> bool {
        let Some(children) = self.parent_to_child.get(&idx) else {
            return false;
        };
        children.iter().any(|&child_idx| {
            let stmt = self.stmts[child_idx.0].borrow();
            match &*stmt {
                None => false, // tombstone
                Some(s) => self.is_stmt_visible(child_idx, s),
            }
        })
    }

    /// Check if the last child of `grandparent_idx` is a media rule with the
    /// same query as the media rule at `parent_idx`. Used for merging consecutive
    /// siblings with matching media queries after bubbling.
    pub fn last_matching_media_sibling(
        &self,
        parent_idx: CssTreeIdx,
        grandparent_idx: CssTreeIdx,
    ) -> Option<CssTreeIdx> {
        let children = self.parent_to_child.get(&grandparent_idx)?;
        let &last = children.last()?;

        if last == parent_idx {
            return None;
        }

        let parent_stmt = self.stmts[parent_idx.0].borrow();
        let last_stmt = self.stmts[last.0].borrow();

        match (&*parent_stmt, &*last_stmt) {
            (Some(CssStmt::Media(parent_media, _)), Some(CssStmt::Media(last_media, _))) => {
                if parent_media.query == last_media.query {
                    Some(last)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn add_stmt(&mut self, child: CssStmt, parent: Option<CssTreeIdx>) -> CssTreeIdx {
        match parent {
            Some(parent) => self.add_child(child, parent),
            None => self.add_child(child, Self::ROOT),
        }
    }

    /// Returns the number of children currently under `parent`.
    pub fn child_count(&self, parent: CssTreeIdx) -> usize {
        self.parent_to_child
            .get(&parent)
            .map_or(0, |children| children.len())
    }

    /// Move children of `from_parent` (starting at index `start`) to `to_parent`.
    /// Used to re-parent CSS nodes that were added to ROOT during module
    /// evaluation but need to be nested under a different parent (e.g., for
    /// nested @import).
    pub fn reparent_children(
        &mut self,
        from_parent: CssTreeIdx,
        to_parent: CssTreeIdx,
        start: usize,
    ) {
        let children_to_move: Vec<CssTreeIdx> = self
            .parent_to_child
            .get(&from_parent)
            .map_or_else(Vec::new, |children| children[start..].to_vec());

        if children_to_move.is_empty() {
            return;
        }

        // Remove from old parent
        if let Some(children) = self.parent_to_child.get_mut(&from_parent) {
            children.truncate(start);
        }

        // Add to new parent and update child_to_parent
        for child_idx in children_to_move {
            self.parent_to_child
                .entry(to_parent)
                .or_default()
                .push(child_idx);
            self.child_to_parent.insert(child_idx, to_parent);
        }
    }

    /// Returns the CssTreeIdx values of children under `parent`, starting at
    /// index `start`. Used to identify which ROOT children were added during
    /// a module's execution.
    pub fn root_children_from(&self, start: usize) -> Vec<CssTreeIdx> {
        self.parent_to_child
            .get(&Self::ROOT)
            .map_or_else(Vec::new, |children| {
                children[start..].to_vec()
            })
    }

    /// Deep-clone a subtree rooted at `idx` into a new parent.
    /// RuleSet selectors get new independent ExtendedSelectors so that
    /// @extend mutations don't bleed between the original and clone.
    /// Returns (new_root_idx, mapping from old ExtendedSelector Rc ptrs to new ones).
    pub fn clone_subtree(
        &mut self,
        idx: CssTreeIdx,
        new_parent: CssTreeIdx,
        selector_map: &mut HashMap<usize, ExtendedSelector>,
    ) -> CssTreeIdx {
        let cloned_stmt = {
            let stmt = self.stmts[idx.0].borrow();
            match &*stmt {
                None => {
                    // Tombstone — skip it
                    return idx;
                }
                Some(CssStmt::RuleSet { selector, is_group_end, .. }) => {
                    let old_ptr = selector.rc_ptr();
                    let new_selector = ExtendedSelector::new(selector.as_selector_list().clone());
                    selector_map.insert(old_ptr, new_selector.clone());
                    CssStmt::RuleSet {
                        selector: new_selector,
                        body: Vec::new(),
                        is_group_end: *is_group_end,
                        source_span: None,
                    }
                }
                Some(other) => other.clone(),
            }
        };

        let new_idx = self.add_child(cloned_stmt, new_parent);

        // Recursively clone children
        let children: Vec<CssTreeIdx> = self
            .parent_to_child
            .get(&idx)
            .cloned()
            .unwrap_or_default();

        for child_idx in children {
            self.clone_subtree(child_idx, new_idx, selector_map);
        }

        new_idx
    }

    /// Check if a node is in the hidden set.
    pub fn is_hidden(&self, idx: CssTreeIdx) -> bool {
        self.hidden.contains(&idx)
    }

    /// Mark a node as hidden. It will be excluded from finish() output.
    pub fn hide(&mut self, idx: CssTreeIdx) {
        self.hidden.insert(idx);
    }

    /// Clone a subtree into a hidden area (no parent in the visible tree).
    /// The cloned nodes are marked hidden and won't appear in finish() output,
    /// but remain available for future clone_subtree calls.
    pub fn clone_subtree_hidden(
        &mut self,
        idx: CssTreeIdx,
        selector_map: &mut HashMap<usize, ExtendedSelector>,
    ) -> CssTreeIdx {
        let cloned_stmt = {
            let stmt = self.stmts[idx.0].borrow();
            match &*stmt {
                None => return idx,
                Some(CssStmt::RuleSet {
                    selector,
                    is_group_end,
                    ..
                }) => {
                    let old_ptr = selector.rc_ptr();
                    let new_selector =
                        ExtendedSelector::new(selector.as_selector_list().clone());
                    selector_map.insert(old_ptr, new_selector.clone());
                    CssStmt::RuleSet {
                        selector: new_selector,
                        body: Vec::new(),
                        is_group_end: *is_group_end,
                        source_span: None,
                    }
                }
                Some(other) => other.clone(),
            }
        };

        let new_idx = self.add_stmt_inner(cloned_stmt);
        self.hidden.insert(new_idx);

        // Recursively clone children
        let children: Vec<CssTreeIdx> = self
            .parent_to_child
            .get(&idx)
            .cloned()
            .unwrap_or_default();

        for child_idx in children {
            let cloned_child = self.clone_subtree_hidden(child_idx, selector_map);
            self.parent_to_child
                .entry(new_idx)
                .or_default()
                .push(cloned_child);
            self.child_to_parent.insert(cloned_child, new_idx);
        }

        new_idx
    }

    fn add_stmt_inner(&mut self, stmt: CssStmt) -> CssTreeIdx {
        let idx = CssTreeIdx(self.stmts.len());
        self.stmts.push(RefCell::new(Some(stmt)));

        idx
    }
}
