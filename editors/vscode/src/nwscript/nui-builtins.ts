// =============================================================================
// NUI Built-in Functions for NWScript Mini-Interpreter
// Auto-generated from nw_inc_nui.nss
// =============================================================================

// -----------------------------------------------------------------------------
// NUI Constants
// -----------------------------------------------------------------------------

export const NUI_CONSTANTS: Record<string, any> = {
  // Direction
  NUI_DIRECTION_HORIZONTAL: 0,
  NUI_DIRECTION_VERTICAL: 1,

  // Mouse buttons
  NUI_MOUSE_BUTTON_LEFT: 0,
  NUI_MOUSE_BUTTON_MIDDLE: 1,
  NUI_MOUSE_BUTTON_RIGHT: 2,

  // Scrollbars
  NUI_SCROLLBARS_NONE: 0,
  NUI_SCROLLBARS_X: 1,
  NUI_SCROLLBARS_Y: 2,
  NUI_SCROLLBARS_BOTH: 3,
  NUI_SCROLLBARS_AUTO: 4,

  // Aspect
  NUI_ASPECT_FIT: 0,
  NUI_ASPECT_FILL: 1,
  NUI_ASPECT_FIT100: 2,
  NUI_ASPECT_EXACT: 3,
  NUI_ASPECT_EXACTSCALED: 4,
  NUI_ASPECT_STRETCH: 5,

  // Horizontal alignment
  NUI_HALIGN_CENTER: 0,
  NUI_HALIGN_LEFT: 1,
  NUI_HALIGN_RIGHT: 2,

  // Vertical alignment
  NUI_VALIGN_MIDDLE: 0,
  NUI_VALIGN_TOP: 1,
  NUI_VALIGN_BOTTOM: 2,

  // Style sizes
  NUI_STYLE_PRIMARY_WIDTH: 150.0,
  NUI_STYLE_PRIMARY_HEIGHT: 50.0,
  NUI_STYLE_SECONDARY_WIDTH: 150.0,
  NUI_STYLE_SECONDARY_HEIGHT: 35.0,
  NUI_STYLE_TERTIARY_WIDTH: 100.0,
  NUI_STYLE_TERTIARY_HEIGHT: 30.0,
  NUI_STYLE_ROW_HEIGHT: 25.0,

  // Number flags
  NUI_NUMBER_FLAG_HEX: 0x001,

  // Text flags
  NUI_TEXT_FLAG_LOWERCASE: 0x001,
  NUI_TEXT_FLAG_UPPERCASE: 0x002,

  // Chart types
  NUI_CHART_TYPE_LINES: 0,
  NUI_CHART_TYPE_COLUMN: 1,

  // Draw list item types
  NUI_DRAW_LIST_ITEM_TYPE_POLYLINE: 0,
  NUI_DRAW_LIST_ITEM_TYPE_CURVE: 1,
  NUI_DRAW_LIST_ITEM_TYPE_CIRCLE: 2,
  NUI_DRAW_LIST_ITEM_TYPE_ARC: 3,
  NUI_DRAW_LIST_ITEM_TYPE_TEXT: 4,
  NUI_DRAW_LIST_ITEM_TYPE_IMAGE: 5,
  NUI_DRAW_LIST_ITEM_TYPE_LINE: 6,
  NUI_DRAW_LIST_ITEM_TYPE_RECT: 7,

  // Draw list item order
  NUI_DRAW_LIST_ITEM_ORDER_BEFORE: -1,
  NUI_DRAW_LIST_ITEM_ORDER_AFTER: 1,

  // Draw list item render conditions
  NUI_DRAW_LIST_ITEM_RENDER_ALWAYS: 0,
  NUI_DRAW_LIST_ITEM_RENDER_MOUSE_OFF: 1,
  NUI_DRAW_LIST_ITEM_RENDER_MOUSE_HOVER: 2,
  NUI_DRAW_LIST_ITEM_RENDER_MOUSE_LEFT: 3,
  NUI_DRAW_LIST_ITEM_RENDER_MOUSE_RIGHT: 4,
  NUI_DRAW_LIST_ITEM_RENDER_MOUSE_MIDDLE: 5,

  // NWScript boolean constants
  TRUE: 1,
  FALSE: 0,

  // Object constants
  OBJECT_SELF: 0x7F000000,
  OBJECT_INVALID: 0x7F000001,
};

// -----------------------------------------------------------------------------
// JSON Built-in Functions
// Maps NWScript's json type to native JavaScript values.
// NWScript json = native JS values (objects, arrays, strings, numbers, booleans, null)
// -----------------------------------------------------------------------------

