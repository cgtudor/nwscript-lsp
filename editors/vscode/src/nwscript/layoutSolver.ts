/**
 * NUI Layout Solver
 *
 * Approximates NWN:EE's kiwi/Cassowary-based NUI layout engine.
 * Outputs a tree of elements with positions RELATIVE to their parent.
 *
 * The actual engine (see gist.github.com/niv/9bf60ca649fdf6709c17886a1a467755):
 *   - Uses kiwi (github.com/nucleic/kiwi) Cassowary constraint solver.
 *   - Span::BuildConstraintDependants chains children end-to-end with ZERO gaps.
 *     Spacing comes only from per-element margins (NuiMargin).
 *   - Fixed sizes (NuiWidth/NuiHeight) are STRONG constraints.
 *   - Spaced-equally (flexible) children use WEAK equal-size constraints.
 *   - LayoutGroup starts at (0,0); gw/gh are edit variables (MEDIUM) set to
 *     the Nuklear content pane dimensions (after title bar + window padding).
 *
 * This solver approximates the layout without a full constraint solver:
 *   - GAP approximates visual spacing from Nuklear's widget rendering padding.
 *   - BODY_PADDING approximates Nuklear's window content pane insets.
 */

const GAP = 8;           // unscaled default margin spacing (margins don't scale in NWN:EE)
const TITLE_BAR_H = 28;
const BODY_PAD_X = 10;   // unscaled Nuklear window content pane horizontal padding per side
const BODY_PAD_Y = 16;   // unscaled Nuklear window content pane vertical padding per side

export interface SolvedNode {
  type: string;
  x: number;  // relative to parent
  y: number;  // relative to parent
  w: number;
  h: number;
  props: Record<string, any>;
  children: SolvedNode[];
}

/**
 * Solve NUI layout. Scale simulates NWN:EE UI scaling behavior:
 * - Window dimensions and Nuklear chrome (title, padding, gaps) scale with UI
 * - NUI element sizes (NuiWidth/NuiHeight) stay in logical pixels (unscaled)
 * - At higher scales, more physical space is available for the same logical content
 */
export function solveLayout(json: any, windowW: number, windowH: number, scale: number = 1.0): SolvedNode | null {
  if (!json || !json.root) return null;

  // Window geometry and title bar scale with UI.
  // GAP and body padding do NOT scale — they come from Nuklear default margins
  // which are fixed pixel values (confirmed: margins don't scale in NWN:EE).
  const physW = windowW * scale;
  const physH = windowH * scale;
  const gap = GAP;        // unscaled (default margin spacing)
  const padX = BODY_PAD_X; // unscaled (Nuklear window padding)
  const padY = BODY_PAD_Y; // unscaled (Nuklear window padding)
  const titleH = TITLE_BAR_H * scale;

  const bodyW = physW - 2 * padX;
  const bodyH = physH - titleH - 2 * padY;

  const rootChildren = solveElementScaled(json.root, bodyW, bodyH, scale, gap);

  // Expand root to fit actual content so overflow:hidden on .nui-el doesn't clip.
  let contentH = bodyH;
  if (rootChildren) {
    const childBottom = maxChildBottom(rootChildren);
    if (childBottom > rootChildren.h) {
      rootChildren.h = childBottom;
    }
    contentH = padY + rootChildren.h + padY;
  }

  return {
    type: "window",
    x: 0, y: 0, w: physW, h: physH,
    props: { title: json.title, closable: json.closable, contentHeight: contentH, titleBarH: titleH, scale },
    children: rootChildren ? [{
      ...rootChildren,
      x: padX, y: padY,
    }] : [],
  };
}

/** Scale=1.0 convenience wrapper */
function solveElement(el: any, availW: number, availH: number): SolvedNode | null {
  return solveElementScaled(el, availW, availH, 1.0, GAP);
}

/**
 * Solve layout for a single element within the given available space.
 * NuiWidth/NuiHeight scale with UI scale. NuiMargin does NOT scale (known engine bug).
 * Container space (availW/availH), gaps, and padding are in physical pixels (already scaled).
 */
