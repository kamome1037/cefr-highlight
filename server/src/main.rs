mod cefr;
mod tokenizer;
mod translate;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

const TOKEN_TYPES: &[&str] = &["cefrA1", "cefrA2", "cefrB1", "cefrB2", "cefrC1", "cefrC2"];
const TOKEN_MODIFIERS: &[&str] = &["phrase"];

fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES
            .iter()
            .map(|t| SemanticTokenType::new(t))
            .collect(),
        token_modifiers: TOKEN_MODIFIERS
            .iter()
            .map(|m| SemanticTokenModifier::new(m))
            .collect(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CefrSettings {
    #[serde(default = "default_min_level")]
    minimum_level: String,
}

fn default_min_level() -> String {
    "B2".to_string()
}

impl Default for CefrSettings {
    fn default() -> Self {
        Self {
            minimum_level: default_min_level(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhraseRange {
    line: u32,
    start_character: u32,
    length: u32,
    level: String,
    phrase: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhraseRangesNotification {
    uri: String,
    phrases: Vec<PhraseRange>,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: DashMap<String, String>,
    settings: DashMap<String, CefrSettings>,
}

impl Backend {
    fn get_min_level(&self) -> cefr::CefrLevel {
        let settings = self
            .settings
            .get("global")
            .map(|s| s.clone())
            .unwrap_or_default();
        cefr::CefrLevel::from_str(&settings.minimum_level).unwrap_or(cefr::CefrLevel::A1)
    }

    async fn fetch_settings(&self) {
        let items = vec![ConfigurationItem {
            scope_uri: None,
            section: Some("cefrHighlight".to_string()),
        }];

        if let Ok(values) = self.client.configuration(items).await {
            if let Some(val) = values.into_iter().next() {
                if let Ok(settings) = serde_json::from_value::<CefrSettings>(val) {
                    self.settings.insert("global".to_string(), settings);
                }
            }
        }
    }

    async fn process_document(&self, uri: &str) {
        let doc = match self.documents.get(uri) {
            Some(d) => d.clone(),
            None => return,
        };

        let result = tokenizer::tokenize(&doc);
        let min_level = self.get_min_level();

        let phrase_ranges: Vec<PhraseRange> = result
            .phrases
            .iter()
            .filter_map(|p| {
                let level = cefr::lookup_phrase_level(&p.phrase_key)?;
                if level < min_level {
                    return None;
                }
                Some(PhraseRange {
                    line: p.line,
                    start_character: p.start_char,
                    length: p.length,
                    level: format!("{:?}", level),
                    phrase: p.phrase_key.clone(),
                })
            })
            .collect();

        let notification = PhraseRangesNotification {
            uri: uri.to_string(),
            phrases: phrase_ranges,
        };

        let _ = self
            .client
            .send_notification::<PhraseRangesMethod>(notification)
            .await;
    }
}

struct PhraseRangesMethod;

impl tower_lsp::lsp_types::notification::Notification for PhraseRangesMethod {
    type Params = PhraseRangesNotification;
    const METHOD: &'static str = "cefr/phraseRanges";
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "cefr-lsp-server".to_string(),
                version: Some("0.2.0".to_string()),
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
        self.fetch_settings().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        self.documents
            .insert(uri.clone(), params.text_document.text);
        self.process_document(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents.insert(uri.clone(), change.text);
            self.process_document(&uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .remove(&params.text_document.uri.to_string());
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.fetch_settings().await;

        let uris: Vec<String> = self.documents.iter().map(|e| e.key().clone()).collect();
        for uri in uris {
            self.process_document(&uri).await;
        }
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

        let result = tokenizer::tokenize(&doc);
        let min_level = self.get_min_level();

        // Collect all tokens: words + phrases, sorted by position
        let mut all_tokens: Vec<(u32, u32, u32, u32, u32)> = Vec::new();

        for span in &result.words {
            if let Some(level) = cefr::lookup_level(&span.word) {
                if level < min_level {
                    continue;
                }
                all_tokens.push((
                    span.line,
                    span.start_char,
                    span.length,
                    level.token_type_index(),
                    0,
                ));
            }
        }

        for phrase in &result.phrases {
            if let Some(level) = cefr::lookup_phrase_level(&phrase.phrase_key) {
                if level < min_level {
                    continue;
                }
                all_tokens.push((
                    phrase.line,
                    phrase.start_char,
                    phrase.length,
                    level.token_type_index(),
                    1, // bit 0 = phrase modifier
                ));
            }
        }

        all_tokens.sort_by_key(|t| (t.0, t.1));

        let mut tokens: Vec<SemanticToken> = Vec::new();
        let mut prev_line: u32 = 0;
        let mut prev_start: u32 = 0;

        for (line, start, length, token_type, modifiers) in &all_tokens {
            let delta_line = line - prev_line;
            let delta_start = if delta_line == 0 {
                start - prev_start
            } else {
                *start
            };

            tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length: *length,
                token_type: *token_type,
                token_modifiers_bitset: *modifiers,
            });

            prev_line = *line;
            prev_start = *start;
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

        let result = tokenizer::tokenize(&doc);

        // Check phrases first (higher priority)
        for phrase in &result.phrases {
            if phrase.line == position.line
                && position.character >= phrase.start_char
                && position.character < phrase.start_char + phrase.length
            {
                if let Some(entries) = cefr::lookup_phrase(&phrase.phrase_key) {
                    let entry = match entries.first() {
                        Some(e) => e,
                        None => continue,
                    };
                    let level = match cefr::CefrLevel::from_str(&entry.level) {
                        Some(l) => l,
                        None => continue,
                    };

                    let chinese = translate::to_chinese(&entry.term).await;
                    let md = format_hover_md(&entry.term, level, &entry.part_of_speech, true, chinese.as_deref());

                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: md,
                        }),
                        range: Some(Range {
                            start: Position::new(phrase.line, phrase.start_char),
                            end: Position::new(phrase.line, phrase.start_char + phrase.length),
                        }),
                    }));
                }
            }
        }

        // Then check individual words
        let hovered = result.words.iter().find(|span| {
            span.line == position.line
                && position.character >= span.start_char
                && position.character < span.start_char + span.length
        });

        let hover = match hovered {
            Some(span) => {
                let entries = match cefr::lookup(&span.word) {
                    Some(e) => e,
                    None => return Ok(None),
                };
                let entry = match entries.first() {
                    Some(e) => e,
                    None => return Ok(None),
                };
                let level = match cefr::CefrLevel::from_str(&entry.level) {
                    Some(l) => l,
                    None => return Ok(None),
                };

                let chinese = translate::to_chinese(&entry.term).await;
                let md = format_hover_md(&entry.term, level, &entry.part_of_speech, false, chinese.as_deref());

                Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: md,
                    }),
                    range: Some(Range {
                        start: Position::new(span.line, span.start_char),
                        end: Position::new(span.line, span.start_char + span.length),
                    }),
                })
            }
            None => None,
        };

        Ok(hover)
    }
}

