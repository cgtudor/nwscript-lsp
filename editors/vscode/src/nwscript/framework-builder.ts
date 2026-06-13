// =============================================================================
// TDN NUI Framework Builder
// =============================================================================
//
// The TDN "nui_i_library" framework (core/nui/nui_i_main.nss) does NOT build its
// window JSON the way vanilla nw_inc_nui.nss does (functional composition via
// NuiCol/NuiRow/...). Instead it builds the window incrementally by issuing
// SQLite statements of the form:
//
//     UPDATE nui_forms SET definition =
//         (SELECT json_set(definition, '<path>', json(@value)) ...)
//
// where <path> is a string JSON-path (e.g. "$.root.children[#-1].children[#]")
// tracked in module-local variables, and an "increment flag" decides whether the
// next element descends into the last-added container.
//
// Rather than embed a SQLite engine, we let the framework's REAL public NUI_*
// function bodies run inside the mini-interpreter and intercept only the private
// engine helpers (nui_SetObject, nui_IncrementPath, the flag/state accessors,
// nui_SaveForm, ...). This class is a faithful, line-for-line port of that engine
// operating on an in-memory JS object that mirrors the `definition` column. The
// public bodies keep doing the type-dependent property-name logic (e.g.
// NUI_BindLabel choosing "value" vs "label"), so the produced JSON matches the
// engine NUI format the layout solver already understands.
//
// Vanilla forms never call NUI_*/nui_*, so none of this activates for them.
// =============================================================================

// ── SQLite-style JSON path support (json_set / json_extract subset) ──────────

type PathToken = { key: string } | { idx: string };

/** Parse a path like "$.root.children[#-1].elements[#]" into ordered tokens. */
function parsePath(path: string): PathToken[] {
  const tokens: PathToken[] = [];
  // Strip leading "$"
  let i = 0;
  if (path[0] === "$") i = 1;
  const re = /\.([^.[\]]+)|\[([^\]]+)\]/g;
  re.lastIndex = i;
  let m: RegExpExecArray | null;
  while ((m = re.exec(path)) !== null) {
    if (m[1] !== undefined) tokens.push({ key: m[1] });
    else tokens.push({ idx: m[2] });
  }
  return tokens;
}

/** Resolve an array index token ("#" = append/len, "#-1" = last, n = literal). */
function resolveIndex(token: string, len: number): number {
  if (token === "#") return len; // append position
  if (token === "#-1") return len - 1; // last element
  const n = parseInt(token, 10);
  return isNaN(n) ? -1 : n;
}

/**
 * Mirror of SQLite's json_set(root, path, value): navigates `path` (intermediate
 * nodes must already exist, which the framework guarantees by building top-down)
 * and sets/appends the leaf. "[#]" appends to an array.
 */
function jsonSet(root: any, path: string, value: any): void {
  if (root == null || typeof root !== "object") return;
  const tokens = parsePath(path);
  if (tokens.length === 0) return;

  let cur: any = root;
  for (let t = 0; t < tokens.length - 1; t++) {
    const tok = tokens[t];
    if ("key" in tok) {
      cur = cur[tok.key];
    } else {
      if (!Array.isArray(cur)) return;
      cur = cur[resolveIndex(tok.idx, cur.length)];
    }
    if (cur == null || typeof cur !== "object") return;
  }

  const last = tokens[tokens.length - 1];
  if ("key" in last) {
    cur[last.key] = value;
  } else {
    if (!Array.isArray(cur)) return;
    if (last.idx === "#") {
      cur.push(value);
    } else {
      const i = resolveIndex(last.idx, cur.length);
      if (i >= 0) cur[i] = value;
    }
  }
}

/** Mirror of SQLite's json_extract(root, path): returns the value at `path`. */
function jsonExtract(root: any, path: string): any {
  if (root == null) return null;
  const tokens = parsePath(path);
  let cur: any = root;
  for (const tok of tokens) {
    if (cur == null || typeof cur !== "object") return null;
    if ("key" in tok) cur = cur[tok.key];
    else {
      if (!Array.isArray(cur)) return null;
      cur = cur[resolveIndex(tok.idx, cur.length)];
    }
  }
  return cur ?? null;
}

