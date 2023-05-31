use std::collections::HashMap;

use full_moon::{
    ast::{Ast, Value},
    node::Node,
    visitors::Visitor,
};
use linked_hash_map::LinkedHashMap;
use lsp_types::InlayHint;
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct ScopeId;
    pub struct VarId;
    pub struct ValueId;
}

#[derive(Debug, Clone, Copy)]
pub enum Var {
    Local(ValueId),
    Reference(ScopeId, VarId),
}

#[derive(Debug)]
pub struct Scope {
    pub value_arena: SlotMap<ValueId, Value>,
    pub var_arena: SlotMap<VarId, Var>,
    pub var_names: LinkedHashMap<String, VarId>,
    pub parent: Option<ScopeId>,
    pub name: Option<String>,
}

impl Scope {
    pub fn new(parent: Option<ScopeId>) -> Self {
        Self {
            value_arena: SlotMap::with_key(),
            var_arena: SlotMap::with_key(),
            var_names: LinkedHashMap::new(),
            parent,
            name: None,
        }
    }

    pub fn new_named(parent: Option<ScopeId>, name: String) -> Self {
        Self {
            value_arena: SlotMap::with_key(),
            var_arena: SlotMap::with_key(),
            var_names: LinkedHashMap::new(),
            parent,
            name: Some(name),
        }
    }

    pub fn alloc_value(&mut self, value: Value) -> ValueId {
        self.value_arena.insert(value)
    }

    pub fn alloc_var(&mut self, name: String, var: Var) -> VarId {
        let id = self.var_arena.insert(var);
        self.var_names.insert(name, id);
        id
    }

    pub fn alloc_local(&mut self, name: String, value: Value) -> VarId {
        let id = self.alloc_value(value);
        self.alloc_var(name, Var::Local(id))
    }

    #[allow(unused)]
    pub fn alloc_reference(&mut self, name: String, scope: ScopeId, var: VarId) -> VarId {
        self.alloc_var(name, Var::Reference(scope, var))
    }

    #[allow(unused)]
    pub fn get_var_id(&self, name: &str) -> Option<VarId> {
        self.var_names.get(name).copied()
    }

    #[allow(unused)]
    pub fn get_var(&self, name: &str) -> Option<&Var> {
        self.get_var_id(name).and_then(|id| self.var_arena.get(id))
    }

    #[allow(unused)]
    pub fn get_var_mut(&mut self, name: &str) -> Option<&mut Var> {
        self.get_var_id(name)
            .and_then(move |id| self.var_arena.get_mut(id))
    }

    #[allow(unused)]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[allow(unused)]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

pub struct ScopeManager {
    pub(crate) ast: Ast,
    pub(crate) scopes: SlotMap<ScopeId, Scope>,
    pub(crate) stack: Vec<ScopeId>,
    pub(crate) node_refs: HashMap<usize, ScopeId>,
    pub(crate) hints: Vec<InlayHint>,
    pub(crate) name_stack: Vec<String>,
}

impl ScopeManager {
    pub fn new(ast: Ast) -> Self {
        let mut scopes = SlotMap::with_key();
        let global = scopes.insert(Scope::new_named(None, "global".to_string()));
        let mut new = Self {
            ast,
            scopes,
            stack: vec![global],
            node_refs: HashMap::new(),
            hints: vec![],
            name_stack: vec![],
        };
        // Safety: We're not modifying the AST and the pointer will remain valid throughout the pass as the
        // manager owns the AST.
        new.visit_ast(unsafe { (&new.ast as *const Ast).as_ref().unwrap_unchecked() });
        new
    }

    #[allow(unused)]
    pub fn name_current_scope(&mut self, name: impl Into<String>) {
        if let Some(s) = self.get_current_scope_mut() { s.name = Some(name.into()); }
    }

    pub fn get_value(&self, scope: ScopeId, value: ValueId) -> Option<&Value> {
        self.scopes.get(scope)?.value_arena.get(value)
    }

    pub fn resolve_reference(&self, scope: ScopeId, var: VarId) -> Option<(ScopeId, ValueId)> {
        let v = self.scopes.get(scope)?.var_arena.get(var)?;
        match v {
            Var::Reference(scope, var) => self.resolve_reference(*scope, *var),
            Var::Local(value) => Some((scope, *value)),
        }
    }

    pub fn find_var(&self, name: &str) -> Option<Var> {
        let Some(mut id) = self.stack.last().copied() else {
        	return None;
        };
        let mut original = true;
        loop {
            let Some(scope) = self.scopes.get(id) else {
				return None;
			};
            if let Some(var) = scope.var_names.get(name) {
                if original {
                    let var = scope.var_arena.get(*var)?;
                    return Some(*var);
                } else {
                    return Some(Var::Reference(id, *var));
                }
            }
            if let Some(parent) = scope.parent {
                original = false;
                id = parent;
            } else {
                break;
            };
        }
        None
    }

    #[allow(unused)]
    pub fn open_scope_named(&mut self, name: impl Into<String>, node: *const dyn Node) -> ScopeId {
        let scope = self
            .scopes
            .insert(Scope::new_named(self.stack.last().copied(), name.into()));
        self.node_refs.insert(node as *const () as usize, scope);
        self.stack.push(scope);
        scope
    }

    pub fn open_scope(&mut self, node: *const dyn Node) -> ScopeId {
        let scope = if let Some(name) = self.name_stack.pop() {
            self.scopes
                .insert(Scope::new_named(self.stack.last().copied(), name))
        } else {
            self.scopes.insert(Scope::new(self.stack.last().copied()))
        };
        self.node_refs
            .insert(node as *const dyn Node as *const () as usize, scope);
        self.stack.push(scope);
        scope
    }

    pub fn name_next_scope(&mut self, name: impl Into<String>) {
        self.name_stack.push(name.into());
    }

    pub fn close_scope(&mut self) {
        self.stack.pop();
    }

    #[allow(unused)]
    pub fn get_scope_id(&self, node: *const dyn Node) -> Option<ScopeId> {
        self.node_refs.get(&(node as *const () as usize)).copied()
    }

    #[allow(unused)]
    pub fn get_scope(&self, id: ScopeId) -> Option<&Scope> {
        self.scopes.get(id)
    }

    #[allow(unused)]
    pub fn get_scope_mut(&mut self, id: ScopeId) -> Option<&mut Scope> {
        self.scopes.get_mut(id)
    }

    #[allow(unused)]
    pub fn get_current_scope(&self) -> Option<&Scope> {
        self.stack.last().and_then(|id| self.scopes.get(*id))
    }

    pub fn get_current_scope_mut(&mut self) -> Option<&mut Scope> {
        self.stack.last().and_then(|id| self.scopes.get_mut(*id))
    }

    #[allow(unused)]
    pub fn get_current_scope_id(&self) -> Option<ScopeId> {
        self.stack.last().copied()
    }
}
