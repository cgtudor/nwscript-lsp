import * as vscode from "vscode";

export function getWebviewContent(
  _webview: vscode.Webview,
  _extensionUri: vscode.Uri
): string {
  return /*html*/ `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline';">
<title>NUI Preview</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    background: var(--vscode-editor-background, #1e1e1e);
    color: var(--vscode-editor-foreground, #d4d4d4);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    font-size: 13px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    min-height: 100vh;
  }
  .toolbar {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 0; margin-bottom: 8px; flex-shrink: 0;
  }
  .toolbar label { font-size: 12px; opacity: 0.8; }
  .toolbar select, .toolbar input[type="number"] {
    background: var(--vscode-input-background, #3c3c3c);
    color: var(--vscode-input-foreground, #ccc);
    border: 1px solid var(--vscode-input-border, #555);
    border-radius: 3px; padding: 2px 6px; font-size: 12px;
  }
  .toolbar input[type="number"] { width: 60px; }
  .error-bar {
    background: var(--vscode-inputValidation-errorBackground, #5a1d1d);
    border: 1px solid var(--vscode-inputValidation-errorBorder, #be1100);
    border-radius: 3px; padding: 6px 10px; margin-bottom: 8px;
    font-size: 12px; max-height: 80px; overflow-y: auto;
  }
  .error-bar ul { list-style: none; }
  .error-bar:empty { display: none; }
  .preview-area {
    flex: 1; display: flex; justify-content: center;
    align-items: flex-start; overflow: auto; padding: 8px;
  }
  .screen-frame {
    position: relative;
    border: 1px dashed #444;
    background: rgba(255,255,255,0.02);
    flex-shrink: 0;
  }
  .screen-label {
    position: absolute; bottom: -18px; right: 0;
    font-size: 10px; color: #556; font-family: monospace;
  }
  .scale-wrapper {
    transform-origin: top left;
  }
  .nui-empty {
    color: #556; font-style: italic; padding: 40px;
    display: flex; align-items: center; justify-content: center;
  }

  /* ── Absolutely positioned NUI elements ── */
  .nui-el { position: absolute; overflow: hidden; }

  .nui-window-frame {
    position: relative;
    background: #1a1a2e;
    border: 2px solid #444466;
    border-radius: 4px;
    box-shadow: 0 4px 16px rgba(0,0,0,0.5);
    overflow: hidden;
  }
  .nui-window-body {
    position: absolute; top: 28px; left: 0; right: 0; bottom: 0;
    overflow-y: auto; overflow-x: hidden;
  }
  .nui-window-body::-webkit-scrollbar { width: 10px; }
  .nui-window-body::-webkit-scrollbar-track { background: #1a1a2e; }
  .nui-window-body::-webkit-scrollbar-thumb { background: #555577; border-radius: 4px; }
  .nui-window-body::-webkit-scrollbar-thumb:hover { background: #6666aa; }
  .nui-titlebar {
    position: absolute; top: 0; left: 0; right: 0; height: 28px;
    display: flex; align-items: center; justify-content: space-between;
    padding: 0 10px;
    background: linear-gradient(180deg, #2a2a4e 0%, #1e1e3a 100%);
    border-bottom: 1px solid #444466;
    font-weight: 600; color: #c8c8e0;
    user-select: none; z-index: 1;
  }
  .nui-close {
    width: 16px; height: 16px; border: 1px solid #555577;
    background: #3a3a5e; color: #aac; font-size: 0.77em;
    text-align: center; line-height: 14px; border-radius: 2px;
  }

  /* Widget styles — font-size inherited from .nui-window-frame so it scales with UI */
  .nui-row, .nui-col, .nui-group-el { /* layout containers are invisible */ }
  .nui-group-el {
    border: 1px solid #555577;
    border-radius: 3px;
    overflow: auto;
    background: rgba(255,255,255,0.02);
  }
  .nui-group-el.no-border { border: none; background: none; }

  .nui-btn {
    display: flex; align-items: center; justify-content: center;
    background: linear-gradient(180deg, #3a3a5e 0%, #2a2a4a 100%);
    border: 1px solid #555577; border-radius: 3px;
    color: #c8c8e0;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    padding: 0 8px;
  }
  .nui-btn.pressed {
    background: linear-gradient(180deg, #1e1e3a 0%, #2a2a50 100%);
    border-color: #77b; color: #aae;
  }
  .nui-label-el {
    display: flex; padding: 0 4px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    color: #c8c8e0;
  }
  .nui-text-el {
    padding: 4px 6px; border: 1px solid #444466;
    background: rgba(0,0,0,0.15); overflow: auto;
    white-space: pre-wrap; word-wrap: break-word;
    color: #b0b0cc;
  }
  .nui-textedit-el {
    background: #0e0e1e; border: 1px solid #444466;
    border-radius: 2px; color: #c8c8e0;
    padding: 3px 6px; display: flex; align-items: center;
  }
  .nui-textedit-el .placeholder { color: #555577; }
  .nui-combo-el {
    background: #0e0e1e; border: 1px solid #444466;
    border-radius: 2px; color: #c8c8e0;
    padding: 2px 4px; display: flex; align-items: center;
  }
  .nui-check-el {
    display: flex; align-items: center; gap: 4px;
    color: #c8c8e0; padding: 0 4px;
  }
  .nui-slider-el {
    display: flex; align-items: center;
    background: #222244; border-radius: 2px;
  }
  .nui-slider-el .track {
    height: 4px; background: #555577; border-radius: 2px;
    flex: 1; margin: 0 8px; position: relative;
  }
  .nui-slider-el .thumb {
    width: 12px; height: 12px; background: #6666aa;
    border-radius: 50%; position: absolute; top: -4px; left: 30%;
  }
  .nui-progress-el {
    background: #0e0e1e; border: 1px solid #444466;
    border-radius: 2px; overflow: hidden; position: relative;
  }
  .nui-progress-fill {
    position: absolute; top: 0; left: 0; height: 100%;
    background: linear-gradient(90deg, #4444aa, #6666cc);
  }
  .nui-image-el {
    background: rgba(0,0,0,0.1); border: 1px dashed #444466;
    display: flex; align-items: center; justify-content: center;
    color: #556; font-size: 0.77em; font-family: monospace;
  }
  .nui-colorpicker-el {
    background: linear-gradient(135deg, #f00, #ff0, #0f0, #0ff, #00f, #f0f);
    border: 1px solid #555577; border-radius: 3px;
  }

  .nui-list-el {
    border: 1px solid #444466; overflow-y: auto;
    background: rgba(0,0,0,0.1);
  }
  .nui-list-row-el {
    border-bottom: 1px solid #333355;
  }
  .nui-list-row-el:nth-child(odd) { background: rgba(255,255,255,0.02); }

  .nui-bind { color: #888; font-style: italic; font-family: monospace; font-size: 0.77em; }
  .nui-spacer-el { /* invisible */ }
</style>
</head>
<body>
<div class="toolbar">
  <label>Function:</label>
  <select id="fn-select"><option value="">(evaluating...)</option></select>
  <label>Size:</label>
  <input type="number" id="w-input" value="500" min="100" readonly>
  <span>&times;</span>
  <input type="number" id="h-input" value="600" min="100" readonly>
  <label>Screen:</label>
  <select id="res-select">
    <option value="1280x720">1280x720</option>
    <option value="1366x768">1366x768</option>
    <option value="1600x900">1600x900</option>
    <option value="1920x1080" selected>1920x1080</option>
    <option value="2560x1080">2560x1080</option>
    <option value="2560x1440">2560x1440</option>
    <option value="3440x1440">3440x1440</option>
    <option value="3840x2160">3840x2160</option>
  </select>
  <label>UI Scale:</label>
  <select id="scale-select"></select>
  <label>Fit:</label>
  <select id="fit-select">
    <option value="window" selected>Window</option>
    <option value="screen">Screen</option>
  </select>
  <select id="view-select" style="display:none"></select>
</div>
<div id="errors" class="error-bar"></div>
<div class="preview-area">
  <div id="preview" class="nui-empty">Evaluating NUI code...</div>
</div>

<script>
const vscode = acquireVsCodeApi();

function tv(v) {
  // text or bind
  if (v != null && typeof v === 'object' && !Array.isArray(v) && 'bind' in v)
    return '<span class="nui-bind">[' + v.bind + ']</span>';
  if (v == null) return '';
  if (typeof v === 'object') return JSON.stringify(v);
  return String(v);
}

function renderNode(n) {
  if (!n) return '';
  const s = 'left:' + n.x + 'px;top:' + n.y + 'px;width:' + n.w + 'px;height:' + n.h + 'px;';
  const kids = (n.children || []).map(renderNode).join('');
  const p = n.props || {};

  switch (n.type) {
    case 'window': {
      const title = tv(p.title) || 'NUI Window';
      const cls = p.closable !== false ? '<div class="nui-close">X</div>' : '';
      const ch = p.contentHeight || n.h;
      const tbH = p.titleBarH || 28;
      const sc = p.scale || 1;
      return '<div class="nui-window-frame" style="width:' + n.w + 'px;height:' + n.h + 'px;font-size:' + (13 * sc) + 'px">'
        + '<div class="nui-titlebar" style="height:' + tbH + 'px;font-size:' + (13 * sc) + 'px"><span>' + title + '</span>' + cls + '</div>'
        + '<div class="nui-window-body" style="top:' + tbH + 'px"><div style="position:relative;min-height:' + ch + 'px">' + kids + '</div></div>'
        + '</div>';
    }
    case 'row':
    case 'col':
      return '<div class="nui-el" style="' + s + '">' + kids + '</div>';

    case 'group': {
      const bc = p.border === false ? ' no-border' : '';
      return '<div class="nui-el nui-group-el' + bc + '" style="' + s + '">' + kids + '</div>';
    }

    case 'spacer':
      return '<div class="nui-el nui-spacer-el" style="' + s + '"></div>';

    case 'button':
      return '<div class="nui-el nui-btn" style="' + s + '">' + tv(p.label) + '</div>';
    case 'button_select': {
      const pr = p.value && !p.value.bind ? ' pressed' : '';
      return '<div class="nui-el nui-btn' + pr + '" style="' + s + '">' + tv(p.label) + '</div>';
    }
    case 'button_image':
      return '<div class="nui-el nui-btn" style="' + s + '">' + tv(p.label || '[img]') + '</div>';

    case 'label': {
      const ha = typeof p.text_halign === 'number' ? p.text_halign : (typeof p.halign === 'number' ? p.halign : 0);
      const va = typeof p.text_valign === 'number' ? p.text_valign : (typeof p.valign === 'number' ? p.valign : 0);
      const jc = ha === 1 ? 'flex-start' : ha === 2 ? 'flex-end' : 'center';
      const ai = va === 1 ? 'flex-start' : va === 2 ? 'flex-end' : 'center';
      return '<div class="nui-el nui-label-el" style="' + s + 'justify-content:' + jc + ';align-items:' + ai + ';">' + tv(p.value) + '</div>';
    }

    case 'text':
      return '<div class="nui-el nui-text-el" style="' + s + '">' + tv(p.value) + '</div>';

    case 'textedit': {
      const ph = tv(p.label) || '';
      return '<div class="nui-el nui-textedit-el" style="' + s + '"><span class="placeholder">' + ph + '</span></div>';
    }

    case 'combo': {
      const els = p.elements;
      let first = '...';
      if (Array.isArray(els) && els.length > 0) {
        first = Array.isArray(els[0]) ? els[0][0] : String(els[0]);
      } else if (els && els.bind) {
        first = '[' + els.bind + ']';
      }
      return '<div class="nui-el nui-combo-el" style="' + s + '">' + first + ' &#9662;</div>';
    }

    case 'check':
      return '<div class="nui-el nui-check-el" style="' + s + '"><input type="checkbox" disabled> ' + tv(p.label) + '</div>';

    case 'slider': case 'sliderf':
      return '<div class="nui-el nui-slider-el" style="' + s + '"><div class="track"><div class="thumb"></div></div></div>';

    case 'progress': {
      const pv = typeof p.value === 'number' ? p.value * 100 : 50;
      return '<div class="nui-el nui-progress-el" style="' + s + '"><div class="nui-progress-fill" style="width:' + pv + '%"></div></div>';
    }

    case 'color_picker':
      return '<div class="nui-el nui-colorpicker-el" style="' + s + '"></div>';

    case 'image':
      return '<div class="nui-el nui-image-el" style="' + s + '">' + tv(p.value || '[image]') + '</div>';

    case 'list':
      return '<div class="nui-el nui-list-el" style="' + s + '">' + kids + '</div>';

    case 'list_row':
      return '<div class="nui-el nui-list-row-el" style="' + s + '">' + kids + '</div>';

    case 'options': case 'tabbar': case 'toggles':
      return '<div class="nui-el" style="' + s + '">' + kids + '</div>';

    default:
      return '<div class="nui-el" style="' + s + 'border:1px dashed #f80;color:#f80;font-size:10px;display:flex;align-items:center;justify-content:center;">[' + n.type + ']</div>';
  }
}

// ── Scale & resolution logic (matches NWN:EE engine behavior) ──
// Max UI scale = floor_to_0.1(min(screenW / 900, screenH / 700)), min 1.0
// Scale range: 1.0 to maxScale in 0.1 steps
function computeMaxScale(w, h) {
  return Math.max(1.0, Math.floor(Math.min(w / 900, h / 700) * 10) / 10);
}

function populateScaleOptions(maxScale) {
  const sel = document.getElementById('scale-select');
  const curVal = parseFloat(sel.value) || 1.0;
  sel.innerHTML = '';
  for (let s = 1.0; s <= maxScale + 0.001; s = Math.round((s + 0.1) * 10) / 10) {
    const opt = document.createElement('option');
    opt.value = String(s);
    opt.textContent = s.toFixed(1) + 'x';
    if (Math.abs(s - curVal) < 0.05 || (s === 1.0 && curVal < 1.0)) opt.selected = true;
    sel.appendChild(opt);
  }
}

let lastLayoutHtml = '';
let lastNuiJson = null;
let lastWindowW = 500, lastWindowH = 600;
let availW = 800, availH = 600;

// Track available space from body size (stable, doesn't depend on content)
new ResizeObserver(function() {
  const toolbar = document.querySelector('.toolbar');
  const errBar = document.getElementById('errors');
  const tbH = (toolbar ? toolbar.offsetHeight : 0) + (errBar ? errBar.offsetHeight : 0);
  availW = document.body.clientWidth - 40;
  availH = document.body.clientHeight - tbH - 40;
}).observe(document.body);

function renderPreview() {
  const scale = parseFloat(document.getElementById('scale-select').value) || 1.0;
  const resSel = document.getElementById('res-select');
  const [screenW, screenH] = resSel.value.split('x').map(Number);
  const fitMode = document.getElementById('fit-select').value;
  const preview = document.getElementById('preview');

  if (!lastLayoutHtml) return;

  const physW = lastWindowW * scale;
  const physH = lastWindowH * scale;

  if (fitMode === 'window') {
    // Fit the NUI window itself to the available area (close-up view)
    const fitScale = Math.min(availW / physW, availH / physH, 1);
    preview.className = '';
    preview.innerHTML =
      '<div class="scale-wrapper" style="transform:scale(' + fitScale + ');transform-origin:top center">' +
        lastLayoutHtml +
      '</div>' +
      '<div class="screen-label" style="position:relative;text-align:center;margin-top:8px">' +
        scale.toFixed(1) + 'x — ' + Math.round(physW) + 'x' + Math.round(physH) + 'px</div>';
  } else {
    // Fit the screen frame to the available area, window positioned inside
    const fitScale = Math.min(availW / screenW, availH / screenH, 1);
    const sfw = Math.round(screenW * fitScale);
    const sfh = Math.round(screenH * fitScale);
    const winX = Math.round((sfw - physW * fitScale) / 2);
    const winY = Math.round((sfh - physH * fitScale) / 2);
    const pctW = Math.round(physW / screenW * 100);
    const pctH = Math.round(physH / screenH * 100);

    preview.className = '';
    preview.innerHTML =
      '<div class="screen-frame" style="width:' + sfw + 'px;height:' + sfh + 'px">' +
        '<div class="scale-wrapper" style="transform:scale(' + fitScale + ');position:absolute;left:' + winX + 'px;top:' + winY + 'px">' +
          lastLayoutHtml +
        '</div>' +
        '<div class="screen-label">' + screenW + 'x' + screenH +
          ' @ ' + scale.toFixed(1) + 'x — ' + Math.round(physW) + 'x' + Math.round(physH) +
          'px (' + pctW + '% x ' + pctH + '%)</div>' +
      '</div>';
  }
}

document.getElementById('res-select').addEventListener('change', function() {
  const [w, h] = this.value.split('x').map(Number);
  populateScaleOptions(computeMaxScale(w, h));
  renderPreview();
});

document.getElementById('fit-select').addEventListener('change', function() {
  renderPreview();
});

document.getElementById('view-select').addEventListener('change', function() {
  const viewName = this.value;
  if (viewName) {
    const scale = parseFloat(document.getElementById('scale-select').value) || 1.0;
    vscode.postMessage({
      type: 'switchView',
      viewName: viewName,
      scale: scale,
    });
  }
});

document.getElementById('scale-select').addEventListener('change', function() {
  const scale = parseFloat(this.value) || 1.0;
  if (lastNuiJson && scale !== 1.0) {
    // Request re-solve from extension with the new scale
    vscode.postMessage({
      type: 'resolve',
      nuiJson: lastNuiJson,
      windowWidth: lastWindowW,
      windowHeight: lastWindowH,
      scale: scale,
    });
  } else {
    renderPreview();
  }
});

// Initialize scale options for default resolution
(function() {
  const resSel = document.getElementById('res-select');
  const [w, h] = resSel.value.split('x').map(Number);
  populateScaleOptions(computeMaxScale(w, h));
})();

window.addEventListener('message', event => {
  const msg = event.data;
  if (msg.type === 'update') {
    lastWindowW = msg.windowWidth || 500;
    lastWindowH = msg.windowHeight || 600;
    lastNuiJson = msg.nuiJson || null;
    document.getElementById('w-input').value = String(lastWindowW);
    document.getElementById('h-input').value = String(lastWindowH);

    // Reset scale to 1.0 on new content
    document.getElementById('scale-select').value = '1';

    // Populate views dropdown if available
    const viewSel = document.getElementById('view-select');
    if (msg.views && msg.views.length > 0) {
      viewSel.style.display = '';
      viewSel.innerHTML = '<option value="">(current view)</option>' +
        msg.views.map(function(v) { return '<option value="' + v + '">' + v.replace(/^Build|View$/g, '') + '</option>'; }).join('');
    } else {
      viewSel.style.display = 'none';
    }

    // Update function selector
    const sel = document.getElementById('fn-select');
    if (msg.functions && msg.functions.length > 0) {
      sel.innerHTML = msg.functions.map(f => '<option>' + f + '</option>').join('');
    }

    // Errors
    const errDiv = document.getElementById('errors');
    if (msg.errors && msg.errors.length > 0) {
      errDiv.innerHTML = '<ul>' + msg.errors.map(e => '<li>' + e + '</li>').join('') + '</ul>';
    } else {
      errDiv.innerHTML = '';
    }

    // Render at scale 1.0
    const preview = document.getElementById('preview');
    if (msg.layout) {
      lastLayoutHtml = renderNode(msg.layout);
      renderPreview();
    } else {
      preview.className = 'nui-empty';
      preview.innerHTML = 'No NUI window data';
    }
  }

  if (msg.type === 'scaleResult' && msg.layout) {
    if (msg.nuiJson) lastNuiJson = msg.nuiJson;  // view switch may update the JSON
    lastLayoutHtml = renderNode(msg.layout);
    renderPreview();
  }
});
</script>
</body>
</html>`;
}