function solveElementScaled(el: any, availW: number, availH: number, scale: number = 1.0, gap: number = GAP): SolvedNode | null {
  if (!el || typeof el !== "object") return null;

  const type: string = el.type ?? "";
  const margin = num(el.margin) ?? 0;        // margins DON'T scale (engine bug)
  const padding = (num(el.padding) ?? 0) * scale;  // padding scales

  // Available space after margin (margin is unscaled)
  const innerW = Math.max(0, availW - 2 * margin);
  const innerH = Math.max(0, availH - 2 * margin);

  // Element dimensions: explicit sizes scale with UI, flexible fills available
  const rawW = num(el.width);
  const rawH = num(el.height);
  let ew = rawW != null ? rawW * scale : innerW;
  let eh = rawH != null ? rawH * scale : innerH;

  // Aspect ratio
  const aspect = num(el.aspect);
  if (aspect != null && aspect > 0) {
    if (rawW != null && rawH == null) eh = ew / aspect;
    else if (rawH != null && rawW == null) ew = eh * aspect;
  }

  // Clamp to available
  ew = Math.min(ew, innerW);
  eh = Math.min(eh, innerH);

  // Content area for children (after padding)
  const cw = Math.max(0, ew - 2 * padding);
  const ch = Math.max(0, eh - 2 * padding);

  const node: SolvedNode = {
    type,
    x: margin,  // relative to parent's slot
    y: margin,
    w: ew,
    h: eh,
    props: extractProps(el),
    children: [],
  };

  switch (type) {
    case "row":
      node.children = solveRow(el.children ?? [], padding, padding, cw, ch, scale, gap);
      break;
    case "col":
      node.children = solveCol(el.children ?? [], padding, padding, cw, ch, scale, gap);
      break;
    case "group":
      node.children = solveGroup(el, padding, padding, cw, ch, scale, gap);
      break;
    case "list":
      node.children = solveList(el, padding, padding, cw, ch, scale, gap);
      break;
  }

  return node;
}

function solveRow(children: any[], startX: number, startY: number, totalW: number, totalH: number, scale: number = 1, gap: number = GAP): SolvedNode[] {
  if (!Array.isArray(children) || children.length === 0) return [];

  const gaps = gap * Math.max(0, children.length - 1);
  const distribW = totalW - gaps;

  // Measure fixed children (widths scale, margins don't)
  let fixedSum = 0;
  let flexCount = 0;
  for (const child of children) {
    if (!child) continue;
    const cw = num(child.width);
    const cm = num(child.margin) ?? 0;
    if (cw != null) {
      fixedSum += cw * scale + 2 * cm;
    } else {
      flexCount++;
    }
  }

  const flexW = flexCount > 0 ? Math.max(0, (distribW - fixedSum) / flexCount) : 0;
  const results: SolvedNode[] = [];
  let curX = startX;

  for (const child of children) {
    if (!child) continue;
    const cm = num(child.margin) ?? 0;
    const explicitW = num(child.width);
    const slotW = explicitW != null ? explicitW * scale + 2 * cm : flexW;

    const solved = solveElementScaled(child, slotW, totalH, scale, gap);
    if (solved) {
      solved.x += curX;
      solved.y += startY;
      results.push(solved);
    }
    curX += slotW + gap;
  }
  return results;
}

function solveCol(children: any[], startX: number, startY: number, totalW: number, totalH: number, scale: number = 1, gap: number = GAP): SolvedNode[] {
  if (!Array.isArray(children) || children.length === 0) return [];

  const gaps = gap * Math.max(0, children.length - 1);
  const distribH = totalH - gaps;

  // Measure fixed children (heights scale, margins don't)
  let fixedSum = 0;
  let flexCount = 0;

  for (const child of children) {
    if (!child) continue;
    const ch = num(child.height);
    const cm = num(child.margin) ?? 0;
    if (ch != null) {
      fixedSum += ch * scale + 2 * cm;
    } else if (child.type === "row") {
      const est = estimateRowHeight(child, scale);
      if (est > 0) {
        fixedSum += est + 2 * cm;
      } else {
        flexCount++;
      }
    } else {
      flexCount++;
    }
  }

  const flexH = flexCount > 0 ? Math.max(0, (distribH - fixedSum) / flexCount) : 0;
  const results: SolvedNode[] = [];
  let curY = startY;

  for (const child of children) {
    if (!child) continue;
    const cm = num(child.margin) ?? 0;
    const explicitH = num(child.height);
    let slotH: number;
    if (explicitH != null) {
      slotH = explicitH * scale + 2 * cm;
    } else if (child.type === "row") {
      const est = estimateRowHeight(child, scale);
      slotH = est > 0 ? est + 2 * cm : flexH;
    } else {
      slotH = flexH;
    }

    const solved = solveElementScaled(child, totalW, slotH, scale, gap);
    if (solved) {
      solved.x += startX;
      solved.y += curY;
      results.push(solved);
    }
    curY += slotH + gap;
  }
  return results;
}