export const jsonBuiltins: Record<string, (...args: any[]) => any> = {
  // JSON constants
  JSON_NULL: null as any, // handled specially below
  JSON_TRUE: true as any,
  JSON_FALSE: false as any,
  JSON_ARRAY: [] as any,
  JSON_OBJECT: {} as any,

  // Constructors
  JsonArray: () => [],
  JsonObject: () => ({}),
  JsonString: (s: string) => (s == null ? "" : String(s)),
  JsonInt: (n: number) => (n == null ? 0 : Math.trunc(Number(n))),
  JsonFloat: (f: number) => (f == null ? 0.0 : Number(f)),
  JsonBool: (b: any) => (b ? true : false),
  JsonNull: () => null,

  // Array operations
  JsonArrayInsert: (arr: any[], val: any) => {
    if (!Array.isArray(arr)) return [val];
    return [...arr, val];
  },
  JsonArrayInsertInplace: (arr: any[], val: any) => {
    if (!Array.isArray(arr)) return [val];
    arr.push(val);
    return arr;
  },
  JsonArrayGet: (arr: any[], idx: number) => {
    if (!Array.isArray(arr)) return null;
    return idx >= 0 && idx < arr.length ? arr[idx] : null;
  },
  JsonArrayDel: (arr: any[], idx: number) => {
    if (!Array.isArray(arr)) return arr;
    const copy = [...arr];
    if (idx >= 0 && idx < copy.length) {
      copy.splice(idx, 1);
    }
    return copy;
  },
  JsonArrayDelInplace: (arr: any[], idx: number) => {
    if (!Array.isArray(arr)) return arr;
    if (idx >= 0 && idx < arr.length) {
      arr.splice(idx, 1);
    }
    return arr;
  },
  JsonArraySet: (arr: any[], idx: number, val: any) => {
    if (!Array.isArray(arr)) return arr;
    const copy = [...arr];
    if (idx >= 0 && idx < copy.length) {
      copy[idx] = val;
    }
    return copy;
  },
  JsonArraySetInplace: (arr: any[], idx: number, val: any) => {
    if (!Array.isArray(arr)) return arr;
    if (idx >= 0 && idx < arr.length) {
      arr[idx] = val;
    }
    return arr;
  },

  // Object operations
  JsonObjectSet: (obj: any, key: string, val: any) => {
    if (obj == null || typeof obj !== "object" || Array.isArray(obj)) {
      return { [key]: val };
    }
    return { ...obj, [key]: val };
  },
  JsonObjectSetInplace: (obj: any, key: string, val: any) => {
    if (obj == null || typeof obj !== "object" || Array.isArray(obj)) {
      return { [key]: val };
    }
    obj[key] = val;
    return obj;
  },
  JsonObjectGet: (obj: any, key: string) => {
    if (obj == null || typeof obj !== "object" || Array.isArray(obj)) return null;
    return key in obj ? obj[key] : null;
  },
  JsonObjectDel: (obj: any, key: string) => {
    if (obj == null || typeof obj !== "object" || Array.isArray(obj)) return obj;
    const copy = { ...obj };
    delete copy[key];
    return copy;
  },
  JsonObjectDelInplace: (obj: any, key: string) => {
    if (obj == null || typeof obj !== "object" || Array.isArray(obj)) return obj;
    delete obj[key];
    return obj;
  },
  JsonObjectKeys: (obj: any) => {
    if (obj == null || typeof obj !== "object" || Array.isArray(obj)) return [];
    return Object.keys(obj);
  },

  // Extraction
  JsonGetString: (j: any) => {
    if (typeof j === "string") return j;
    if (j == null) return "";
    return String(j);
  },
  JsonGetInt: (j: any) => {
    if (typeof j === "number") return Math.trunc(j);
    if (typeof j === "boolean") return j ? 1 : 0;
    if (typeof j === "string") {
      const n = parseInt(j, 10);
      return isNaN(n) ? 0 : n;
    }
    return 0;
  },
  JsonGetFloat: (j: any) => {
    if (typeof j === "number") return j;
    if (typeof j === "boolean") return j ? 1.0 : 0.0;
    if (typeof j === "string") {
      const n = parseFloat(j);
      return isNaN(n) ? 0.0 : n;
    }
    return 0.0;
  },
  JsonGetLength: (j: any) => {
    if (Array.isArray(j)) return j.length;
    if (j != null && typeof j === "object") return Object.keys(j).length;
    return 0;
  },

  // Serialization
  JsonParse: (s: string) => {
    try {
      return JSON.parse(s);
    } catch {
      return null;
    }
  },
  JsonDump: (j: any) => {
    try {
      return JSON.stringify(j);
    } catch {
      return "null";
    }
  },

  // Type checking
  JsonGetType: (j: any) => {
    if (j === null || j === undefined) return 0; // JSON_TYPE_NULL
    if (typeof j === "boolean") return 6; // JSON_TYPE_BOOL (not standard but matches NWN)
    if (typeof j === "number") {
      return Number.isInteger(j) ? 1 : 2; // JSON_TYPE_INTEGER : JSON_TYPE_FLOAT
    }
    if (typeof j === "string") return 3; // JSON_TYPE_STRING
    if (Array.isArray(j)) return 5; // JSON_TYPE_ARRAY
    if (typeof j === "object") return 4; // JSON_TYPE_OBJECT
    return 0;
  },

  // Merge
  JsonMerge: (a: any, b: any) => {
    if (a != null && b != null && typeof a === "object" && typeof b === "object" && !Array.isArray(a) && !Array.isArray(b)) {
      return { ...a, ...b };
    }
    return b;
  },

  // Pointer (path access)
  JsonPointer: (j: any, pointer: string) => {
    if (!pointer || pointer === "/") return j;
    const parts = pointer.split("/").filter(Boolean);
    let cur = j;
    for (const part of parts) {
      if (cur == null) return null;
      if (Array.isArray(cur)) {
        const idx = parseInt(part, 10);
        cur = isNaN(idx) ? null : cur[idx];
      } else if (typeof cur === "object") {
        cur = cur[part];
      } else {
        return null;
      }
    }
    return cur ?? null;
  },

  // SetAt with pointer
  JsonSetAt: (j: any, pointer: string, val: any) => {
    if (!pointer || pointer === "/") return val;
    const copy = structuredClone(j);
    const parts = pointer.split("/").filter(Boolean);
    let cur = copy;
    for (let i = 0; i < parts.length - 1; i++) {
      const part = parts[i];
      if (Array.isArray(cur)) {
        cur = cur[parseInt(part, 10)];
      } else if (typeof cur === "object" && cur != null) {
        cur = cur[part];
      }
    }
    const lastPart = parts[parts.length - 1];
    if (Array.isArray(cur)) {
      cur[parseInt(lastPart, 10)] = val;
    } else if (typeof cur === "object" && cur != null) {
      cur[lastPart] = val;
    }
    return copy;
  },
};