fn level_emoji(level: cefr::CefrLevel) -> &'static str {
    match level {
        cefr::CefrLevel::A1 => "⚪",
        cefr::CefrLevel::A2 => "🟢",
        cefr::CefrLevel::B1 => "🔵",
        cefr::CefrLevel::B2 => "🟣",
        cefr::CefrLevel::C1 => "🟡",
        cefr::CefrLevel::C2 => "🟠",
    }
}

fn format_hover_md(
    term: &str,
    level: cefr::CefrLevel,
    pos: &str,
    is_phrase: bool,
    chinese: Option<&str>,
) -> String {
    let emoji = level_emoji(level);
    let kind = if is_phrase { " · phrase" } else { "" };
    let pos_str = if pos.is_empty() {
        String::new()
    } else {
        format!(" · *{}*", pos)
    };

    let mut md = format!(
        "### {} {}\n\n{} **{}**{}{}\n\n",
        emoji,
        term,
        emoji,
        level.label(),
        kind,
        pos_str,
    );

    if let Some(zh) = chinese {
        md.push_str(&format!("---\n\n📖 **{}**\n", zh));
    }

    md
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let _ = cefr::index();
    log::info!(
        "CEFR index loaded ({} entries, {} phrases)",
        cefr::index().len(),
        cefr::phrase_keys().len()
    );

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(|client| Backend {
        client,
        documents: DashMap::new(),
        settings: DashMap::new(),
    })
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
