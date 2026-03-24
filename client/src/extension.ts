import * as path from "path";
import {
  ExtensionContext,
  window,
  workspace,
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
