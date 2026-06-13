import * as vscode from "vscode";
import { evaluateNuiScript, findEntryFunction } from "./nwscript/interpreter";
import { IncludeResolver } from "./nwscript/includeResolver";
import { solveLayout } from "./nwscript/layoutSolver";
import { getWebviewContent } from "./webview/previewHtml";
import { discoverBinds, resolveBinds, builtinPresets, defaultBindValue, BindInfo, Preset } from "./nwscript/bindResolver";

export class NuiPreviewPanel {
  public static readonly viewType = "nwscript.nuiPreview";

  private static panels = new Map<string, NuiPreviewPanel>();
  private static includeResolver = new IncludeResolver();

  private panel: vscode.WebviewPanel;
  private documentUri: vscode.Uri;
  private storage: vscode.Memento;
  private disposables: vscode.Disposable[] = [];
  private debounceTimer: NodeJS.Timeout | undefined;

  // ── Authoritative render state (host owns it; the webview only sends triggers) ──
  private interpreter: any = null;
  private windowJson: any = null;
  private swapGroupId: string | null = null;
  private binds: BindInfo[] = [];
  private bindValues: Record<string, any> = {}; // overrides; missing keys use placeholders
  private formId = "";
  private activeView: string | null = null; // group-swap view, if any
  private chosenFunction: string | undefined; // function-selector override
  private curScale = 1.0;
  private curW = 500;
  private curH = 600;

  private constructor(panel: vscode.WebviewPanel, documentUri: vscode.Uri, storage: vscode.Memento) {
    this.panel = panel;
    this.documentUri = documentUri;
    this.storage = storage;

    this.panel.onDidDispose(() => this.dispose(), null, this.disposables);

    this.disposables.push(
      vscode.workspace.onDidChangeTextDocument((e) => {
        if (e.document.uri.toString() === this.documentUri.toString()) this.scheduleUpdate();
      })
    );
    this.disposables.push(
      vscode.workspace.onDidSaveTextDocument((doc) => {
        if (doc.uri.toString() === this.documentUri.toString()) this.updatePreview();
      })
    );
    this.disposables.push(this.panel.webview.onDidReceiveMessage((msg) => this.onMessage(msg)));

    this.updatePreview();
  }

  // ── Message handling ────────────────────────────────────────────────────────

  private onMessage(msg: any): void {
    switch (msg.type) {
      case "resolve": // scale / window-size change
        this.applyViewport(msg);
        this.postLayout("scaleResult");
        break;

      case "switchView":
        this.activeView = msg.viewName || null;
        this.applyViewport(msg);
        // A swapped-in view brings its own binds — re-discover so the inspector
        // reflects the current view, then push the refreshed list + layout.
        this.binds = discoverBinds(this.currentJson());
        this.postBindState();
        break;

      case "selectFunction":
        this.chosenFunction = msg.name || undefined;
        this.updatePreview();
        break;

      case "setBind": // a single bind value was edited in the inspector
        if (typeof msg.name === "string") {
          this.bindValues[msg.name] = msg.value;
          this.postLayout("scaleResult");
        }
        break;

      case "applyPreset": {
        const preset = this.allPresets().find((p) => p.name === msg.name);
        if (preset) {
          this.bindValues = { ...preset.values };
          this.postBindState();
        }
        break;
      }

      case "resetBinds":
        this.bindValues = {};
        this.postBindState();
        break;

      case "savePreset":
        if (typeof msg.name === "string" && msg.name.trim()) {
          this.saveCustomPreset(msg.name.trim(), { ...this.bindValues });
          this.postBindState();
        }
        break;

      case "deletePreset":
        if (typeof msg.name === "string") {
          this.deleteCustomPreset(msg.name);
          this.postBindState();
        }
        break;
    }
  }

  private applyViewport(msg: any): void {
    if (typeof msg.scale === "number") this.curScale = msg.scale;
    if (typeof msg.windowWidth === "number" && msg.windowWidth > 50) this.curW = Math.round(msg.windowWidth);
    if (typeof msg.windowHeight === "number" && msg.windowHeight > 50) this.curH = Math.round(msg.windowHeight);
  }

  // ── Rendering ─────────────────────────────────────────────────────────────

  /** The window JSON for the current view (clone with the active view swapped in). */
  private currentJson(): any {
    if (!this.windowJson) return null;
    if (this.activeView && this.interpreter && this.swapGroupId) {
      const viewLayout = this.interpreter.tryCallForLayout(this.activeView);
      if (viewLayout) {
        const json = JSON.parse(JSON.stringify(this.windowJson));
        applyToGroup(json, this.swapGroupId, viewLayout);
        return json;
      }
    }
    return this.windowJson;
  }

  /** Resolve binds into the current view's JSON and solve its layout. */
  private solveCurrent(): any {
    const json = this.currentJson();
    if (!json) return null;
    return solveLayout(resolveBinds(json, this.bindValues), this.curW, this.curH, this.curScale);
  }

  private postLayout(type: "scaleResult" | "update"): void {
    this.panel.webview.postMessage({ type, layout: this.solveCurrent(), scale: this.curScale });
  }

  /** Push refreshed binds + values + presets to the inspector along with a new layout. */
  private postBindState(): void {
    this.panel.webview.postMessage({
      type: "bindState",
      layout: this.solveCurrent(),
      scale: this.curScale,
      binds: this.binds,
      bindValues: this.effectiveValues(),
      presets: this.allPresets().map((p) => p.name),
      customPresets: this.customPresets().map((p) => p.name),
    });
  }

