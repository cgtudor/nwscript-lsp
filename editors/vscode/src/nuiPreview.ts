import * as vscode from "vscode";
import { evaluateNuiScript } from "./nwscript/interpreter";
import { IncludeResolver } from "./nwscript/includeResolver";
import { solveLayout } from "./nwscript/layoutSolver";
import { getWebviewContent } from "./webview/previewHtml";

export class NuiPreviewPanel {
  public static readonly viewType = "nwscript.nuiPreview";

  private static panels = new Map<string, NuiPreviewPanel>();
  private static includeResolver = new IncludeResolver();

  private panel: vscode.WebviewPanel;
  private documentUri: vscode.Uri;
  private disposables: vscode.Disposable[] = [];
  private debounceTimer: NodeJS.Timeout | undefined;
  private lastResult: any = null;  // stored eval result for view switching

  private constructor(panel: vscode.WebviewPanel, documentUri: vscode.Uri) {
    this.panel = panel;
    this.documentUri = documentUri;

    this.panel.onDidDispose(() => this.dispose(), null, this.disposables);

    // Listen for document changes
    this.disposables.push(
      vscode.workspace.onDidChangeTextDocument((e) => {
        if (e.document.uri.toString() === this.documentUri.toString()) {
          this.scheduleUpdate();
        }
      })
    );

    // Listen for save events (also re-evaluate on save for include changes)
    this.disposables.push(
      vscode.workspace.onDidSaveTextDocument((doc) => {
        if (doc.uri.toString() === this.documentUri.toString()) {
          this.updatePreview();
        }
      })
    );

    // Listen for messages from webview (scale changes, view switches)
    this.disposables.push(
      this.panel.webview.onDidReceiveMessage((msg) => {
        if (msg.type === "resolve" && msg.nuiJson) {
          const layout = solveLayout(msg.nuiJson, msg.windowWidth, msg.windowHeight, msg.scale ?? 1.0);
          this.panel.webview.postMessage({
            type: "scaleResult",
            layout,
            scale: msg.scale,
          });
        }
        if (msg.type === "switchView" && this.lastResult) {
          const { interpreter, swapGroupId } = this.lastResult;
          if (interpreter && swapGroupId && msg.viewName) {
            const viewLayout = interpreter.tryCallForLayout(msg.viewName);
            if (viewLayout) {
              // Deep clone the window JSON before modifying (avoid mutating cached state)
              const jsonClone = JSON.parse(JSON.stringify(interpreter.getWindowJson()));
              // Apply the view layout to the swap group in the clone
              const applyToGroup = (node: any, groupId: string, layout: any): boolean => {
                if (!node || typeof node !== 'object') return false;
                if (node.id === groupId && node.type === 'group') {
                  node.children = [layout];
                  return true;
                }
                if (node.root && applyToGroup(node.root, groupId, layout)) return true;
                if (Array.isArray(node.children)) {
                  for (const c of node.children) {
                    if (applyToGroup(c, groupId, layout)) return true;
                  }
                }
                return false;
              };
              applyToGroup(jsonClone, swapGroupId, viewLayout);

              const scale = msg.scale ?? 1.0;
              const geo = this.lastResult.geometry;
              // Prefer the webview's size override (editable W/H inputs), then captured geometry
              const winW = msg.windowWidth > 50 ? Math.round(msg.windowWidth)
                : geo && geo.w > 50 ? Math.round(geo.w) : 500;
              const winH = msg.windowHeight > 50 ? Math.round(msg.windowHeight)
                : geo && geo.h > 50 ? Math.round(geo.h) : 600;
              const layout = solveLayout(jsonClone, winW, winH, scale);
              this.panel.webview.postMessage({
                type: "scaleResult",
                layout,
                scale,
                nuiJson: jsonClone,
              });
            }
          }
        }
      })
    );

    // Initial render
    this.updatePreview();
  }

  /**
   * Open or reveal the NUI preview panel for the given document.
   */
  public static async createOrShow(
    extensionUri: vscode.Uri,
    document: vscode.TextDocument
  ): Promise<void> {
    const key = document.uri.toString();

    // If panel already exists, reveal it
    const existing = NuiPreviewPanel.panels.get(key);
    if (existing) {
      existing.panel.reveal(vscode.ViewColumn.Beside);
      return;
    }

    // Create new panel
    const panel = vscode.window.createWebviewPanel(
      NuiPreviewPanel.viewType,
      `NUI: ${this.basename(document.uri)}`,
      { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [extensionUri],
      }
    );

    panel.iconPath = new vscode.ThemeIcon("preview");
    panel.webview.html = getWebviewContent(panel.webview, extensionUri);

    const instance = new NuiPreviewPanel(panel, document.uri);
    NuiPreviewPanel.panels.set(key, instance);
  }

  /**
   * Check if the active document looks like it contains NUI code.
   */
  public static containsNuiCode(document: vscode.TextDocument): boolean {
    const text = document.getText();
    return /\bNuiWindow\s*\(/.test(text) || /\bNuiCreate\s*\(/.test(text);
  }

  public static invalidateIncludes(): void {
    NuiPreviewPanel.includeResolver.invalidate();
  }

  private static basename(uri: vscode.Uri): string {
    return uri.path.split("/").pop() ?? "preview";
  }

  private scheduleUpdate(): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    this.debounceTimer = setTimeout(() => this.updatePreview(), 500);
  }

  private async updatePreview(): Promise<void> {
    // Read the current document text
    let document: vscode.TextDocument;
    try {
      document = await vscode.workspace.openTextDocument(this.documentUri);
    } catch {
      this.panel.webview.postMessage({ type: "error", errors: ["Could not read document"] });
      return;
    }

    const source = document.getText();

    // Resolve includes
    let resolvedSource: string;
    try {
      resolvedSource = await NuiPreviewPanel.includeResolver.resolveAll(source, this.documentUri);
    } catch (e: any) {
      resolvedSource = source; // Fall back to unresolved
    }

    // Evaluate the NWScript to produce JSON
    const result = evaluateNuiScript(resolvedSource);

    // Determine window dimensions from captured geometry or defaults
    const geo = result.geometry;
    const winW = geo && geo.w > 50 ? Math.round(geo.w) : 500;
    const winH = geo && geo.h > 50 ? Math.round(geo.h) : 600;

    // Note: if geometry wasn't captured, we fall back to 500x600

    // Solve layout using the same algorithm as NWN's Cassowary solver
    const layout = result.json ? solveLayout(result.json, winW, winH) : null;

    // Store result for view switching
    this.lastResult = { ...result, geometry: geo };

    // Send solved layout + raw JSON to webview (webview re-solves on scale changes)
    this.panel.webview.postMessage({
      type: "update",
      layout,
      nuiJson: result.json,
      windowWidth: winW,
      windowHeight: winH,
      errors: result.errors.filter((e) => e && !e.startsWith("Unknown identifier")),
      functions: result.functions,
      views: result.views,
    });
  }

  private dispose(): void {
    NuiPreviewPanel.panels.delete(this.documentUri.toString());
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    for (const d of this.disposables) d.dispose();
    this.disposables = [];
  }
}
