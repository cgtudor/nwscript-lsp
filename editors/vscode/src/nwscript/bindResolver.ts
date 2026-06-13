// =============================================================================
// NUI Bind Resolution
// =============================================================================
//
// NUI windows leave many properties as bind objects — `{ "bind": "pc_name" }` —
// that the engine fills at runtime via NuiSetBind. With no runtime, the preview
// would otherwise render every such slot as a literal "[pc_name]" token.
//
// This module:
//   1. discoverBinds()  — walks the window JSON and reports every bind, inferring
//      a semantic *kind* from the property slot it sits in and the element type
//      (so the inspector can show the right editor and we can pick a good default).
//   2. resolveBinds()   — returns a clone of the window JSON with each bind object
//      replaced by a concrete value, drawn from a supplied value map and falling
//      back to a slot/name-aware placeholder. Layout-affecting binds like
//      `row_count` resolve to numbers so the solver renders a realistic row count.
//   3. presets          — built-in named value sets (Typical / Empty / Overflow /
//      Max rows) computed from the discovered binds.
//
// Works for both vanilla nw_inc_nui.nss forms and the TDN framework, since both
// emit the same engine bind shape.
// =============================================================================

export type BindKind = 'string' | 'bool' | 'number' | 'rows' | 'color' | 'array' | 'resref';

export interface BindInfo {
  /** Bind variable name (the value of the "bind" key). */
  name: string;
  /** Inferred semantic kind, used to pick a default and an inspector editor. */
  kind: BindKind;
  /** The JSON property the bind sits in (e.g. "value", "row_count", "visible"). */
  slot: string;
  /** The owning element's type (e.g. "label", "list"), or "" for the form root. */
  elementType: string;
}

/** Property slots that are structural/geometry/draw-list — not user-facing binds. */
const SKIP_SLOTS = new Set([
  'geometry', 'rect', 'points', 'c', 'a', 'b', 'ctrl0', 'ctrl1',
  'draw_list', 'draw_list_scissor', 'image_region',
]);

const BOOL_SLOTS = new Set([
  'visible', 'enabled', 'encouraged', 'border', 'fill', 'resizable', 'closable',
  'collapsed', 'accepts_input', 'transparent', 'multiline', 'wordwrap', 'scissor',
]);

const NUMBER_SLOTS = new Set([
  'min', 'max', 'step', 'aspect', 'image_aspect', 'image_halign', 'image_valign',
  'margin', 'padding', 'width', 'height', 'row_height', 'line_thickness', 'radius',
]);

const STRING_SLOTS = new Set([
  'label', 'text', 'placeholder', 'tooltip', 'disabled_tooltip', 'legend', 'title',
]);

function isBindObject(v: any): v is { bind: string } {
  return v != null && typeof v === 'object' && !Array.isArray(v) && typeof v.bind === 'string';
}

/** Infer the semantic kind of a bind from its slot and owning element type. */
function classify(slot: string, elementType: string): BindKind | null {
  if (SKIP_SLOTS.has(slot)) return null;
  if (slot.endsWith('_selected')) return 'bool';
  if (slot === 'row_count') return 'rows';
  if (slot === 'color' || slot === 'foreground_color') return 'color';
  if (slot === 'elements') return 'array';
  if (slot === 'image') return 'resref';
  if (BOOL_SLOTS.has(slot)) return 'bool';
  if (NUMBER_SLOTS.has(slot)) return 'number';
  if (STRING_SLOTS.has(slot)) return 'string';
  if (slot === 'value') {
    switch (elementType) {
      case 'check':
      case 'button_select':
        return 'bool';
      case 'slider':
      case 'sliderf':
      case 'progress':
        return 'number';
      case 'color_picker':
        return 'color';
      case 'image':
        return 'resref';
      default:
        return 'string';
    }
  }
  return 'string';
}

/** "pc_name" / "BACK_LABEL" -> "Pc Name" / "Back Label". */
function humanize(name: string): string {
  return name
    .replace(/[_\-]+/g, ' ')
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .trim()
    .split(/\s+/)
    .map((w) => (w.length ? w[0].toUpperCase() + w.slice(1).toLowerCase() : w))
    .join(' ');
}

/** A sensible sample value for a bind, given its kind and name. */
export function defaultBindValue(info: BindInfo): any {
  switch (info.kind) {
    case 'bool':
      // Visibility/enablement default visible; selection-state defaults off.
      if (info.slot.endsWith('_selected')) return false;
      return true;
    case 'rows':
      return 3;
    case 'number':
      if (info.elementType === 'progress') return 0.65;
      if (info.elementType === 'slider' || info.elementType === 'sliderf') return 50;
      return 0;
    case 'color':
      return { r: 180, g: 185, b: 210, a: 255 };
    case 'resref':
      return 'ir_invisible';
    case 'array':
      // combo entries are [label, value]; options/tabbar elements are plain strings.
      if (info.elementType === 'combo') {
        return [['Option 1', 0], ['Option 2', 1], ['Option 3', 2]];
      }
      return ['Option 1', 'Option 2', 'Option 3'];
    case 'string':
    default: {
      const lower = info.name.toLowerCase();
      if (lower.includes('desc')) return 'A short sample description that shows how wrapped text fills this element.';
      return humanize(info.name);
    }
  }
}

/**
 * Walk the window JSON, invoking `visit` for every (non-skipped) bind. `visit`
 * may return a replacement value (used by resolveBinds); discoverBinds ignores
 * the return. The walk mutates in place when a replacement is returned, so pass
 * a clone if you need the original preserved.
 */
