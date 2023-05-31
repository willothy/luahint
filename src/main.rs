use lsp::Backend;
use tower_lsp::jsonrpc::Result;
use tower_lsp::{LspService, Server};

mod lsp;
mod scope;
mod visitor;

#[tokio::main]
async fn main() -> Result<()> {
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());

    let (service, socket) = LspService::new(|client| Backend::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