// ── The builder (port of the nui_i_main.nss private engine) ──────────────────

export class FrameworkBuilder {
  /** The form table: formId -> definition object (mirrors the nui_forms table). */
  private forms = new Map<string, any>();

  // Build state (mirrors the module-local variables in nui_i_main.nss).
  private path = "$";
  private incrementFlag = false;
  private drawlistFlag = false;
  private listboxFlag = false;
  private definitionFlag = false;
  private controlType = "";
  private entryCount = 0;
  private formId = "";
  private formfile = "";

  // ── String helpers (port of nui_RegExp* / nui_*SubString) ──────────────────

  private regExpReplaceAll(str: string, token: string, sub: string): string {
    return str.split(token).join(sub);
  }

  /** Port of nui_RegExpReplaceLast: truncate from the last `token` and append `sub`. */
  private regExpReplaceLast(token: string, str: string, sub: string): string {
    const i = str.lastIndexOf(token);
    return i < 0 ? str : str.slice(0, i) + sub;
  }

  /** Port of nui_RegExpMatch: GLOB "*row_template???" or "*row_template?????". */
  private regExpMatch(str: string): boolean {
    return /row_template(.{3}|.{5})$/.test(str);
  }

  // ── Path helpers ───────────────────────────────────────────────────────────

  getPath(): string {
    return this.path;
  }
  setPath(p: string): string {
    this.path = p;
    return p;
  }
  resetPath(): void {
    this.setPath("$");
  }

  substitutePath(sub: string): string {
    return this.setPath(this.regExpReplaceAll(this.path, "@", sub));
  }
  getSubstitutedPath(sub: string): string {
    return this.regExpReplaceAll(this.path, "@", sub);
  }

  getGroupKey(): string {
    return this.regExpMatch(this.path) ? "row_template" : "";
  }

  /** Port of nui_IncrementPath. */
  incrementPath(sElement = "", bForce = false): string {
    if (!this.incrementFlag && !bForce) return this.path;
    this.toggleIncrementFlag();

    let sPath = this.path;
    if (sPath === "$") {
      sPath += ".root";
    } else {
      sPath = this.substitutePath("#-1");
      if (this.getGroupKey() === "row_template" && (this.controlType === "group" || sElement === "draw_list")) {
        sPath += "[0]";
      }
      if (sElement !== "draw_list") {
        sPath += this.controlType === "listbox" ? ".row_template[@]" : ".children[@]";
      } else {
        sPath += ".draw_list[@]";
      }
    }
    return this.setPath(sPath);
  }

  /** Port of nui_DecrementPath. */
  decrementPath(n = 1): string {
    let sPath = this.path;
    while (n-- > 0) sPath = this.setPath(this.regExpReplaceLast("[#-1]", this.path, "[@]"));
    return sPath;
  }

  // ── Flag / state accessors (port of nui_Toggle*/nui_Get*/nui_Set*) ─────────

  private toggleFlag(current: boolean, n: number): boolean {
    const val = n === -1 ? !current : !!n;
    return val;
  }

  toggleIncrementFlag(n = -1): number {
    this.incrementFlag = this.toggleFlag(this.incrementFlag, n);
    return this.incrementFlag ? 1 : 0;
  }
  getIncrementFlag(): number {
    return this.incrementFlag ? 1 : 0;
  }

  toggleDrawlistFlag(n = -1): number {
    this.drawlistFlag = this.toggleFlag(this.drawlistFlag, n);
    return this.drawlistFlag ? 1 : 0;
  }
  getDrawlistFlag(): number {
    return this.drawlistFlag ? 1 : 0;
  }

