use std::collections::HashMap;

use full_moon::{ast::Ast, node::Node, visitors::Visitor};
use lsp_types::InlayHint;
use slotmap::{new_key_type, SlotMap};

#[derive(Debug)]
pub struct Scope {
    pub functions: HashMap<String, Vec<(String, full_moon::tokenizer::Position)>>,
    pub parent: Option<ScopeId>,
    pub name: Option<String>,
}

impl Scope {
    pub fn new(parent: Option<ScopeId>) -> Self {
        Self {
            functions: HashMap::new(),
            parent,
            name: None,
        }
    }

    pub fn new_named(parent: Option<ScopeId>, name: String) -> Self {
        Self {
            functions: HashMap::new(),
            parent,
            name: Some(name),
        }
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

new_key_type! {
    pub struct ScopeId;
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
        self.get_current_scope_mut().map(|s| {
            s.name = Some(name.into());
        });
    }

    pub fn find_function(
        &self,
        name: &str,
    ) -> Option<(ScopeId, &[(String, full_moon::tokenizer::Position)])> {
        let Some(mut id) = self.stack.last().copied() else {
        	return None;
        };
        loop {
            let Some(scope) = self.scopes.get(id) else {
				return None;
			};
            if let Some(params) = scope.functions.get(name) {
                return Some((id, params.as_slice()));
            }
            if let Some(parent) = scope.parent {
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