// Expose JSON constants as proper values (not functions)
export const JSON_CONSTANTS: Record<string, any> = {
  JSON_NULL: null,
  JSON_TRUE: true,
  JSON_FALSE: false,
  JSON_ARRAY: [],
  JSON_OBJECT: {},
  JSON_STRING: "", // JSON_STRING is "" in NWN (empty string default)
};

// -----------------------------------------------------------------------------
// Internal helpers (mirror NWScript internal functions)
// -----------------------------------------------------------------------------

function NuiElement(sType: string, jLabel: any, jValue: any): any {
  const ret: any = { type: sType };
  if (jLabel != null) ret.label = jLabel;
  if (jValue != null) ret.value = jValue;
  return ret;
}

function NuiDrawListItem(
  nType: number,
  jEnabled: any,
  jColor: any,
  jFill: any,
  jLineThickness: any,
  nOrder: number = 1,
  nRender: number = 0,
  nBindArrays: number = 0
): any {
  const ret: any = { type: nType };
  if (jEnabled != null && jEnabled !== true) ret.enabled = jEnabled;
  if (jColor != null) ret.color = jColor;
  if (jFill != null && jFill !== false) ret.fill = jFill;
  if (jLineThickness != null && jLineThickness !== 1.0) ret.line_thickness = jLineThickness;
  if (nOrder !== 1) ret.order = nOrder;
  if (nRender !== 0) ret.render = nRender;
  if (nBindArrays) ret.arrayBinds = true;
  return ret;
}

// -----------------------------------------------------------------------------
// NUI Built-in Functions
// Direct port from nw_inc_nui.nss
// -----------------------------------------------------------------------------

