use tower_lsp::{LspService, Server};
use tracing_subscriber::EnvFilter;

mod diagnostics;
mod document;
mod index;
mod keybif;
mod nasher;
mod nwn_install;
mod providers;
mod server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::NwscriptLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