function walkBinds(
  node: any,
  parentType: string,
  visit: (info: BindInfo) => { replace: boolean; value?: any }
): void {
  if (node == null || typeof node !== 'object') return;

  if (Array.isArray(node)) {
    for (const item of node) walkBinds(item, parentType, visit);
    return;
  }

  const elementType: string = typeof node.type === 'string' ? node.type : parentType;

  for (const key of Object.keys(node)) {
    const val = node[key];
    if (isBindObject(val)) {
      const kind = classify(key, elementType);
      if (kind == null) continue; // skipped slot (geometry, draw-list, ...)
      const r = visit({ name: val.bind, kind, slot: key, elementType });
      if (r.replace) node[key] = r.value;
    } else if (val != null && typeof val === 'object') {
      walkBinds(val, elementType, visit);
    }
  }
}

/**
 * Collect every user-facing bind in the window, de-duplicated by name.
 *
 * A single bind name is often used in multiple slots — most commonly an array
 * bind that drives a list's `row_count` AND its row label. We pick a *primary*
 * kind for the inspector: if the bind is ever used as `row_count`, it's a 'rows'
 * control (a number that grows the list); otherwise the first slot wins.
 */
export function discoverBinds(windowJson: any): BindInfo[] {
  const byName = new Map<string, BindInfo[]>();
  walkBinds(windowJson, '', (info) => {
    const list = byName.get(info.name);
    if (list) list.push(info);
    else byName.set(info.name, [info]);
    return { replace: false };
  });

  const result: BindInfo[] = [];
  for (const [, occurrences] of byName) {
    const rows = occurrences.find((o) => o.kind === 'rows');
    result.push(rows ?? occurrences[0]);
  }
  return result.sort((a, b) => a.name.localeCompare(b.name));
}

/** Coerce an inspector/preset value to the kind expected by a specific slot. */
function coerceToSlot(value: any, slotKind: BindKind, name: string, elementType: string): any {
  switch (slotKind) {
    case 'rows': {
      if (typeof value === 'number') return Math.max(0, Math.trunc(value));
      if (Array.isArray(value)) return value.length;
      const n = parseInt(String(value), 10);
      return isNaN(n) ? 3 : n;
    }
    case 'number': {
      if (typeof value === 'number') return value;
      const n = parseFloat(String(value));
      return isNaN(n) ? 0 : n;
    }
    case 'bool':
      if (typeof value === 'boolean') return value;
      if (typeof value === 'number') return value !== 0;
      if (value === 'true') return true;
      if (value === 'false') return false;
      return defaultBindValue({ name, kind: 'bool', slot: '', elementType });
    case 'color':
      return value && typeof value === 'object' && 'r' in value
        ? value
        : defaultBindValue({ name, kind: 'color', slot: '', elementType });
    case 'array':
      return Array.isArray(value) ? value : defaultBindValue({ name, kind: 'array', slot: '', elementType });
    case 'resref':
      return typeof value === 'string' && value ? value : 'ir_invisible';
    case 'string':
    default:
      // A non-string value (e.g. a 'rows' number reused as a label) reads better
      // as the humanized bind name than as a stray "3".
      return typeof value === 'string' ? value : humanize(name);
  }
}

/**
 * Return a clone of the window JSON with every bind replaced by a concrete value.
 * Each occurrence is coerced to the kind its slot expects, so a bind used as both
 * `row_count` and a label resolves to a row count in one place and readable text
 * in the other.
 */
export function resolveBinds(windowJson: any, values: Record<string, any> = {}): any {
  if (windowJson == null) return windowJson;
  const clone = JSON.parse(JSON.stringify(windowJson));
  walkBinds(clone, '', (info) => {
    if (Object.prototype.hasOwnProperty.call(values, info.name)) {
      return { replace: true, value: coerceToSlot(values[info.name], info.kind, info.name, info.elementType) };
    }
    return { replace: true, value: defaultBindValue(info) };
  });
  return clone;
}

export interface Preset {
  name: string;
  /** Bind name -> value. Binds absent from the map fall back to placeholders. */
  values: Record<string, any>;
}

/** Built-in presets computed from the form's discovered binds. */
export function builtinPresets(binds: BindInfo[]): Preset[] {
  const typical: Record<string, any> = {};
  const empty: Record<string, any> = {};
  const overflow: Record<string, any> = {};
  const maxRows: Record<string, any> = {};

  const longText =
    'The quick brown fox jumps over the lazy dog, and then keeps on running well past the edge of this element to test wrapping and clipping.';

  for (const b of binds) {
    const def = defaultBindValue(b);
    typical[b.name] = def;
    maxRows[b.name] = def;
    overflow[b.name] = def;

    switch (b.kind) {
      case 'string':
        empty[b.name] = '';
        overflow[b.name] = longText;
        break;
      case 'bool':
        empty[b.name] = false;
        break;
      case 'rows':
        empty[b.name] = 0;
        overflow[b.name] = 12;
        maxRows[b.name] = 50;
        break;
      case 'number':
        empty[b.name] = 0;
        break;
      case 'array':
        empty[b.name] = [];
        break;
      default:
        break;
    }
  }

  return [
    { name: 'Typical', values: typical },
    { name: 'Empty', values: empty },
    { name: 'Overflow', values: overflow },
    { name: 'Max rows', values: maxRows },
  ];
}