export const nuiBuiltins: Record<string, (...args: any[]) => any> = {
  // Internal helper exposed for scripts that call it directly
  NuiElement: (sType: string, jLabel: any, jValue: any) => {
    return NuiElement(sType, jLabel, jValue);
  },

  // Internal draw list item helper
  NuiDrawListItem: (
    nType: number,
    jEnabled: any,
    jColor: any,
    jFill: any,
    jLineThickness: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    return NuiDrawListItem(nType, jEnabled, jColor, jFill, jLineThickness, nOrder, nRender, nBindArrays);
  },

  // ---------------------------------------------------------------------------
  // Window
  // ---------------------------------------------------------------------------

  NuiWindow: (
    jRoot: any,
    jTitle: any,
    jGeometry: any,
    jResizable: any,
    jCollapsed: any,
    jClosable: any,
    jTransparent: any,
    jBorder: any,
    jAcceptsInput: any = true,
    jSizeConstraint: any = null,
    jEdgeConstraint: any = null,
    jFont: any = ""
  ) => {
    const ret: any = { version: 1, root: jRoot };
    if (jTitle != null) ret.title = jTitle;
    ret.geometry = jGeometry;
    if (jResizable != null) ret.resizable = jResizable;
    if (jCollapsed != null) ret.collapsed = jCollapsed;
    if (jClosable != null) ret.closable = jClosable;
    if (jTransparent != null) ret.transparent = jTransparent;
    if (jBorder != null) ret.border = jBorder;
    // Only include non-default optional fields
    if (jAcceptsInput !== true) ret.accepts_input = jAcceptsInput;
    if (jSizeConstraint != null) ret.size_constraint = jSizeConstraint;
    if (jEdgeConstraint != null) ret.edge_constraint = jEdgeConstraint;
    if (jFont && jFont !== "") ret.font = jFont;
    return ret;
  },

  // ---------------------------------------------------------------------------
  // Values
  // ---------------------------------------------------------------------------

  NuiBind: (sId: string, nNumberFlags: number = 0, nNumberPrecision: number = 0, nTextFlags: number = 0) => {
    const ret: any = { bind: sId };
    if (nNumberFlags) ret.number_flags = nNumberFlags;
    if (nNumberPrecision) ret.number_precision = nNumberPrecision;
    if (nTextFlags) ret.text_flags = nTextFlags;
    return ret;
  },

  NuiId: (jElem: any, sId: string) => {
    if (jElem == null || typeof jElem !== "object") return { id: sId };
    return { ...jElem, id: sId };
  },

  NuiStrRef: (nStrRef: number) => {
    const ret: any = {};
    ret.strref = nStrRef;
    return ret;
  },

  // ---------------------------------------------------------------------------
  // Layout
  // ---------------------------------------------------------------------------

  NuiCol: (jList: any) => {
    const elem = NuiElement("col", null, null);
    return { ...elem, children: jList };
  },

  NuiRow: (jList: any) => {
    const elem = NuiElement("row", null, null);
    return { ...elem, children: jList };
  },

  NuiGroup: (jChild: any, bBorder: number = 1, nScroll: number = 4 /* NUI_SCROLLBARS_AUTO */) => {
    const ret = NuiElement("group", null, null);
    ret.children = [jChild];
    ret.border = bBorder ? true : false;
    ret.scrollbars = nScroll;
    return ret;
  },

  // ---------------------------------------------------------------------------
  // Modifiers / Attributes
  // ---------------------------------------------------------------------------

  NuiWidth: (jElem: any, fWidth: number) => {
    if (jElem == null || typeof jElem !== "object") return { width: fWidth };
    return { ...jElem, width: fWidth };
  },

  NuiHeight: (jElem: any, fHeight: number) => {
    if (jElem == null || typeof jElem !== "object") return { height: fHeight };
    return { ...jElem, height: fHeight };
  },

  NuiAspect: (jElem: any, fAspect: number) => {
    if (jElem == null || typeof jElem !== "object") return { aspect: fAspect };
    return { ...jElem, aspect: fAspect };
  },

  NuiMargin: (jElem: any, fMargin: number) => {
    if (jElem == null || typeof jElem !== "object") return { margin: fMargin };
    return { ...jElem, margin: fMargin };
  },

  NuiPadding: (jElem: any, fPadding: number) => {
    if (jElem == null || typeof jElem !== "object") return { padding: fPadding };
    return { ...jElem, padding: fPadding };
  },

  NuiEnabled: (jElem: any, jEnabler: any) => {
    if (jElem == null || typeof jElem !== "object") return { enabled: jEnabler };
    return { ...jElem, enabled: jEnabler };
  },

  NuiVisible: (jElem: any, jVisible: any) => {
    if (jElem == null || typeof jElem !== "object") return { visible: jVisible };
    return { ...jElem, visible: jVisible };
  },

  NuiTooltip: (jElem: any, jTooltip: any) => {
    if (jElem == null || typeof jElem !== "object") return { tooltip: jTooltip };
    return { ...jElem, tooltip: jTooltip };
  },

  NuiDisabledTooltip: (jElem: any, jTooltip: any) => {
    if (jElem == null || typeof jElem !== "object") return { disabled_tooltip: jTooltip };
    return { ...jElem, disabled_tooltip: jTooltip };
  },

  NuiEncouraged: (jElem: any, jEncouraged: any) => {
    if (jElem == null || typeof jElem !== "object") return { encouraged: jEncouraged };
    return { ...jElem, encouraged: jEncouraged };
  },

  // ---------------------------------------------------------------------------
  // Props & Style
  // ---------------------------------------------------------------------------

  NuiVec: (x: number, y: number) => {
    return { x, y };
  },

  NuiRect: (x: number, y: number, w: number, h: number) => {
    return { x, y, w, h };
  },

  NuiColor: (r: number, g: number, b: number, a: number = 255) => {
    return { r, g, b, a };
  },

  NuiStyleForegroundColor: (jElem: any, jColor: any) => {
    if (jElem == null || typeof jElem !== "object") return { foreground_color: jColor };
    return { ...jElem, foreground_color: jColor };
  },

  NuiStyleFont: (jElem: any, jFont: any) => {
    if (jElem == null || typeof jElem !== "object") return { font: jFont };
    return { ...jElem, font: jFont };
  },

  // ---------------------------------------------------------------------------
  // Widgets
  // ---------------------------------------------------------------------------

  NuiSpacer: () => {
    return NuiElement("spacer", null, null);
  },

  NuiLabel: (jValue: any, jHAlign: any, jVAlign: any) => {
    const ret = NuiElement("label", null, jValue);
    ret.text_halign = jHAlign;
    ret.text_valign = jVAlign;
    return ret;
  },

  NuiText: (jValue: any, bBorder: number = 1, nScroll: number = 4 /* NUI_SCROLLBARS_AUTO */) => {
    const ret = NuiElement("text", null, jValue);
    ret.border = bBorder ? true : false;
    ret.scrollbars = nScroll;
    return ret;
  },

  NuiButton: (jLabel: any) => {
    return NuiElement("button", jLabel, null);
  },

  NuiButtonImage: (jResRef: any) => {
    return NuiElement("button_image", jResRef, null);
  },

  NuiButtonSelect: (jLabel: any, jValue: any) => {
    return NuiElement("button_select", jLabel, jValue);
  },

  NuiCheck: (jLabel: any, jBool: any) => {
    return NuiElement("check", jLabel, jBool);
  },

  NuiImage: (jResRef: any, jAspect: any, jHAlign: any, jVAlign: any) => {
    const img = NuiElement("image", null, jResRef);
    img.image_aspect = jAspect;
    img.image_halign = jHAlign;
    img.image_valign = jVAlign;
    return img;
  },

  NuiImageRegion: (jImage: any, jRegion: any) => {
    if (jImage == null || typeof jImage !== "object") return { image_region: jRegion };
    return { ...jImage, image_region: jRegion };
  },

  NuiCombo: (jElements: any, jSelected: any) => {
    const elem = NuiElement("combo", null, jSelected);
    return { ...elem, elements: jElements };
  },

  NuiComboEntry: (sLabel: string, nValue: number) => {
    return [sLabel, nValue];
  },

  NuiSliderFloat: (jValue: any, jMin: any, jMax: any, jStepSize: any) => {
    const ret = NuiElement("sliderf", null, jValue);
    ret.min = jMin;
    ret.max = jMax;
    ret.step = jStepSize;
    return ret;
  },

  NuiSlider: (jValue: any, jMin: any, jMax: any, jStepSize: any) => {
    const ret = NuiElement("slider", null, jValue);
    ret.min = jMin;
    ret.max = jMax;
    ret.step = jStepSize;
    return ret;
  },

  NuiProgress: (jValue: any) => {
    return NuiElement("progress", null, jValue);
  },

  NuiTextEdit: (jPlaceholder: any, jValue: any, nMaxLength: number, bMultiline: number, bWordWrap: number = 1) => {
    const ret = NuiElement("textedit", jPlaceholder, jValue);
    ret.max = nMaxLength;
    ret.multiline = bMultiline ? true : false;
    ret.wordwrap = bWordWrap ? true : false;
    return ret;
  },

  NuiList: (
    jTemplate: any,
    jRowCount: any,
    fRowHeight: number = 25.0 /* NUI_STYLE_ROW_HEIGHT */,
    bBorder: number = 1,
    nScroll: number = 2 /* NUI_SCROLLBARS_Y */
  ) => {
    const ret = NuiElement("list", null, null);
    ret.row_template = jTemplate;
    ret.row_count = jRowCount;
    ret.row_height = fRowHeight;
    ret.border = bBorder ? true : false;
    ret.scrollbars = nScroll;
    return ret;
  },

  NuiListTemplateCell: (jElem: any, fWidth: number, bVariable: number) => {
    return [jElem, fWidth, bVariable ? true : false];
  },

  NuiColorPicker: (jColor: any) => {
    return NuiElement("color_picker", null, jColor);
  },

  NuiOptions: (nDirection: number, jElements: any, jValue: any) => {
    const ret = NuiElement("options", null, jValue);
    ret.direction = nDirection;
    ret.elements = jElements;
    return ret;
  },

  NuiToggles: (nDirection: number, jElements: any, jValue: any) => {
    const ret = NuiElement("tabbar", null, jValue);
    ret.direction = nDirection;
    ret.elements = jElements;
    return ret;
  },

  NuiChartSlot: (nType: number, jLegend: any, jColor: any, jData: any) => {
    const ret: any = {};
    ret.type = nType;
    ret.legend = jLegend;
    ret.color = jColor;
    ret.data = jData;
    return ret;
  },

  NuiChart: (jSlots: any) => {
    return NuiElement("chart", null, jSlots);
  },

  // ---------------------------------------------------------------------------
  // Draw Lists
  // ---------------------------------------------------------------------------

  NuiDrawListPolyLine: (
    jEnabled: any,
    jColor: any,
    jFill: any,
    jLineThickness: any,
    jPoints: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    const ret = NuiDrawListItem(0 /* POLYLINE */, jEnabled, jColor, jFill, jLineThickness, nOrder, nRender, nBindArrays);
    ret.points = jPoints;
    return ret;
  },

  NuiDrawListCurve: (
    jEnabled: any,
    jColor: any,
    jLineThickness: any,
    jA: any,
    jB: any,
    jCtrl0: any,
    jCtrl1: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    const ret = NuiDrawListItem(1 /* CURVE */, jEnabled, jColor, false, jLineThickness, nOrder, nRender, nBindArrays);
    ret.a = jA;
    ret.b = jB;
    ret.ctrl0 = jCtrl0;
    ret.ctrl1 = jCtrl1;
    return ret;
  },

  NuiDrawListCircle: (
    jEnabled: any,
    jColor: any,
    jFill: any,
    jLineThickness: any,
    jRect: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    const ret = NuiDrawListItem(2 /* CIRCLE */, jEnabled, jColor, jFill, jLineThickness, nOrder, nRender, nBindArrays);
    ret.rect = jRect;
    return ret;
  },

  NuiDrawListArc: (
    jEnabled: any,
    jColor: any,
    jFill: any,
    jLineThickness: any,
    jCenter: any,
    jRadius: any,
    jAMin: any,
    jAMax: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    const ret = NuiDrawListItem(3 /* ARC */, jEnabled, jColor, jFill, jLineThickness, nOrder, nRender, nBindArrays);
    ret.c = jCenter;
    ret.radius = jRadius;
    ret.amin = jAMin;
    ret.amax = jAMax;
    return ret;
  },

  NuiDrawListText: (
    jEnabled: any,
    jColor: any,
    jRect: any,
    jText: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0,
    jFont: any = ""
  ) => {
    const ret = NuiDrawListItem(4 /* TEXT */, jEnabled, jColor, null, null, nOrder, nRender, nBindArrays);
    ret.rect = jRect;
    ret.text = jText;
    // NuiStyleFont sets "font" key
    ret.font = jFont;
    return ret;
  },

  NuiDrawListImage: (
    jEnabled: any,
    jResRef: any,
    jRect: any,
    jAspect: any,
    jHAlign: any,
    jVAlign: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    const ret = NuiDrawListItem(5 /* IMAGE */, jEnabled, null, null, null, nOrder, nRender, nBindArrays);
    ret.image = jResRef;
    ret.rect = jRect;
    ret.image_aspect = jAspect;
    ret.image_halign = jHAlign;
    ret.image_valign = jVAlign;
    return ret;
  },

  NuiDrawListImageRegion: (jDrawListImage: any, jRegion: any) => {
    if (jDrawListImage == null || typeof jDrawListImage !== "object") return { image_region: jRegion };
    return { ...jDrawListImage, image_region: jRegion };
  },

  NuiDrawListLine: (
    jEnabled: any,
    jColor: any,
    jLineThickness: any,
    jA: any,
    jB: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    const ret = NuiDrawListItem(6 /* LINE */, jEnabled, jColor, null, jLineThickness, nOrder, nRender, nBindArrays);
    ret.a = jA;
    ret.b = jB;
    return ret;
  },

  NuiDrawListRect: (
    jEnabled: any,
    jColor: any,
    jFill: any,
    jLineThickness: any,
    jRect: any,
    nOrder: number = 1,
    nRender: number = 0,
    nBindArrays: number = 0
  ) => {
    const ret = NuiDrawListItem(7 /* RECT */, jEnabled, jColor, jFill, jLineThickness, nOrder, nRender, nBindArrays);
    ret.rect = jRect;
    return ret;
  },

  NuiDrawList: (jElem: any, jScissor: any, jList: any) => {
    if (jElem == null || typeof jElem !== "object") {
      return { draw_list: jList, draw_list_scissor: jScissor };
    }
    const ret = { ...jElem, draw_list: jList };
    ret.draw_list_scissor = jScissor;
    return ret;
  },
};

