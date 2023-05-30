use std::collections::HashMap;
use std::time::Duration;

use full_moon::ast::{
    Call, Expression, FunctionCall, FunctionDeclaration, FunctionName, MethodCall, Suffix, Value,
};
use full_moon::visitors::Visitor;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::notification::{Notification, Progress};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
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
                ..ServerCapabilities::default()
            },
            ..Default::default()
        })
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        // let mut hints = vec![];

        todo!()
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        if params.command == "custom.notification" {
            self.client
                .send_notification::<Progress>(ProgressParams {
                    token: ProgressToken::String("testing".to_string()),
                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin({
                        let mut prog = WorkDoneProgressBegin::default();
                        prog.message = Some("eeeeeeee".to_string());
                        prog.percentage = Some(69);
                        prog.title = "Testing".to_string();
                        prog
                    })),
                })
                .await;
            let start = std::time::Instant::now();
            let duration = Duration::from_secs(5);
            let mut current = std::time::Instant::now();
            while current - start < duration {
                self.client
                    .send_notification::<Progress>(ProgressParams {
                        token: ProgressToken::String("testing".to_string()),
                        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                            WorkDoneProgressReport {
                                message: Some("Testing".to_string()),
                                percentage: Some(
                                    ((current - start).as_secs_f32() / duration.as_secs_f32()
                                        * 100.0)
                                        .round() as u32,
                                ),
                                cancellable: None,
                            },
                        )),
                    })
                    .await;
                tokio::time::sleep(Duration::from_millis(150)).await;
                current = std::time::Instant::now();
            }
            self.client
                .send_notification::<Progress>(ProgressParams {
                    token: ProgressToken::String("testing".to_string()),
                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::End({
                        let mut prog = WorkDoneProgressEnd::default();
                        prog.message = Some("End af!".to_string());
                        prog
                    })),
                })
                .await;
            Ok(None)
        } else {
            Err(Error::invalid_request())
        }
    }
}

pub struct Scope<'a> {
    pub variables: HashMap<String, &'a Value>,
    pub parent: Option<ScopeId>,
}

impl<'a> Scope<'a> {
    pub fn new(parent: Option<ScopeId>) -> Self {
        Self {
            variables: HashMap::new(),
            parent,
        }
    }

    pub fn new_with_parent(parent: ScopeId) -> Self {
        Self {
            variables: HashMap::new(),
            parent: Some(parent),
        }
    }
}

new_key_type! {
    pub struct ScopeId;
}

struct HintCollector<'a> {
    scopes: SlotMap<ScopeId, Scope<'a>>,
    scope_stack: Vec<ScopeId>,
    global: ScopeId,
    node_scopes: HashMap<*const dyn full_moon::node::Node, ScopeId>,
}

impl<'a> HintCollector<'a> {
    pub fn new() -> Self {
        let mut scopes = SlotMap::with_key();
        let global = scopes.insert(Scope::new(None));
        Self {
            scopes,
            global,
            scope_stack: Vec::new(),
            node_scopes: HashMap::new(),
        }
    }
}

impl<'a> Visitor for HintCollector<'a> {
    fn visit_anonymous_call(&mut self, _node: &full_moon::ast::FunctionArgs) {}

