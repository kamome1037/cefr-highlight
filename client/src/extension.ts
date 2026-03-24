import * as path from "path";
import {
  ExtensionContext,
  workspace,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  const serverBin = context.asAbsolutePath(
    path.join("server", "target", "release", "cefr-lsp-server")
  );

  const serverOptions: ServerOptions = {
    run: { command: serverBin },
    debug: { command: serverBin },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "plaintext" },
      { scheme: "file", language: "markdown" },
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.*"),
    },
  };

  client = new LanguageClient(
    "cefrHighlight",
    "CEFR Highlight",
    serverOptions,
    clientOptions
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
