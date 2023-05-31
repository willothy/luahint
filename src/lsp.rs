use std::sync::atomic::{AtomicI32, Ordering};

use crop::Rope;
use dashmap::DashMap;
use lsp_types::{InitializeParams, InitializeResult, ServerInfo, Url};

use tower_lsp::jsonrpc::{self, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::scope::ScopeManager;

#[derive(Debug)]
pub struct Doc {
    text: Rope,
    version: AtomicI32,
    #[allow(unused)]
    uri: Url,
}

impl Doc {
    pub fn new(text: String, uri: Url, version: i32) -> Self {
        Self {
            text: Rope::from(text),
            uri,
            version: AtomicI32::new(version),
        }
    }
}

#[derive(Debug)]
pub struct Backend {
    #[allow(unused)]
    client: Client,
    documents: DashMap<Url, Doc>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
        }
    }
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
        let TextDocumentItem {
            uri, version, text, ..
        } = params.text_document;

        self.documents
            .insert(uri.clone(), Doc::new(text, uri, version));
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(mut doc) = self.documents.get_mut(&uri) else {
			return
		};
        let text = &mut doc.text;
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
        let text = doc.text.to_string();
        let ast = full_moon::parse(&text).map_err(|_| jsonrpc::Error::internal_error())?;
        let hints = ScopeManager::new(ast).hints;

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
