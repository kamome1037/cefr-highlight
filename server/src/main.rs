mod cefr;
mod tokenizer;

use dashmap::DashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

const TOKEN_TYPES: &[&str] = &["cefrA1", "cefrA2", "cefrB1", "cefrB2", "cefrC1", "cefrC2"];

fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES
            .iter()
            .map(|t| SemanticTokenType::new(t))
            .collect(),
        token_modifiers: vec![],
    }
}

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: DashMap<String, String>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "cefr-lsp-server".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        ..Default::default()
                    },
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        log::info!("CEFR LSP server initialized");
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        self.documents
            .insert(uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents.insert(uri, change.text);
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .remove(&params.text_document.uri.to_string());
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri.to_string();
        let doc = match self.documents.get(&uri) {
            Some(d) => d.clone(),
            None => return Ok(None),
        };

        let words = tokenizer::tokenize(&doc);
        let mut tokens: Vec<SemanticToken> = Vec::new();
        let mut prev_line: u32 = 0;
        let mut prev_start: u32 = 0;

        for span in &words {
            if let Some(level) = cefr::lookup_level(&span.word) {
                let delta_line = span.line - prev_line;
                let delta_start = if delta_line == 0 {
                    span.start_char - prev_start
                } else {
                    span.start_char
                };

                tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length: span.length,
                    token_type: level.token_type_index(),
                    token_modifiers_bitset: 0,
                });

                prev_line = span.line;
                prev_start = span.start_char;
            }
        }

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        let doc = match self.documents.get(&uri) {
            Some(d) => d.clone(),
            None => return Ok(None),
        };

        let words = tokenizer::tokenize(&doc);
        let hovered = words.iter().find(|span| {
            span.line == position.line
                && position.character >= span.start_char
                && position.character < span.start_char + span.length
        });

        let hover = hovered.and_then(|span| {
            let entries = cefr::lookup(&span.word)?;
            let mut lines = Vec::new();

            for entry in entries {
                let level = cefr::CefrLevel::from_str(&entry.level)?;
                let mut parts = vec![format!("**{}** — {}", entry.term, level.label())];
                if !entry.part_of_speech.is_empty() {
                    parts.push(format!("*{}*", entry.part_of_speech));
                }
                if !entry.topic.is_empty() {
                    parts.push(format!("Topic: {}", entry.topic));
                }
                lines.push(parts.join(" | "));
            }

            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: lines.join("\n\n"),
                }),
                range: Some(Range {
                    start: Position::new(span.line, span.start_char),
                    end: Position::new(span.line, span.start_char + span.length),
                }),
            })
        });

        Ok(hover)
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    // Force-initialize the CEFR index at startup
    let _ = cefr::index();
    log::info!("CEFR index loaded ({} entries)", cefr::index().len());

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(|client| Backend {
        client,
        documents: DashMap::new(),
    })
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