  toggleListboxFlag(n = -1): number {
    this.listboxFlag = this.toggleFlag(this.listboxFlag, n);
    return this.listboxFlag ? 1 : 0;
  }
  getListboxFlag(): number {
    return this.listboxFlag ? 1 : 0;
  }

  toggleDefinitionFlag(n = -1): number {
    this.definitionFlag = this.toggleFlag(this.definitionFlag, n);
    return this.definitionFlag ? 1 : 0;
  }
  getDefinitionFlag(): number {
    return this.definitionFlag ? 1 : 0;
  }

  setControlType(s: string): void {
    this.controlType = s;
  }
  getControlType(): string {
    return this.controlType;
  }

  getEntryCount(): number {
    return this.entryCount;
  }
  resetEntryCount(): void {
    this.entryCount = 0;
  }
  incrementEntryCount(inc = 1): number {
    this.entryCount += inc;
    return this.entryCount;
  }

  setFormId(s: string): void {
    this.formId = s;
  }
  getFormId(): string {
    return this.formId;
  }
  setFormfile(s: string): void {
    this.formfile = s;
  }
  getFormfile(): string {
    return this.formfile;
  }

  clearVariables(): void {
    this.incrementFlag = false;
    this.drawlistFlag = false;
    this.listboxFlag = false;
    this.entryCount = 0;
    this.controlType = "";
    this.path = "$";
    this.formId = "";
  }

  // ── Form storage (port of nui_SaveForm / nui_DeleteForm / read-back) ────────

  saveForm(id: string, sJson: string): void {
    let obj: any;
    try {
      obj = JSON.parse(sJson);
    } catch {
      obj = {};
    }
    this.forms.set(id, obj);
  }

  deleteForm(id: string): void {
    this.forms.delete(id);
  }

  getForm(id: string): any {
    return this.forms.get(id) ?? null;
  }

  getDefinitionValue(id: string, path = ""): any {
    const obj = this.forms.get(id);
    if (obj == null) return "";
    return jsonExtract(obj, "$" + (path === "" ? "" : "." + path));
  }

  private createRowTemplate(controlJson: string): string {
    // Port of nui_CreateRowTemplate: [<control>, 25.0, true]
    return "[" + controlJson + ",25.0,true]";
  }

  // ── The choke-point (port of nui_SetObject) ─────────────────────────────────

  /**
   * sProperty === ""  -> add a control (control branch)
   * sProperty !== ""  -> set a property on the last element (property branch)
   */
  setObject(sProperty: string, sValue: string, sType = ""): void {
    const formObj = this.forms.get(this.formId);
    if (formObj == null) return;

    let sPath: string;

    if (sProperty !== "") {
      sPath = this.getSubstitutedPath("#-1");

      if (this.getGroupKey() === "row_template") {
        if (sProperty === "NUI_TEMPLATE_WIDTH") sPath += "[1]";
        else if (sProperty === "NUI_TEMPLATE_VARIABLE") sPath += "[2]";
        else sPath += "[0]." + sProperty;
      } else if (sProperty === "NUI_ELEMENT") {
        sPath += ".elements[#]";
      } else if (sProperty === "NUI_SERIES") {
        sPath += ".value[#]";
      } else {
        sPath += "." + sProperty;
      }
    } else {
      this.incrementPath(sType);

      if (this.getGroupKey() === "row_template") sValue = this.createRowTemplate(sValue);

      if (sType !== "") {
        if (sType === "combo" || sType === "options" || sType === "tabbar") this.resetEntryCount();
        this.setControlType(sType);
      }

      sPath = this.getSubstitutedPath("#");
    }

    let parsed: any;
    try {
      parsed = JSON.parse(sValue);
    } catch {
      return; // malformed value; skip rather than corrupt the tree
    }

    jsonSet(formObj, sPath, parsed);
  }

  /** The primary/main form built during DefineForm (its root holds the layout). */
  getMainForm(): any {
    return this.formId ? this.forms.get(this.formId) ?? null : null;
  }
}
