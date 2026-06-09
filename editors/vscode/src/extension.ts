import * as path from "path";
import * as fs from "fs";
import {
  workspace,
  ExtensionContext,
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

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