    fn visit_assignment(&mut self, node: &full_moon::ast::Assignment) {
        fn val_inner<'b>(h: &'b mut HintCollector, v: &Expression) -> Option<&'b Value> {
            match v {
                full_moon::ast::Expression::Parentheses { expression, .. } => {
                    val_inner(h, &expression)
                }
                full_moon::ast::Expression::Value { value } => match value.as_ref() {
                    Value::Function(_) => todo!(),
                    Value::TableConstructor(_) => todo!(),
                    Value::ParenthesesExpression(_) => todo!(),
                    Value::Var(_) => todo!(),
                    _ => None,
                },
                _ => None,
            }
        }

        for (n, v) in node
            .variables()
            .into_iter()
            .zip(node.expressions().into_iter())
        {
            match n {
                full_moon::ast::Var::Expression(e) => match e.prefix() {
                    full_moon::ast::Prefix::Expression(_) => todo!(),
                    full_moon::ast::Prefix::Name(_) => todo!(),
                    _ => todo!(),
                },
                full_moon::ast::Var::Name(_) => todo!(),
                _ => todo!(),
            }
            if let Some(val) = val_inner(self, v) {
                self.scopes
                    .get_mut(*self.scope_stack.last().unwrap())
                    .unwrap()
                    .variables
                    .insert(n.name().to_string(), val);
            }
        }
    }

    fn visit_block(&mut self, node: &full_moon::ast::Block) {
        let scope = self
            .scopes
            .insert(Scope::new(self.scope_stack.last().cloned()));
        self.scope_stack.push(scope);
        for stmt in node.stmts() {
            self.visit_stmt(stmt);
        }
        self.scope_stack.pop();
        self.node_scopes.insert(node, scope);
    }

    fn visit_stmt(&mut self, node: &full_moon::ast::Stmt) {
        match node {
            full_moon::ast::Stmt::Assignment(a) => self.visit_assignment(a),
            full_moon::ast::Stmt::FunctionDeclaration(f) => self.visit_function_declaration(f),
            full_moon::ast::Stmt::Do(d) => self.visit_do(d),
            full_moon::ast::Stmt::LocalAssignment(a) => self.visit_local_assignment(a),
            full_moon::ast::Stmt::LocalFunction(f) => self.visit_local_function(f),
            full_moon::ast::Stmt::If(i) => self.visit_if(i),
            full_moon::ast::Stmt::NumericFor(f) => self.visit_numeric_for(f),
            full_moon::ast::Stmt::GenericFor(f) => self.visit_generic_for(f),
            full_moon::ast::Stmt::Repeat(r) => self.visit_repeat(r),
            full_moon::ast::Stmt::While(w) => self.visit_while(w),
            _ => {}
        }
    }

    fn visit_local_assignment(&mut self, _node: &full_moon::ast::LocalAssignment) {}

    fn visit_local_function(&mut self, _node: &full_moon::ast::LocalFunction) {}

    fn visit_call(&mut self, _node: &Call) {}

    fn visit_contained_span(&mut self, _node: &full_moon::ast::span::ContainedSpan) {}

    fn visit_do(&mut self, _node: &full_moon::ast::Do) {}

    fn visit_else_if(&mut self, _node: &full_moon::ast::ElseIf) {}

    fn visit_eof(&mut self, _node: &full_moon::tokenizer::TokenReference) {}

    fn visit_expression(&mut self, _node: &full_moon::ast::Expression) {}

    fn visit_field(&mut self, _node: &full_moon::ast::Field) {}

    fn visit_function_args(&mut self, _node: &full_moon::ast::FunctionArgs) {}

    fn visit_function_body(&mut self, _node: &full_moon::ast::FunctionBody) {}

    fn visit_function_call(&mut self, _node: &FunctionCall) {}

    fn visit_function_declaration(&mut self, _node: &FunctionDeclaration) {}

    fn visit_function_name(&mut self, _node: &FunctionName) {}

    fn visit_generic_for(&mut self, _node: &full_moon::ast::GenericFor) {}

    fn visit_if(&mut self, _node: &full_moon::ast::If) {}

    fn visit_index(&mut self, _node: &full_moon::ast::Index) {}

    fn visit_last_stmt(&mut self, _node: &full_moon::ast::LastStmt) {}

    fn visit_method_call(&mut self, _node: &MethodCall) {}

    fn visit_numeric_for(&mut self, _node: &full_moon::ast::NumericFor) {}

    fn visit_parameter(&mut self, _node: &full_moon::ast::Parameter) {}

    fn visit_prefix(&mut self, _node: &full_moon::ast::Prefix) {}

    fn visit_return(&mut self, _node: &full_moon::ast::Return) {}

    fn visit_repeat(&mut self, _node: &full_moon::ast::Repeat) {}

    fn visit_suffix(&mut self, _node: &Suffix) {}

    fn visit_table_constructor(&mut self, _node: &full_moon::ast::TableConstructor) {}

    fn visit_token_reference(&mut self, _node: &full_moon::tokenizer::TokenReference) {}

    fn visit_un_op(&mut self, _node: &full_moon::ast::UnOp) {}

    fn visit_value(&mut self, _node: &Value) {}

    fn visit_var(&mut self, _node: &full_moon::ast::Var) {}

    fn visit_var_expression(&mut self, _node: &full_moon::ast::VarExpression) {}

    fn visit_while(&mut self, _node: &full_moon::ast::While) {}

    fn visit_identifier(&mut self, _token: &full_moon::tokenizer::Token) {}

    fn visit_multi_line_comment(&mut self, _token: &full_moon::tokenizer::Token) {}

    fn visit_number(&mut self, _token: &full_moon::tokenizer::Token) {}

    fn visit_single_line_comment(&mut self, _token: &full_moon::tokenizer::Token) {}

    fn visit_string_literal(&mut self, _token: &full_moon::tokenizer::Token) {}

    fn visit_symbol(&mut self, _token: &full_moon::tokenizer::Token) {}

    fn visit_token(&mut self, _token: &full_moon::tokenizer::Token) {}

    fn visit_whitespace(&mut self, _token: &full_moon::tokenizer::Token) {}
}

#[tokio::main]
async fn main() -> Result<()> {
    let test = r#"
function test(x)
	return x + 1;
end

test(5)
	"#;

    let ast = full_moon::parse(test).map_err(|e| Error::invalid_params(format!("{e}")))?;

    let mut h = HintCollector {
        scopes: SlotMap::with_key(),
        scope_stack: Vec::new(),
        node_scopes: HashMap::new(),
    };

    let nodes = ast.nodes();
    h.visit_block(nodes);

    // tracing_subscriber::fmt().init();
    //
    // let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    //
    // let (service, socket) = LspService::new(|client| Backend { client });
    // Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
