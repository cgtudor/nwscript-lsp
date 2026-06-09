import * as path from "path";
import * as fs from "fs";
import {
  commands,
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

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
