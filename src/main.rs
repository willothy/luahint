use std::collections::HashMap;

use std::sync::atomic::{AtomicI32, Ordering};

use crop::Rope;
use dashmap::DashMap;
use full_moon::ast::{
    Ast, Call, Expression, FunctionArgs, FunctionCall, FunctionDeclaration, Suffix, Value,
};
use full_moon::node::Node;
use full_moon::visitors::Visitor;
use slotmap::{new_key_type, SlotMap};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::{self, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Doc {
    text: RwLock<Rope>,
    #[allow(unused)]
    uri: Url,
    version: AtomicI32,
}

#[derive(Debug)]
struct Backend {
    #[allow(unused)]
    client: Client,
    documents: DashMap<Url, Doc>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: env!("CARGO_PKG_NAME").to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            capabilities: ServerCapabilities {
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["custom.notification".to_string()],
                    work_done_progress_options: Default::default(),
                }),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),

                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                inlay_hint_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            ..Default::default()
        })
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let text = params.text_document.text;
        let rope = Rope::from(text);
        self.documents.insert(
            params.text_document.uri.clone(),
            Doc {
                text: RwLock::new(rope),
                uri: params.text_document.uri,
                version: AtomicI32::new(params.text_document.version),
            },
        );
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(doc) = self.documents.get(&uri) else {
			return
		};
        let mut text = doc.text.write().await;
        params.content_changes.into_iter().for_each(|change| {
            let Some(range) = change.range else {
				return
			};
            let start_byte =
                text.byte_of_line(range.start.line as usize) + range.start.character as usize;
            let end_byte =
                text.byte_of_line(range.end.line as usize) + range.end.character as usize;
            text.replace(start_byte..end_byte, change.text);
        });
        doc.version
            .swap(params.text_document.version, Ordering::Relaxed);
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let Some(doc) = self.documents.get(&params.text_document.uri) else {
			return Ok(None)
		};
        let text = doc.text.read().await.to_string();
        let ast = full_moon::parse(&text).map_err(|_| jsonrpc::Error::internal_error())?;
        let hints = HintManager::get_hints(&ast);

        Ok(Some(hints))
    }

    async fn inlay_hint_resolve(&self, params: InlayHint) -> Result<InlayHint> {
        Ok(params)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn execute_command(
        &self,
        _params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }
}

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

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

new_key_type! {
    pub struct ScopeId;
}

pub struct ScopeManager {
    scopes: SlotMap<ScopeId, Scope>,
    stack: Vec<ScopeId>,
    // Safety: ScopeManager will always be owned by the PassManager, which owns the AST,
    // so these pointers will always be valid. They're only pointers as references don't implement
    // Hash
    node_refs: HashMap<usize, ScopeId>,
    hints: Vec<InlayHint>,
    name_stack: Vec<String>,
}

impl ScopeManager {
    pub fn new() -> Self {
        let mut scopes = SlotMap::with_key();
        let global = scopes.insert(Scope::new_named(None, "global".to_string()));
        let new = Self {
            scopes,
            stack: vec![global],
            node_refs: HashMap::new(),
            hints: vec![],
            name_stack: vec![],
        };
        new
    }

    pub fn init(&mut self, ast: &Ast) {
        self.visit_ast(ast);
    }

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

    pub fn open_scope_named(
        &mut self,
        name: impl Into<String>,
        node: *const dyn full_moon::node::Node,
    ) -> ScopeId {
        let scope = self
            .scopes
            .insert(Scope::new_named(self.stack.last().copied(), name.into()));
        self.node_refs.insert(node as *const () as usize, scope);
        self.stack.push(scope);
        scope
    }

    pub fn open_scope(&mut self, node: *const dyn full_moon::node::Node) -> ScopeId {
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

    pub fn get_scope(&self, node: &dyn full_moon::node::Node) -> Option<&Scope> {
        self.node_refs
            .get(&(node as *const dyn full_moon::node::Node as *const () as usize))
            .and_then(|id| self.scopes.get(*id))
    }

    pub fn get_scope_mut(&mut self, node: &dyn full_moon::node::Node) -> Option<&mut Scope> {
        self.node_refs
            .get(&(node as *const dyn full_moon::node::Node as *const () as usize))
            .and_then(|id| self.scopes.get_mut(*id))
    }

    pub fn get_scope_id(&self, node: *const dyn full_moon::node::Node) -> Option<ScopeId> {
        self.node_refs.get(&(node as *const () as usize)).copied()
    }

    pub fn get_scope_by_id(&self, id: ScopeId) -> Option<&Scope> {
        self.scopes.get(id)
    }

    pub fn get_scope_by_id_mut(&mut self, id: ScopeId) -> Option<&mut Scope> {
        self.scopes.get_mut(id)
    }

    pub fn get_current_scope(&self) -> Option<&Scope> {
        self.stack.last().and_then(|id| self.scopes.get(*id))
    }

    pub fn get_current_scope_mut(&mut self) -> Option<&mut Scope> {
        self.stack.last().and_then(|id| self.scopes.get_mut(*id))
    }

    pub fn get_current_scope_id(&self) -> Option<ScopeId> {
        self.stack.last().copied()
    }
}

impl Visitor for ScopeManager {
    fn visit_block(&mut self, block: &full_moon::ast::Block) {
        self.open_scope(block);
    }

