import * as path from "path";
import { randomUUID } from "crypto";
import {
  ExtensionContext,
  window,
  workspace,
  commands,
  Uri,
  TextDocument,
  TextDocumentContentProvider,
  CancellationToken,
  TextEditorDecorationType,
  Range,
  Position,
  ConfigurationTarget,
  Tab,
  TabInputTextDiff,
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
const MEMORY_RECALL_DIFF_SCHEME = "cefr-memory-recall-diff";
const STORAGE_MEMORY_SESSION = "cefrHighlight.memoryRecallSessionId";
const STORAGE_MEMORY_RECALL_URI = "cefrHighlight.memoryRecallRecallUri";

const memoryOriginalBySession = new Map<string, string>();
const memoryRecallDiffBySession = new Map<string, string>();
const memoryRecallStartedAtMs = new Map<string, number>();
/** After the first mismatch diff was opened, the next submit ends the session (closes draft, no new diff). */
const memoryRecallDiffWasOpened = new Map<string, boolean>();

/** Strips horizontal whitespace at the end of each line (does not trim line-start indent). */
function stripTrailingWhitespacePerLine(text: string): string {
  return text.replace(/[^\S\r\n]+$/gm, "");
}

/** Same normalization as the diff view, plus unified newlines, for an exact “correct” check. */
function normalizeForRecallCompare(text: string): string {
  const lf = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  return stripTrailingWhitespacePerLine(lf);
}

async function clearMemoryRecallSession(
  context: ExtensionContext,
  sessionId: string
): Promise<void> {
  memoryOriginalBySession.delete(sessionId);
  memoryRecallDiffBySession.delete(sessionId);
  memoryRecallStartedAtMs.delete(sessionId);
  memoryRecallDiffWasOpened.delete(sessionId);
  await context.workspaceState.update(STORAGE_MEMORY_SESSION, undefined);
  await context.workspaceState.update(STORAGE_MEMORY_RECALL_URI, undefined);
}

async function closeMemoryRecallDiffTabs(sessionId: string): Promise<void> {
  const tabsToClose: Tab[] = [];
  for (const group of window.tabGroups.all) {
    for (const tab of group.tabs) {
      const input = tab.input;
      if (input instanceof TabInputTextDiff) {
        const oid = memorySessionIdFromUri(input.original);
        const mid = memorySessionIdFromUri(input.modified);
        if (
          oid === sessionId &&
          mid === sessionId &&
          input.original.scheme === MEMORY_ORIGINAL_SCHEME &&
          input.modified.scheme === MEMORY_RECALL_DIFF_SCHEME
        ) {
          tabsToClose.push(tab);
        }
      }
    }
  }
  if (tabsToClose.length > 0) {
    await window.tabGroups.close(tabsToClose);
  }
}

async function closeRecallDraftTabAndDeleteIfFile(
  recallDoc: TextDocument
): Promise<void> {
  const recallUri = recallDoc.uri;
  const wasUntitled = recallDoc.isUntitled;
  await window.showTextDocument(recallDoc);
  await commands.executeCommand("workbench.action.revertAndCloseActiveEditor");
  if (!wasUntitled && recallUri.scheme === "file") {
    try {
      await workspace.fs.delete(recallUri, { useTrash: true });
    } catch {
      /* closed or already removed */
    }
  }
}

function formatWritingDuration(ms: number): string {
  const totalSec = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  const parts: string[] = [];
  if (h > 0) parts.push(`${h}h`);
  if (m > 0) parts.push(`${m}m`);
  if (s > 0 || parts.length === 0) parts.push(`${s}s`);
  return parts.join(" ");
}

function memorySessionIdFromUri(uri: Uri): string | undefined {
  const name = uri.path.replace(/^\/+/, "");
  if (!name.endsWith(".txt")) return undefined;
  return name.slice(0, -4) || undefined;
}

class MemoryOriginalProvider implements TextDocumentContentProvider {
  provideTextDocumentContent(uri: Uri, _token: CancellationToken): string {
    const id = memorySessionIdFromUri(uri);
    const raw = id ? memoryOriginalBySession.get(id) ?? "" : "";
    return stripTrailingWhitespacePerLine(raw);
  }
}

class MemoryRecallDiffProvider implements TextDocumentContentProvider {
  provideTextDocumentContent(uri: Uri, _token: CancellationToken): string {
    const id = memorySessionIdFromUri(uri);
    return id ? memoryRecallDiffBySession.get(id) ?? "" : "";
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
    memoryRecallDiffBySession.delete(previousId);
    memoryRecallStartedAtMs.delete(previousId);
    memoryRecallDiffWasOpened.delete(previousId);
  }

  const sessionId = randomUUID();
  memoryOriginalBySession.set(sessionId, text);
  await context.workspaceState.update(STORAGE_MEMORY_SESSION, sessionId);

  const doc = await workspace.openTextDocument({
    language: "plaintext",
    content: "",
  });
  await window.showTextDocument(doc, { preview: false });
  memoryRecallStartedAtMs.set(sessionId, Date.now());
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

  await closeMemoryRecallDiffTabs(sessionId);

  const expectedUri = context.workspaceState.get<string>(STORAGE_MEMORY_RECALL_URI);
  const recallDocFromState = expectedUri
    ? workspace.textDocuments.find((d) => d.uri.toString() === expectedUri)
    : undefined;

  const active = window.activeTextEditor;
  const activeUri = active?.document.uri;

  const onOurDiffVirtual = (uri: Uri | undefined) =>
    !!uri &&
    (uri.scheme === MEMORY_ORIGINAL_SCHEME || uri.scheme === MEMORY_RECALL_DIFF_SCHEME) &&
    memorySessionIdFromUri(uri) === sessionId;

  let recallDoc: TextDocument | undefined = recallDocFromState;

  if (!recallDoc && active && activeUri && !onOurDiffVirtual(activeUri)) {
    if (!expectedUri || activeUri.toString() === expectedUri) {
      recallDoc = active.document;
    }
  }

  if (
    !recallDoc &&
    active &&
    activeUri &&
    !onOurDiffVirtual(activeUri) &&
    expectedUri &&
    activeUri.toString() !== expectedUri
  ) {
    const choice = await window.showWarningMessage(
      "The active editor is not your recall draft. Compare the active file to the stored original anyway?",
      "Compare anyway",
      "Cancel"
    );
    if (choice === "Compare anyway") {
      recallDoc = active.document;
    } else {
      return;
    }
  }

  if (!recallDoc) {
    void window.showWarningMessage(
      "Open your recall draft (where you typed from memory), then submit again."
    );
    return;
  }

  const startedAt = memoryRecallStartedAtMs.get(sessionId);
  const elapsedMs =
    startedAt !== undefined ? Date.now() - startedAt : 0;
  const durationLabel = formatWritingDuration(elapsedMs);

  const repeatAfterDiff = memoryRecallDiffWasOpened.get(sessionId) === true;

  const matches =
    normalizeForRecallCompare(original) ===
    normalizeForRecallCompare(recallDoc.getText());

  if (matches) {
    await closeRecallDraftTabAndDeleteIfFile(recallDoc);
    await clearMemoryRecallSession(context, sessionId);
    void window.showInformationMessage(
      `Correct — writing time: ${durationLabel}. Recall tab closed.`
    );
    return;
  }

  if (repeatAfterDiff) {
    await closeRecallDraftTabAndDeleteIfFile(recallDoc);
    await clearMemoryRecallSession(context, sessionId);
    void window.showInformationMessage(
      "Session closed — diff closed and recall draft removed."
    );
    return;
  }

  memoryRecallDiffBySession.set(
    sessionId,
    stripTrailingWhitespacePerLine(recallDoc.getText())
  );

  const leftUri = Uri.from({
    scheme: MEMORY_ORIGINAL_SCHEME,
    path: `/${sessionId}.txt`,
  });
  const rightUri = Uri.from({
    scheme: MEMORY_RECALL_DIFF_SCHEME,
    path: `/${sessionId}.txt`,
  });

  const diffTitle = `CEFR memory recall — ${durationLabel} — original ↔ your text (EOL spaces ignored)`;

  await commands.executeCommand("vscode.diff", leftUri, rightUri, diffTitle);
  memoryRecallDiffWasOpened.set(sessionId, true);

  void window.showInformationMessage(`Writing time: ${durationLabel}.`);
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

  const memoryOriginalProvider = new MemoryOriginalProvider();
  const memoryRecallDiffProvider = new MemoryRecallDiffProvider();
  context.subscriptions.push(
    workspace.registerTextDocumentContentProvider(
      MEMORY_ORIGINAL_SCHEME,
      memoryOriginalProvider
    ),
    workspace.registerTextDocumentContentProvider(
      MEMORY_RECALL_DIFF_SCHEME,
      memoryRecallDiffProvider
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