function solveGroup(el: any, startX: number, startY: number, cw: number, ch: number, scale: number = 1, gap: number = GAP): SolvedNode[] {
  const children = el.children ?? [];
  if (!Array.isArray(children) || children.length === 0) return [];

  // Group typically contains one child (a col)
  const results: SolvedNode[] = [];
  for (const child of children) {
    const solved = solveElementScaled(child, cw, ch, scale, gap);
    if (solved) {
      solved.x += startX;
      solved.y += startY;
      results.push(solved);
    }
  }
  return results;
}

function solveList(el: any, startX: number, startY: number, cw: number, _ch: number, scale: number = 1, gap: number = GAP): SolvedNode[] {
  const template = el.row_template ?? [];
  const rowHeight = (el.row_height ?? 25) * scale;  // row_height scales with UI
  const rowCount = num(el.row_count) ?? 12;
  const count = Math.min(typeof rowCount === "number" ? rowCount : 12, 50);

  const results: SolvedNode[] = [];
  let curY = startY;

  for (let r = 0; r < count; r++) {
    const rowChildren = solveListRow(template, 0, 0, cw, rowHeight, scale);
    results.push({
      type: "list_row",
      x: startX, y: curY, w: cw, h: rowHeight,
      props: { rowIndex: r },
      children: rowChildren,
    });
    curY += rowHeight;
  }
  return results;
}

function solveListRow(template: any[], startX: number, startY: number, w: number, h: number, scale: number = 1): SolvedNode[] {
  if (!Array.isArray(template) || template.length === 0) return [];

  // Fixed cells use their exact width; variable cells share remaining space
  // Variable cells with a preferred width distribute proportionally by weight
  let fixedSum = 0;
  let varWeightSum = 0;
  let varCount = 0;
  for (const cell of template) {
    const cellW = cell[1] ?? cell.width ?? 0;
    const isVar = cell[2] ?? cell.is_variable ?? !cellW;
    if (!isVar && cellW > 0) {
      fixedSum += cellW;
    } else {
      varCount++;
      varWeightSum += cellW > 0 ? cellW : 1;  // no-width variable cells get weight 1
    }
  }

  const remaining = Math.max(0, w - fixedSum);

  const results: SolvedNode[] = [];
  let curX = startX;
  for (const cell of template) {
    const cellEl = cell[0] ?? cell.element ?? cell;
    const cellW = cell[1] ?? cell.width ?? 0;
    const isVar = cell[2] ?? cell.is_variable ?? !cellW;

    let cw: number;
    if (!isVar && cellW > 0) {
      cw = cellW;
    } else {
      const weight = cellW > 0 ? cellW : 1;
      cw = varWeightSum > 0 ? remaining * (weight / varWeightSum) : 0;
    }

    // For variable cells, the element fills the cell width (override explicit width, in physical px)
    // Divide by scale since solveElementScaled will multiply width back by scale
    const elForSolve = isVar ? { ...cellEl, width: cw / scale } : cellEl;
    const solved = solveElementScaled(elForSolve, cw, h, scale);
    if (solved) {
      solved.x += curX;
      solved.y += startY;
      results.push(solved);
    }
    curX += cw;
  }
  return results;
}

/** Find the furthest bottom edge among a node's children (recursive into layout containers). */
function maxChildBottom(node: SolvedNode): number {
  let max = node.h;
  for (const child of node.children) {
    const childMax = child.y + maxChildBottom(child);
    if (childMax > max) max = childMax;
  }
  return max;
}

function estimateRowHeight(row: any, scale: number = 1): number {
  const children = row?.children;
  if (!Array.isArray(children)) return 0;
  let maxH = 0;
  for (const child of children) {
    const h = num(child?.height);
    const m = num(child?.margin) ?? 0;  // margins don't scale
    if (h != null && h * scale + 2 * m > maxH) maxH = h * scale + 2 * m;
  }
  return maxH;
}

function num(v: any): number | null {
  if (v == null) return null;
  if (typeof v === "number") return v;
  if (typeof v === "object" && "bind" in v) return null;
  return null;
}

function extractProps(el: any): Record<string, any> {
  const props: Record<string, any> = {};
  const keys = [
    "label", "value", "id", "border", "scrollbars",
    "text_halign", "halign", "text_valign", "valign",
    "visible", "enabled", "tooltip", "foreground_color",
    "image_aspect", "image_halign", "image_valign",
    "min", "max", "step", "multiline", "elements",
    "row_template", "row_count", "row_height",
  ];
  for (const key of keys) {
    if (el[key] !== undefined) props[key] = el[key];
  }
  return props;
}