    fn visit_block_end(&mut self, _node: &full_moon::ast::Block) {
        self.close_scope();
    }

    fn visit_local_function(&mut self, func: &full_moon::ast::LocalFunction) {
        let Some(scope) = self.get_current_scope_mut() else {
			return
		};
        let name = func.name().to_string();
        let body = func.body();
        let params = body
            .parameters()
            .iter()
            .map(|param| {
                (
                    param.to_string(),
                    param.start_position().unwrap_or_default(),
                )
            })
            .collect();
        scope.functions.insert(name.clone(), params);
        self.name_next_scope(name);
    }

    fn visit_function_declaration(&mut self, node: &FunctionDeclaration) {
        let Some(global_id) = self.stack.first() else {
			return
		};
        let Some(scope) = self.scopes.get_mut(*global_id) else {
			return
		};
        let name = node.name().to_string().trim().to_string();
        let body = node.body();
        let params = body
            .parameters()
            .iter()
            .map(|param| {
                (
                    param.to_string(),
                    param.start_position().unwrap_or_default(),
                )
            })
            .collect();
        scope.functions.insert(name.clone(), params);
        self.stack.push(*global_id);
        self.name_next_scope(name);
    }

    fn visit_function_declaration_end(&mut self, _node: &FunctionDeclaration) {
        self.stack.pop();
    }

    fn visit_assignment(&mut self, node: &full_moon::ast::Assignment) {
        let Some(global_id) = self.stack.first().copied() else {
			return
		};

        self.stack.push(global_id);
        node.variables()
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .zip(node.expressions().into_iter())
            .for_each(|(v, e)| match v {
                full_moon::ast::Var::Name(name) => match e {
                    Expression::Value { value } => match value.as_ref() {
                        Value::Function((_, f)) => {
                            let Some(scope) = self.scopes.get_mut(global_id) else {
								return
							};
                            let name = name.to_string().trim().to_string();
                            let params = f
                                .parameters()
                                .iter()
                                .map(|param| {
                                    (
                                        param.to_string(),
                                        param.start_position().unwrap_or_default(),
                                    )
                                })
                                .collect();

                            scope.functions.insert(name.clone(), params);
                            self.name_next_scope(name);
                        }
                        _ => {}
                    },
                    _ => {}
                },
                _ => {}
            });
    }

    fn visit_assignment_end(&mut self, _node: &full_moon::ast::Assignment) {
        self.close_scope();
    }

    fn visit_local_assignment(&mut self, node: &full_moon::ast::LocalAssignment) {
        node.names()
            .into_iter()
            .zip(node.expressions().into_iter())
            .for_each(|(name, e)| match e {
                Expression::Value { value } => match value.as_ref() {
                    Value::Function((_, f)) => {
                        let Some(scope) = self.get_current_scope_mut() else {
							return
						};
                        let name = name.to_string().trim().to_string();
                        let params = f
                            .parameters()
                            .iter()
                            .map(|param| {
                                (
                                    param.to_string(),
                                    param.start_position().unwrap_or_default(),
                                )
                            })
                            .collect();

                        scope.functions.insert(name.clone(), params);
                        self.name_next_scope(name);
                    }
                    _ => {}
                },
                _ => {}
            });
    }

    fn visit_local_assignment_end(&mut self, _node: &full_moon::ast::LocalAssignment) {}

    fn visit_function_call(&mut self, node: &FunctionCall) {
        let (_, params) = match node.prefix() {
            full_moon::ast::Prefix::Name(n) => {
                let name = n.to_string().trim().to_string();
                if let Some((_, params)) = self.find_function(&name) {
                    (name, params.to_vec())
                } else {
                    return;
                }
            }
            _ => return,
        };

        node.suffixes().into_iter().next().map(|s| match s {
            Suffix::Call(Call::AnonymousCall(FunctionArgs::Parentheses { arguments, .. })) => {
                arguments
                    .iter()
                    .zip(params)
                    .map(|(param, (name, _))| (name, param.start_position().unwrap_or_default()))
                    .for_each(|(name, pos)| {
                        self.hints.push(InlayHint {
                            position: lsp_types::Position {
                                line: pos.line() as u32,
                                character: pos.character() as u32,
                            },
                            label: InlayHintLabel::String(name.clone()),
                            kind: Some(InlayHintKind::PARAMETER),
                            text_edits: None,
                            tooltip: None,
                            padding_left: None,
                            padding_right: None,
                            data: None,
                        });
                    });
            }
            _ => {}
        });
    }
}

pub struct HintManager {
    pub ast: Ast,
    pub scope_manager: ScopeManager,
}

impl HintManager {
    pub fn get_hints(ast: &Ast) -> Vec<InlayHint> {
        let mut m = ScopeManager::new();
        m.init(&ast);
        m.hints
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());

    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: DashMap::new(),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