  /** Per-bind value actually used for rendering: an override if set, else the placeholder. */
  private effectiveValues(): Record<string, any> {
    const out: Record<string, any> = {};
    for (const b of this.binds) {
      out[b.name] = Object.prototype.hasOwnProperty.call(this.bindValues, b.name)
        ? this.bindValues[b.name]
        : defaultBindValue(b);
    }
    return out;
  }

  // ── Presets ─────────────────────────────────────────────────────────────────

  private allPresets(): Preset[] {
    return [...builtinPresets(this.binds), ...this.customPresets()];
  }

  private storageKey(): string {
    return `nuiPreview.presets.${this.formId || this.documentUri.toString()}`;
  }

  private customPresets(): Preset[] {
    return this.storage.get<Preset[]>(this.storageKey(), []);
  }

  private saveCustomPreset(name: string, values: Record<string, any>): void {
    const builtin = new Set(["Typical", "Empty", "Overflow", "Max rows"]);
    if (builtin.has(name)) return; // don't shadow built-ins
    const existing = this.customPresets().filter((p) => p.name !== name);
    existing.push({ name, values });
    this.storage.update(this.storageKey(), existing);
  }

  private deleteCustomPreset(name: string): void {
    const remaining = this.customPresets().filter((p) => p.name !== name);
    this.storage.update(this.storageKey(), remaining);
  }

  // ── Evaluation ───────────────────────────────────────────────────────────────

  private scheduleUpdate(): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    this.debounceTimer = setTimeout(() => this.updatePreview(), 500);
  }

  private async updatePreview(): Promise<void> {
    let document: vscode.TextDocument;
    try {
      document = await vscode.workspace.openTextDocument(this.documentUri);
    } catch {
      this.panel.webview.postMessage({ type: "error", errors: ["Could not read document"] });
      return;
    }

    const source = document.getText();
    let resolvedSource: string;
    try {
      resolvedSource = await NuiPreviewPanel.includeResolver.resolveAll(source, this.documentUri);
    } catch {
      resolvedSource = source;
    }

    // Target the builder defined in THIS file (not one pulled in transitively via
    // includes), unless the user picked a specific function from the dropdown.
    const entry = this.chosenFunction ?? findEntryFunction(source) ?? undefined;
    const result = evaluateNuiScript(resolvedSource, entry);

    this.interpreter = result.interpreter;
    this.windowJson = result.json;
    this.swapGroupId = result.swapGroupId;
    this.activeView = null;
    this.binds = discoverBinds(this.currentJson());
    this.formId = result.json && typeof result.json.id === "string" ? result.json.id : "";

    // Keep prior overrides that still apply (so editing the file doesn't wipe your
    // inspector edits); drop binds that no longer exist.
    const names = new Set(this.binds.map((b) => b.name));
    const carried: Record<string, any> = {};
    for (const k of Object.keys(this.bindValues)) if (names.has(k)) carried[k] = this.bindValues[k];
    this.bindValues = carried;

    const geo = result.geometry;
    this.curW = geo && geo.w > 50 ? Math.round(geo.w) : 500;
    this.curH = geo && geo.h > 50 ? Math.round(geo.h) : 600;

    this.panel.webview.postMessage({
      type: "update",
      layout: this.solveCurrent(),
      nuiJson: result.json,
      windowWidth: this.curW,
      windowHeight: this.curH,
      errors: result.errors.filter((e) => e && !e.startsWith("Unknown identifier")),
      functions: result.functions,
      selectedFunction: entry,
      views: result.views,
      binds: this.binds,
      bindValues: this.effectiveValues(),
      presets: this.allPresets().map((p) => p.name),
      customPresets: this.customPresets().map((p) => p.name),
    });
  }

  // ── Lifecycle / statics ──────────────────────────────────────────────────────

  public static async createOrShow(
    extensionUri: vscode.Uri,
    document: vscode.TextDocument,
    storage: vscode.Memento
  ): Promise<void> {
    const key = document.uri.toString();
    const existing = NuiPreviewPanel.panels.get(key);
    if (existing) {
      existing.panel.reveal(vscode.ViewColumn.Beside);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      NuiPreviewPanel.viewType,
      `NUI: ${this.basename(document.uri)}`,
      { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true },
      { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: [extensionUri] }
    );

    panel.iconPath = new vscode.ThemeIcon("preview");
    panel.webview.html = getWebviewContent(panel.webview, extensionUri);

    const instance = new NuiPreviewPanel(panel, document.uri, storage);
    NuiPreviewPanel.panels.set(key, instance);
  }

  /** Check if the active document looks like it contains NUI code. */
  public static containsNuiCode(document: vscode.TextDocument): boolean {
    const text = document.getText();
    return (
      /\bNuiWindow\s*\(/.test(text) ||
      /\bNuiCreate\s*\(/.test(text) ||
      /\bNUI_CreateForm\s*\(/.test(text) // TDN nui_i_library framework forms
    );
  }

  public static invalidateIncludes(): void {
    NuiPreviewPanel.includeResolver.invalidate();
  }

  private static basename(uri: vscode.Uri): string {
    return uri.path.split("/").pop() ?? "preview";
  }

  private dispose(): void {
    NuiPreviewPanel.panels.delete(this.documentUri.toString());
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    for (const d of this.disposables) d.dispose();
    this.disposables = [];
  }
}

/** Replace a group element's children with a swapped-in view layout. */
function applyToGroup(node: any, groupId: string, layout: any): boolean {
  if (!node || typeof node !== "object") return false;
  if (node.id === groupId && node.type === "group") {
    node.children = [layout];
    return true;
  }
  if (node.root && applyToGroup(node.root, groupId, layout)) return true;
  if (Array.isArray(node.children)) {
    for (const c of node.children) if (applyToGroup(c, groupId, layout)) return true;
  }
  return false;
}