// -----------------------------------------------------------------------------
// Engine Mock Functions
// Stubs for game engine functions that NUI scripts may call
// -----------------------------------------------------------------------------

export const engineMocks: Record<string, (...args: any[]) => any> = {
  // Object info
  GetName: (_obj?: any, _bOriginal?: any) => "MockObject",
  GetTag: (_obj?: any) => "mock_tag",
  GetResRef: (_obj?: any) => "mock_resref",
  GetObjectByTag: (_sTag?: string, _nNth?: number) => 0x7f000000,
  GetPCSpeaker: () => 0x7f000000,
  GetFirstPC: () => 0x7f000000,
  GetNextPC: () => 0x7f000001,
  GetModule: () => 0x7f000000,
  GetArea: (_obj?: any) => 0x7f000000,

  // Object validity and type
  GetIsObjectValid: (_obj?: any) => 1,
  GetObjectType: (_obj?: any) => 5, // OBJECT_TYPE_CREATURE
  GetIsDM: (_obj?: any) => 1,
  GetIsDMPossessed: (_obj?: any) => 0,
  GetIsPC: (_obj?: any) => 1,

  // NUI-specific engine functions
  NuiFindWindow: (_oPC?: any, _sId?: string) => 0,
  NuiGetBind: (_oPC?: any, _nToken?: number, _sBind?: string) => null,
  NuiSetBind: (_oPC?: any, _nToken?: number, _sBind?: string, _jValue?: any) => {},
  NuiCreate: (_oPC?: any, _jNui?: any, _sId?: string) => 0,
  NuiDestroy: (_oPC?: any, _nToken?: number) => {},
  NuiGetNthWindow: (_oPC?: any, _nIdx?: number) => 0,
  NuiGetWindowToken: (_oPC?: any, _sId?: string) => 0,
  NuiGetEventType: () => 0,
  NuiGetEventElement: () => "",
  NuiGetEventArrayIndex: () => 0,
  NuiGetEventWindow: () => "",
  NuiGetEventPayload: () => null,
  NuiSetBindWatch: (_oPC?: any, _nToken?: number, _sBind?: string, _bWatch?: number) => {},
  NuiSetGroupLayout: (_oPC?: any, _nToken?: number, _sGroup?: string, _jLayout?: any) => {},
  NuiScrollTo: (_oPC?: any, _nToken?: number, _sElem?: string, _nDir?: number, _nPos?: number) => {},

  // Type conversions
  IntToString: (n: any) => String(Math.trunc(Number(n) || 0)),
  FloatToString: (f: any, nWidth: number = 0, nDecimals: number = 3) => {
    const val = Number(f) || 0.0;
    if (nWidth > 0) {
      return val.toFixed(nDecimals).padStart(nWidth);
    }
    return val.toFixed(nDecimals);
  },
  StringToInt: (s: any) => {
    const n = parseInt(String(s), 10);
    return isNaN(n) ? 0 : n;
  },
  StringToFloat: (s: any) => {
    const n = parseFloat(String(s));
    return isNaN(n) ? 0.0 : n;
  },
  IntToFloat: (n: any) => Number(n) || 0.0,
  FloatToInt: (f: any) => Math.trunc(Number(f) || 0),

  // String functions
  GetStringLength: (s: any) => String(s ?? "").length,
  GetStringLeft: (s: any, nCount: number) => String(s ?? "").substring(0, nCount),
  GetStringRight: (s: any, nCount: number) => {
    const str = String(s ?? "");
    return str.substring(str.length - nCount);
  },
  GetSubString: (s: any, nStart: number, nCount: number) => String(s ?? "").substring(nStart, nStart + nCount),
  GetStringLowerCase: (s: any) => String(s ?? "").toLowerCase(),
  GetStringUpperCase: (s: any) => String(s ?? "").toUpperCase(),
  FindSubString: (s: any, sSub: any, nStart: number = 0) => String(s ?? "").indexOf(String(sSub ?? ""), nStart),

  // Math
  abs: (n: any) => Math.abs(Number(n) || 0),
  fabs: (f: any) => Math.abs(Number(f) || 0),
  cos: (f: any) => Math.cos(Number(f) || 0),
  sin: (f: any) => Math.sin(Number(f) || 0),
  tan: (f: any) => Math.tan(Number(f) || 0),
  acos: (f: any) => Math.acos(Number(f) || 0),
  asin: (f: any) => Math.asin(Number(f) || 0),
  atan: (f: any) => Math.atan(Number(f) || 0),
  log: (f: any) => Math.log(Number(f) || 0),
  pow: (f: any, e: any) => Math.pow(Number(f) || 0, Number(e) || 0),
  sqrt: (f: any) => Math.sqrt(Number(f) || 0),

  // Random (deterministic in preview)
  Random: (nMax: any) => 0,
  d2: (_nDice: number = 1) => 1,
  d3: (_nDice: number = 1) => 1,
  d4: (_nDice: number = 1) => 1,
  d6: (_nDice: number = 1) => 1,
  d8: (_nDice: number = 1) => 1,
  d10: (_nDice: number = 1) => 1,
  d12: (_nDice: number = 1) => 1,
  d20: (_nDice: number = 1) => 1,
  d100: (_nDice: number = 1) => 1,

  // Player / character info stubs
  GetLevelByPosition: (_nPos?: number, _oCreature?: any) => 10,
  GetClassByPosition: (_nPos?: number, _oCreature?: any) => 0,
  GetHitDice: (_oCreature?: any) => 10,
  GetAbilityScore: (_oCreature?: any, _nAbility?: number, _bBase?: number) => 10,
  GetAbilityModifier: (_oCreature?: any, _nAbility?: number) => 0,
  GetSkillRank: (_nSkill?: number, _oCreature?: any, _bBase?: number) => 5,
  GetHasSkill: (_nSkill?: number, _oCreature?: any) => 1,
  GetHasFeat: (_nFeat?: number, _oCreature?: any) => 1,
  GetGold: (_oCreature?: any) => 1000,
  GetXP: (_oCreature?: any) => 45000,
  GetRacialType: (_oCreature?: any) => 0,
  GetGender: (_oCreature?: any) => 0,
  GetAlignmentLawChaos: (_oCreature?: any) => 50,
  GetAlignmentGoodEvil: (_oCreature?: any) => 50,
  GetDeity: (_oCreature?: any) => "Tempus",
  GetSubRace: (_oCreature?: any) => "",
  GetDescription: (_obj?: any, _bOriginal?: number, _bIdentified?: number) => "A mock object.",
  GetPortraitResRef: (_oCreature?: any) => "po_hu_m_01_",

  // Item stubs
  GetItemInSlot: (_nSlot?: number, _oCreature?: any) => 0x7f000000,
  GetBaseItemType: (_oItem?: any) => 0,
  GetIdentified: (_oItem?: any) => 1,
  GetItemStackSize: (_oItem?: any) => 1,
  GetItemCharges: (_oItem?: any) => 0,

  // Local variables
  GetLocalInt: (_oObj?: any, _sVar?: string) => 0,
  GetLocalFloat: (_oObj?: any, _sVar?: string) => 0.0,
  GetLocalString: (_oObj?: any, _sVar?: string) => "",
  GetLocalObject: (_oObj?: any, _sVar?: string) => 0x7f000001,
  GetLocalJson: (_oObj?: any, _sVar?: string) => null,
  SetLocalInt: (_oObj?: any, _sVar?: string, _nVal?: number) => {},
  SetLocalFloat: (_oObj?: any, _sVar?: string, _fVal?: number) => {},
  SetLocalString: (_oObj?: any, _sVar?: string, _sVal?: string) => {},
  SetLocalObject: (_oObj?: any, _sVar?: string, _oVal?: any) => {},
  SetLocalJson: (_oObj?: any, _sVar?: string, _jVal?: any) => {},
  DeleteLocalInt: (_oObj?: any, _sVar?: string) => {},
  DeleteLocalFloat: (_oObj?: any, _sVar?: string) => {},
  DeleteLocalString: (_oObj?: any, _sVar?: string) => {},
  DeleteLocalObject: (_oObj?: any, _sVar?: string) => {},
  DeleteLocalJson: (_oObj?: any, _sVar?: string) => {},

  // Misc stubs for commonly called functions in NUI scripts
  SendMessageToPC: (_oPC?: any, _sMsg?: string) => {},
  SendMessageToAllDMs: (_sMsg?: string) => {},
  PrintString: (_s?: string) => {},
  WriteTimestampedLogEntry: (_s?: string) => {},
  GetPCPlayerName: (_oPC?: any) => "MockPlayer",
  GetPCPublicCDKey: (_oPC?: any, _bSingle?: number) => "AAAA0000",
  ObjectToString: (_oObj?: any) => "7f000000",
  StringToObject: (_s?: string) => 0x7f000000,
  GetIsInCombat: (_oCreature?: any) => 0,
  GetCurrentHitPoints: (_oCreature?: any) => 50,
  GetMaxHitPoints: (_oCreature?: any) => 50,

  // Color/formatting
  GetColorCode: (_r?: number, _g?: number, _b?: number) => "<c>",

  // NWNX functions (common stubs — these appear in included NWNX wrappers)
  NWNXPushInt: (_n?: number) => {},
  NWNXPushFloat: (_f?: number) => {},
  NWNXPushString: (_s?: string) => {},
  NWNXPushObject: (_o?: any) => {},
  NWNXPushJson: (_j?: any) => {},
  NWNXCall: (_sPlugin?: string, _sFunc?: string) => {},
  NWNXPopInt: () => 0,
  NWNXPopFloat: () => 0.0,
  NWNXPopString: () => "",
  NWNXPopObject: () => 0x7f000000,
  NWNXPopJson: () => null,
  NWNXGetIsAvailable: () => 1,
  NWNX_PushArgumentInt: (_sPlugin?: string, _n?: number) => {},
  NWNX_PushArgumentFloat: (_sPlugin?: string, _f?: number) => {},
  NWNX_PushArgumentString: (_sPlugin?: string, _s?: string) => {},
  NWNX_PushArgumentObject: (_sPlugin?: string, _o?: any) => {},
  NWNX_CallFunction: (_sPlugin?: string, _sFunc?: string) => {},
  NWNX_GetReturnValueInt: (_sPlugin?: string) => 0,
  NWNX_GetReturnValueFloat: (_sPlugin?: string) => 0.0,
  NWNX_GetReturnValueString: (_sPlugin?: string) => "",
  NWNX_GetReturnValueObject: (_sPlugin?: string) => 0x7f000000,

  // Effect/itemproperty stubs (return placeholder values)
  EffectAttackIncrease: (..._a: any[]) => 0,
  EffectAttackDecrease: (..._a: any[]) => 0,
  EffectDamageIncrease: (..._a: any[]) => 0,
  EffectDamageDecrease: (..._a: any[]) => 0,
  EffectTemporaryHitpoints: (..._a: any[]) => 0,
  EffectHeal: (..._a: any[]) => 0,
  EffectDamage: (..._a: any[]) => 0,
  EffectAbilityIncrease: (..._a: any[]) => 0,
  EffectAbilityDecrease: (..._a: any[]) => 0,
  EffectACIncrease: (..._a: any[]) => 0,
  EffectACDecrease: (..._a: any[]) => 0,
  EffectSavingThrowIncrease: (..._a: any[]) => 0,
  EffectSavingThrowDecrease: (..._a: any[]) => 0,
  EffectSkillIncrease: (..._a: any[]) => 0,
  EffectSkillDecrease: (..._a: any[]) => 0,
  EffectVisualEffect: (..._a: any[]) => 0,
  EffectLinkEffects: (..._a: any[]) => 0,
  SupernaturalEffect: (..._a: any[]) => 0,
  ExtraordinaryEffect: (..._a: any[]) => 0,
  ApplyEffectToObject: (..._a: any[]) => {},
  RemoveEffect: (..._a: any[]) => {},
  GetFirstEffect: (..._a: any[]) => 0,
  GetNextEffect: (..._a: any[]) => 0,
  GetIsEffectValid: (..._a: any[]) => 0,
  GetEffectType: (..._a: any[]) => 0,
  ItemPropertyCustom: (..._a: any[]) => 0,
  GetItemPropertyType: (..._a: any[]) => 0,

  // Action/command stubs
  DelayCommand: (_f?: number, _a?: any) => {},
  AssignCommand: (_o?: any, _a?: any) => {},
  ActionDoCommand: (_a?: any) => {},
  ExecuteScript: (..._a: any[]) => {},
  ClearAllActions: (..._a: any[]) => {},
  EnterTargetingMode: (..._a: any[]) => {},

  // 2DA / string ref
  Get2DAString: (_s2DA?: string, _sCol?: string, _nRow?: number) => "",
  GetStringByStrRef: (_nStrRef?: number) => "StrRef:" + String(_nStrRef ?? 0),
  GetPlayerDeviceProperty: (_oPC?: any, _nProp?: number) => 1920,

  // SQL stubs
  SqlPrepareQueryCampaign: (..._a: any[]) => ({ _mock: true }),
  SqlStep: (_q?: any) => 0,
  SqlGetString: (_q?: any, _n?: number) => "",
  SqlGetInt: (_q?: any, _n?: number) => 0,
  SqlGetFloat: (_q?: any, _n?: number) => 0.0,
  SqlBindInt: (..._a: any[]) => {},
  SqlBindFloat: (..._a: any[]) => {},
  SqlBindString: (..._a: any[]) => {},

  // Persistent storage stubs (used for window position saving)
  GetDMPersistentFloat: (..._a: any[]) => 0.0,
  SetDMPersistentFloat: (..._a: any[]) => {},
  GetDMPersistentInt: (..._a: any[]) => 0,
  SetDMPersistentInt: (..._a: any[]) => {},
  GetDMPersistentString: (..._a: any[]) => "",
  SetDMPersistentString: (..._a: any[]) => {},
  tdn_GetPersistentFloat: (..._a: any[]) => 0.0,
  tdn_SetPersistentFloat: (..._a: any[]) => {},
  tdn_GetPersistentInt: (..._a: any[]) => 0,
  tdn_SetPersistentInt: (..._a: any[]) => {},
  tdn_GetPersistentString: (..._a: any[]) => "",
  tdn_SetPersistentString: (..._a: any[]) => {},

  // OBJECT_SELF accessor
  OBJECT_SELF: 0x7f000000 as any,
};
