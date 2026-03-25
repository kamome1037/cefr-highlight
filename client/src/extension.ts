import * as path from "path";
import { randomUUID } from "crypto";
import {
  ExtensionContext,
  window,
  workspace,
  commands,
  Uri,
  TextDocumentContentProvider,
  CancellationToken,
  TextEditorDecorationType,
  Range,
  Position,
  ConfigurationTarget,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient;
let phraseDecorations: Map<string, TextEditorDecorationType> = new Map();
let phraseRangesCache: Map<string, PhraseRange[]> = new Map();

interface PhraseRange {
  line: number;
  startCharacter: number;
  length: number;
  level: string;
  phrase: string;
}

interface PhraseRangesNotification {
  uri: string;
  phrases: PhraseRange[];
}

const LEVELS = ["A1", "A2", "B1", "B2", "C1", "C2"] as const;

const MEMORY_ORIGINAL_SCHEME = "cefr-memory-original";
const STORAGE_MEMORY_SESSION = "cefrHighlight.memoryRecallSessionId";
const STORAGE_MEMORY_RECALL_URI = "cefrHighlight.memoryRecallRecallUri";

const memoryOriginalBySession = new Map<string, string>();

function memorySessionIdFromUri(uri: Uri): string | undefined {
  const name = uri.path.replace(/^\/+/, "");
  if (!name.endsWith(".txt")) return undefined;
  return name.slice(0, -4) || undefined;
}

class MemoryOriginalProvider implements TextDocumentContentProvider {
  provideTextDocumentContent(uri: Uri, _token: CancellationToken): string {
    const id = memorySessionIdFromUri(uri);
    return id ? memoryOriginalBySession.get(id) ?? "" : "";
  }
}

async function startMemoryRecall(context: ExtensionContext): Promise<void> {
  const editor = window.activeTextEditor;
  if (!editor) {
    void window.showWarningMessage(
      "Open a file and select the paragraph you want to memorize."
    );
    return;
  }
  const text = editor.document.getText(editor.selection);
  if (!text.trim()) {
    void window.showWarningMessage(
      "Select the paragraph text first, then run this command again."
    );
    return;
  }

  const previousId = context.workspaceState.get<string>(STORAGE_MEMORY_SESSION);
  if (previousId) {
    memoryOriginalBySession.delete(previousId);
  }

  const sessionId = randomUUID();
  memoryOriginalBySession.set(sessionId, text);
  await context.workspaceState.update(STORAGE_MEMORY_SESSION, sessionId);

  const doc = await workspace.openTextDocument({
    language: "plaintext",
    content: "",
  });
  await window.showTextDocument(doc, { preview: false });
  await context.workspaceState.update(STORAGE_MEMORY_RECALL_URI, doc.uri.toString());

  void window.showInformationMessage(
    "Type the paragraph from memory, then run “CEFR Highlight: Submit memory recall (show diff)”."
  );
}

async function submitMemoryRecall(context: ExtensionContext): Promise<void> {
  const sessionId = context.workspaceState.get<string>(STORAGE_MEMORY_SESSION);
  const original =
    sessionId !== undefined ? memoryOriginalBySession.get(sessionId) : undefined;
  if (!sessionId || original === undefined) {
    void window.showWarningMessage(
      'No active recall session. Run “CEFR Highlight: Start paragraph memory recall” first.'
    );
    return;
  }

  const active = window.activeTextEditor;
  if (!active) {
    void window.showWarningMessage("Open the recall tab you started, then run this command again.");
    return;
  }

  const expectedUri = context.workspaceState.get<string>(STORAGE_MEMORY_RECALL_URI);
  if (expectedUri && active.document.uri.toString() !== expectedUri) {
    const choice = await window.showWarningMessage(
      "The active editor is not your recall draft. Compare the active file to the stored original anyway?",
      "Compare anyway",
      "Cancel"
    );
    if (choice !== "Compare anyway") {
      return;
    }
  }

  const leftUri = Uri.from({
    scheme: MEMORY_ORIGINAL_SCHEME,
    path: `/${sessionId}.txt`,
  });

  await commands.executeCommand(
    "vscode.diff",
    leftUri,
    active.document.uri,
    "CEFR memory recall — original ↔ your text"
  );
}

function getColorConfig(): Record<string, string> {
  const config = workspace.getConfiguration("cefrHighlight");
  const colors: Record<string, string> = {};
  for (const level of LEVELS) {
    colors[level] = config.get<string>(`colors.${level}`, getDefaultColor(level));
  }
  return colors;
}

function getDefaultColor(level: string): string {
  const defaults: Record<string, string> = {
    A1: "#BDBDBD",
    A2: "#43A047",
    B1: "#1E88E5",
    B2: "#8E24AA",
    C1: "#CDDC39",
    C2: "#FB8C00",
  };
  return defaults[level] ?? "#FFFFFF";
}

function getPhraseBackground(): string {
  return workspace
    .getConfiguration("cefrHighlight")
    .get<string>("phraseBackground", "rgba(255, 235, 59, 0.15)");
}

function syncSemanticTokenColors() {
  const colors = getColorConfig();
  const rules: Record<string, { foreground: string; fontStyle: string }> = {};
  for (const level of LEVELS) {
    const tokenName = `cefr${level}`;
    rules[tokenName] = {
      foreground: colors[level],
      fontStyle: level === "C2" ? "bold" : "",
    };
  }

  const currentConfig = workspace.getConfiguration("editor");
  const current = currentConfig.get<any>("semanticTokenColorCustomizations") ?? {};
  const updated = { ...current, rules: { ...current.rules, ...rules } };

  currentConfig.update(
    "semanticTokenColorCustomizations",
    updated,
    ConfigurationTarget.Global
  );
}

function createPhraseDecorationTypes(): Map<string, TextEditorDecorationType> {
  const colors = getColorConfig();
  const bg = getPhraseBackground();
  const map = new Map<string, TextEditorDecorationType>();

  for (const level of LEVELS) {
    map.set(
      level,
      window.createTextEditorDecorationType({
        backgroundColor: bg,
        color: colors[level],
        fontWeight: level === "C2" ? "bold" : "normal",
        borderRadius: "3px",
      })
    );
  }

  return map;
}

function disposePhraseDecorations() {
  for (const dec of phraseDecorations.values()) {
    dec.dispose();
  }
  phraseDecorations.clear();
}

function applyPhraseDecorations(uri: string) {
  const editor = window.visibleTextEditors.find(
    (e) => e.document.uri.toString() === uri
  );
  if (!editor) return;

  const phrases = phraseRangesCache.get(uri) ?? [];
  const byLevel = new Map<string, Range[]>();

  for (const p of phrases) {
    const range = new Range(
      new Position(p.line, p.startCharacter),
      new Position(p.line, p.startCharacter + p.length)
    );
    const list = byLevel.get(p.level) ?? [];
    list.push(range);
    byLevel.set(p.level, list);
  }

  for (const level of LEVELS) {
    const dec = phraseDecorations.get(level);
    if (dec) {
      editor.setDecorations(dec, byLevel.get(level) ?? []);
    }
  }
}

export function activate(context: ExtensionContext) {
  const serverBin = context.asAbsolutePath(
    path.join("server", "target", "release", "cefr-lsp-server")
  );

  const serverOptions: ServerOptions = {
    run: { command: serverBin },
    debug: { command: serverBin },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "cefr" }],
    synchronize: {
      configurationSection: "cefrHighlight",
    },
  };

  client = new LanguageClient(
    "cefrHighlight",
    "CEFR Highlight",
    serverOptions,
    clientOptions
  );

  syncSemanticTokenColors();
  phraseDecorations = createPhraseDecorationTypes();

  client.start().then(() => {
    client.onNotification("cefr/phraseRanges", (params: PhraseRangesNotification) => {
      phraseRangesCache.set(params.uri, params.phrases);
      applyPhraseDecorations(params.uri);
    });
  });

  const memoryProvider = new MemoryOriginalProvider();
  context.subscriptions.push(
    workspace.registerTextDocumentContentProvider(
      MEMORY_ORIGINAL_SCHEME,
      memoryProvider
    )
  );

  context.subscriptions.push(
    commands.registerCommand("cefrHighlight.startMemoryRecall", () =>
      startMemoryRecall(context)
    ),
    commands.registerCommand("cefrHighlight.submitMemoryRecall", () =>
      submitMemoryRecall(context)
    )
  );

  context.subscriptions.push(
    workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("cefrHighlight")) {
        syncSemanticTokenColors();
        disposePhraseDecorations();
        phraseDecorations = createPhraseDecorationTypes();

        for (const [uri] of phraseRangesCache) {
          applyPhraseDecorations(uri);
        }
      }
    })
  );

  context.subscriptions.push(
    window.onDidChangeActiveTextEditor((editor) => {
      if (editor) {
        applyPhraseDecorations(editor.document.uri.toString());
      }
    })
  );
}

export function deactivate(): Thenable<void> | undefined {
  disposePhraseDecorations();
  if (!client) {
    return undefined;
  }
  return client.stop();
}
