import * as path from "path";
import * as fs from "fs";
import {
  commands,
  Selection,
  workspace,
  ExtensionContext,
  Position,
  Uri,
  window,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import { NuiPreviewPanel } from "./nuiPreview";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration("nwscriptLsp");

  // Resolve server binary path
  let serverPath = config.get<string>("serverPath", "");
  if (!serverPath) {
    // Look for bundled binary next to the extension
    const bundledPath = path.join(
      context.extensionPath,
      "bin",
      process.platform === "win32" ? "nwscript-lsp.exe" : "nwscript-lsp"
    );
    if (fs.existsSync(bundledPath)) {
      serverPath = bundledPath;
      // Ensure execute permission on Linux/macOS (VSIX ZIP doesn't preserve it)
      if (process.platform !== "win32") {
        try { fs.chmodSync(bundledPath, 0o755); } catch {}
      }
    } else {
      // Fall back to PATH
      serverPath = process.platform === "win32" ? "nwscript-lsp.exe" : "nwscript-lsp";
    }
  }

  const serverOptions: ServerOptions = {
    run: { command: serverPath },
    debug: { command: serverPath },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "nwscript" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.nss"),
    },
    initializationOptions: {
      compilerPath: config.get<string>("compilerPath", ""),
      includeDirs: config.get<string[]>("includeDirs", []),
      excludeDirs: config.get<string[]>("excludeDirs"),
      nwscriptNssPath: config.get<string>("nwscriptNssPath", ""),
      nwnRoot: config.get<string>("nwnRoot", ""),
      extractVanillaScripts: config.get<boolean>("extractVanillaScripts", true),
      inlayHints: {
        enabled: config.get<boolean>("inlayHints.enabled", true),
        suppressForSingleArgCalls: config.get<boolean>("inlayHints.suppressForSingleArgCalls", false),
      },
      formatter: {
        maxLineWidth: config.get<number>("formatter.maxLineWidth"),
        braceStyle: config.get<string>("formatter.braceStyle"),
        sortIncludes: config.get<boolean>("formatter.sortIncludes"),
        maxBlankLines: config.get<number>("formatter.maxBlankLines"),
        trimTrailingWhitespace: config.get<boolean>("formatter.trimTrailingWhitespace"),
        spaceAfterKeywords: config.get<boolean>("formatter.spaceAfterKeywords"),
        spaceInsideParens: config.get<boolean>("formatter.spaceInsideParens"),
        spaceAroundOperators: config.get<boolean>("formatter.spaceAroundOperators"),
        spaceAfterComma: config.get<boolean>("formatter.spaceAfterComma"),
      },
    },
  };

  client = new LanguageClient(
    "nwscript-lsp",
    "NWScript Language Server",
    serverOptions,
    clientOptions
  );

  // Register command for code lens "N references" click
  context.subscriptions.push(
    commands.registerCommand("nwscript-lsp.findReferences", (uriStr: string, pos: { line: number; character: number }) => {
      const uri = Uri.parse(uriStr);
      const position = new Position(pos.line, pos.character);
      commands.executeCommand("editor.action.findReferences", uri, position);
    })
  );

  // Register command for rename-after-refactor: finds the symbol in the
  // active editor and triggers rename on it. Used by extract variable/function.
  context.subscriptions.push(
    commands.registerCommand("nwscript-lsp.renameSymbol", async (symbolName: string) => {
      const editor = window.activeTextEditor;
      if (!editor) return;

      const text = editor.document.getText();
      // Find the last occurrence (for extract function: the call, not the definition)
      const idx = text.lastIndexOf(symbolName);
      if (idx === -1) return;

      const pos = editor.document.positionAt(idx);
      editor.selection = new Selection(pos, pos);
      editor.revealRange(editor.selection);
      await commands.executeCommand("editor.action.rename");
    })
  );

  // Register command for rename-after-extract-to-file: reveals the new file
  // in the explorer and triggers file rename so the user can choose the name.
  context.subscriptions.push(
    commands.registerCommand("nwscript-lsp.renameFile", async (uriStr: string) => {
      const uri = Uri.parse(uriStr);
      await commands.executeCommand("revealInExplorer", uri);
      // Small delay to let the explorer focus on the file
      await new Promise((resolve) => setTimeout(resolve, 200));
      await commands.executeCommand("renameFile");
    })
  );

  // Register NUI preview command
  context.subscriptions.push(
    commands.registerCommand("nwscript-lsp.openNuiPreview", async () => {
      const editor = window.activeTextEditor;
      if (!editor || editor.document.languageId !== "nwscript") {
        window.showInformationMessage("Open an NWScript file to preview NUI.");
        return;
      }
      await NuiPreviewPanel.createOrShow(context.extensionUri, editor.document);
    })
  );

  // Invalidate include cache when .nss files change on disk
  const watcher = workspace.createFileSystemWatcher("**/*.nss");
  watcher.onDidCreate(() => NuiPreviewPanel.invalidateIncludes());
  watcher.onDidDelete(() => NuiPreviewPanel.invalidateIncludes());
  context.subscriptions.push(watcher);

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
