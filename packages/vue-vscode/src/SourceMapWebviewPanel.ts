import { WebviewPanel, window, ViewColumn, Uri, Disposable } from "vscode";
import type { UnifiedVirtualFileItem } from "./UnifiedVirtualFilesProvider";

/**
 * Source Map visualization WebviewPanel.
 *
 * Shows a side-by-side panel (like evanw/source-map-visualization):
 * - Left: Vue source code with colored mapping regions
 * - Right: Tabbed generated code (TSX, Script, Template, etc.) with colored regions
 *
 * Bidirectional hover highlighting links source ↔ generated segments.
 * Click-to-scroll navigates to the corresponding region in the other pane.
 */
export class SourceMapWebviewPanel implements Disposable {
  private panel: WebviewPanel | undefined;

  show(
    sourceCode: string,
    sourceUri: string,
    virtualFiles: UnifiedVirtualFileItem[],
    selectedTab: number = 0,
  ): void {
    if (this.panel) {
      this.panel.reveal(ViewColumn.Beside);
    } else {
      this.panel = window.createWebviewPanel(
        "verterSourceMap",
        "Source Map Visualization",
        { viewColumn: ViewColumn.Beside, preserveFocus: true },
        {
          enableScripts: true,
          retainContextWhenHidden: true,
        },
      );

      this.panel.onDidDispose(() => {
        this.panel = undefined;
      });
    }

    const filesWithMaps = virtualFiles.filter((vf) => vf.sourceMap);
    // Clamp selectedTab
    const clampedTab = Math.max(0, Math.min(selectedTab, filesWithMaps.length - 1));

    const data = {
      sourceCode,
      sourceUri,
      selectedTab: clampedTab,
      virtualFiles: filesWithMaps.map((vf) => ({
        kind: vf.kind,
        lang: vf.lang,
        code: vf.code,
        sourceMap: vf.sourceMap,
        isTsx: vf.isTsx,
      })),
    };

    this.panel.webview.html = this.getWebviewHtml(data);
  }

  private getWebviewHtml(data: {
    sourceCode: string;
    sourceUri: string;
    selectedTab: number;
    virtualFiles: Array<{
      kind: string;
      lang: string;
      code: string;
      sourceMap: string | null;
      isTsx: boolean;
    }>;
  }): string {
    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Source Map Visualization</title>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body {
      font-family: var(--vscode-editor-font-family, 'Consolas, monospace');
      font-size: var(--vscode-editor-font-size, 13px);
      background: var(--vscode-editor-background);
      color: var(--vscode-editor-foreground);
      height: 100vh;
      display: flex;
      flex-direction: column;
      overflow: hidden;
    }
    .toolbar {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 6px 12px;
      background: var(--vscode-sideBar-background);
      border-bottom: 1px solid var(--vscode-panel-border);
      flex-shrink: 0;
    }
    .toolbar button {
      background: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
      border: none;
      padding: 4px 10px;
      cursor: pointer;
      border-radius: 2px;
      font-size: 12px;
    }
    .toolbar button:hover {
      background: var(--vscode-button-hoverBackground);
    }
    .toolbar button.active {
      background: var(--vscode-button-secondaryBackground);
      color: var(--vscode-button-secondaryForeground);
    }
    .toolbar .label {
      font-size: 11px;
      color: var(--vscode-descriptionForeground);
    }
    .toolbar .spacer { flex: 1; }
    .toolbar .stats {
      font-size: 11px;
      color: var(--vscode-descriptionForeground);
    }
    .copy-btn {
      background: transparent;
      color: var(--vscode-descriptionForeground);
      border: 1px solid var(--vscode-panel-border);
      padding: 2px 8px;
      cursor: pointer;
      border-radius: 2px;
      font-size: 11px;
    }
    .copy-btn:hover {
      background: var(--vscode-list-hoverBackground);
      color: var(--vscode-foreground);
    }
    .copy-btn.copied {
      color: var(--vscode-testing-iconPassed, #73c991);
      border-color: var(--vscode-testing-iconPassed, #73c991);
    }

    /* Side-by-side layout */
    .panels {
      display: flex;
      flex-direction: row;
      flex: 1;
      overflow: hidden;
    }
    .pane {
      flex: 1;
      display: flex;
      flex-direction: column;
      overflow: hidden;
      min-width: 0;
    }
    .pane-divider {
      width: 1px;
      background: var(--vscode-panel-border);
      flex-shrink: 0;
    }
    .pane-header {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 4px 12px;
      background: var(--vscode-sideBar-background);
      border-bottom: 1px solid var(--vscode-panel-border);
      flex-shrink: 0;
    }
    .pane-title {
      font-size: 11px;
      font-weight: 600;
      color: var(--vscode-descriptionForeground);
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }
    .pane-content {
      flex: 1;
      overflow: auto;
      padding: 8px 12px;
    }

    /* Tabs in the generated pane header */
    .tabs {
      display: flex;
      gap: 2px;
      margin-left: auto;
    }
    .tab {
      padding: 2px 8px;
      cursor: pointer;
      font-size: 11px;
      border-radius: 2px;
      color: var(--vscode-foreground);
    }
    .tab:hover {
      background: var(--vscode-list-hoverBackground);
    }
    .tab.active {
      background: var(--vscode-list-activeSelectionBackground);
      color: var(--vscode-list-activeSelectionForeground);
    }

    pre {
      white-space: pre;
      // line-height: 1;
      tab-size: 2;
    }
    .line {
      display: block;
      // min-height: 1.2em;
    }
    .line-number {
      display: inline-block;
      width: 3.5em;
      text-align: right;
      margin-right: 1em;
      color: var(--vscode-editorLineNumber-foreground);
      user-select: none;
    }
    .mapped-region {
      border-radius: 2px;
      cursor: pointer;
      position: relative;
    }
    .mapped-region:hover {
      outline: 1px solid var(--vscode-focusBorder);
    }
    .highlight {
      outline: 2px solid var(--vscode-focusBorder) !important;
      filter: brightness(1.3);
    }
    .no-data {
      padding: 20px;
      text-align: center;
      color: var(--vscode-descriptionForeground);
    }

    /* Tooltip for mapping info */
    .seg-tooltip {
      position: fixed;
      background: var(--vscode-editorHoverWidget-background, #2d2d30);
      color: var(--vscode-editorHoverWidget-foreground, #ccc);
      border: 1px solid var(--vscode-editorHoverWidget-border, #454545);
      padding: 4px 8px;
      font-size: 11px;
      border-radius: 3px;
      pointer-events: none;
      z-index: 1000;
      white-space: nowrap;
      display: none;
    }

    /* 32-color segment palette */
    ${generateSegmentColors(32)}
  </style>
</head>
<body>
  <div class="toolbar">
    <span class="label">Mode:</span>
    <button id="btn-cursor" class="active" onclick="setMode('cursor')">Cursor</button>
    <button id="btn-full" onclick="setMode('full')">Full Visualization</button>
    <span class="spacer"></span>
    <span class="stats" id="stats"></span>
  </div>
  <div class="panels">
    <div class="pane" id="source-pane">
      <div class="pane-header">
        <span class="pane-title">Vue Source</span>
        <button class="copy-btn" onclick="copyPane('source')" title="Copy source code">Copy</button>
      </div>
      <div class="pane-content" id="source-scroll">
        <pre id="source-code"></pre>
      </div>
    </div>
    <div class="pane-divider"></div>
    <div class="pane" id="generated-pane">
      <div class="pane-header">
        <span class="pane-title">Generated</span>
        <button class="copy-btn" onclick="copyPane('generated')" title="Copy generated code">Copy</button>
        <button class="copy-btn" onclick="copyPane('sourcemap')" title="Copy source map JSON">Copy Map</button>
        <div class="tabs" id="tabs-container"></div>
      </div>
      <div class="pane-content" id="generated-scroll">
        <pre id="generated-code"></pre>
      </div>
    </div>
  </div>
  <div class="seg-tooltip" id="tooltip"></div>

  <script>
    const DATA = JSON.parse(decodeURIComponent("${encodeURIComponent(JSON.stringify(data))}"));

    // VLQ decoder
    const VLQ_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const VLQ_LOOKUP = new Map();
    for (let i = 0; i < VLQ_CHARS.length; i++) VLQ_LOOKUP.set(VLQ_CHARS[i], i);

    function decodeVLQValue(str, index) {
      let result = 0, shift = 0, i = index;
      while (true) {
        const digit = VLQ_LOOKUP.get(str[i]);
        if (digit === undefined) throw new Error('Invalid VLQ');
        i++;
        result += (digit & 31) << shift;
        shift += 5;
        if ((digit & 32) === 0) break;
      }
      const negate = (result & 1) !== 0;
      result >>= 1;
      return [negate ? -result : result, i];
    }

    function parseMappings(mappings) {
      if (!mappings) return [];
      const lines = [];
      const groups = mappings.split(';');
      let srcLine = 0, srcCol = 0, sourceIdx = 0, nameIdx = 0;
      for (const group of groups) {
        const segments = [];
        let genCol = 0;
        if (group.length > 0) {
          const parts = group.split(',');
          for (const part of parts) {
            if (!part.length) continue;
            let i = 0;
            const values = [];
            while (i < part.length) {
              const [val, next] = decodeVLQValue(part, i);
              values.push(val);
              i = next;
            }
            if (values.length >= 4) {
              genCol += values[0];
              sourceIdx += values[1];
              srcLine += values[2];
              srcCol += values[3];
              if (values.length >= 5) {
                nameIdx += values[4];
                segments.push([genCol, sourceIdx, srcLine, srcCol, nameIdx]);
              } else {
                segments.push([genCol, sourceIdx, srcLine, srcCol]);
              }
            } else if (values.length === 1) {
              genCol += values[0];
            }
          }
        }
        lines.push(segments);
      }
      return lines;
    }

    function parseSourceMap(json) {
      try {
        const map = JSON.parse(json);
        return { ...map, parsed: parseMappings(map.mappings) };
      } catch { return null; }
    }

    // State
    let currentMode = 'full';
    let currentTab = DATA.selectedTab || 0;
    let parsedMaps = [];

    // Parse all source maps
    for (const vf of DATA.virtualFiles) {
      parsedMaps.push(vf.sourceMap ? parseSourceMap(vf.sourceMap) : null);
    }

    function setMode(mode) {
      currentMode = mode;
      document.getElementById('btn-cursor').classList.toggle('active', mode === 'cursor');
      document.getElementById('btn-full').classList.toggle('active', mode === 'full');
      render();
    }

    function setTab(idx) {
      currentTab = idx;
      document.querySelectorAll('.tab').forEach((t, i) => {
        t.classList.toggle('active', i === idx);
      });
      render();
    }

    function escapeHtml(text) {
      return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }

    function buildSegmentMap(parsedSegments) {
      const segments = [];
      for (let genLine = 0; genLine < parsedSegments.length; genLine++) {
        for (const seg of parsedSegments[genLine] || []) {
          segments.push({
            idx: segments.length,
            genLine,
            genCol: seg[0],
            srcLine: seg[2],
            srcCol: seg[3],
            colorIdx: segments.length % 32,
          });
        }
      }
      return segments;
    }

    function renderFullVisualization(sourceEl, genEl) {
      const map = parsedMaps[currentTab];
      if (!map) {
        sourceEl.innerHTML = renderPlainLines(DATA.sourceCode);
        genEl.innerHTML = '<div class="no-data">No source map for this file</div>';
        document.getElementById('stats').textContent = '';
        return;
      }

      const segments = buildSegmentMap(map.parsed);
      document.getElementById('stats').textContent = segments.length + ' mappings';

      // Render source with colored regions
      const srcLines = DATA.sourceCode.split('\\n');
      sourceEl.innerHTML = renderColoredLines(srcLines, segments, 'src');

      // Render generated with colored regions
      const genCode = DATA.virtualFiles[currentTab].code;
      const genLines = genCode.split('\\n');
      genEl.innerHTML = renderColoredLines(genLines, segments, 'gen');
    }

    function renderColoredLines(lines, segments, side) {
      // Group segments by line, keeping original index
      const lineSegments = new Map();
      for (const seg of segments) {
        const line = side === 'src' ? seg.srcLine : seg.genLine;
        const col = side === 'src' ? seg.srcCol : seg.genCol;
        if (!lineSegments.has(line)) lineSegments.set(line, []);
        lineSegments.get(line).push({ ...seg, col, segIdx: seg.idx });
      }

      return lines.map((line, lineIdx) => {
        const lineNum = '<span class="line-number">' + (lineIdx + 1) + '</span>';
        const segs = lineSegments.get(lineIdx);
        if (!segs || segs.length === 0) {
          return '<span class="line">' + lineNum + escapeHtml(line) + '</span>';
        }

        // Sort segments by column
        segs.sort((a, b) => a.col - b.col);

        let html = lineNum;
        let lastCol = 0;
        for (let i = 0; i < segs.length; i++) {
          const seg = segs[i];
          const nextCol = i + 1 < segs.length ? segs[i + 1].col : line.length;

          // Text before this segment
          if (seg.col > lastCol) {
            html += escapeHtml(line.slice(lastCol, seg.col));
          }

          // The segment region — use segIdx for data-seg (stable reference)
          const regionEnd = Math.min(nextCol, line.length);
          const regionText = line.slice(seg.col, regionEnd);
          if (regionText.length > 0) {
            html += '<span class="mapped-region seg-color-' + seg.colorIdx +
              '" data-seg="' + seg.segIdx +
              '" data-src-line="' + seg.srcLine +
              '" data-src-col="' + seg.srcCol +
              '" data-gen-line="' + seg.genLine +
              '" data-gen-col="' + seg.genCol +
              '">' + escapeHtml(regionText) + '</span>';
          }
          lastCol = regionEnd;
        }

        // Remaining text
        if (lastCol < line.length) {
          html += escapeHtml(line.slice(lastCol));
        }

        return '<span class="line">' + html + '</span>';
      }).join('\\n');
    }

    function renderPlainLines(code) {
      return code.split('\\n').map((line, i) =>
        '<span class="line"><span class="line-number">' + (i + 1) + '</span>' + escapeHtml(line) + '</span>'
      ).join('\\n');
    }

    function renderCursorMode(sourceEl, genEl) {
      sourceEl.innerHTML = renderPlainLines(DATA.sourceCode);

      const vf = DATA.virtualFiles[currentTab];
      if (!vf) {
        genEl.innerHTML = '<div class="no-data">No virtual file selected</div>';
        return;
      }

      genEl.innerHTML = renderPlainLines(vf.code);
      document.getElementById('stats').textContent = '';
    }

    function render() {
      const sourceEl = document.getElementById('source-code');
      const genEl = document.getElementById('generated-code');

      if (currentMode === 'full') {
        renderFullVisualization(sourceEl, genEl);
      } else {
        renderCursorMode(sourceEl, genEl);
      }
    }

    // Build tabs
    function buildTabs() {
      const container = document.getElementById('tabs-container');
      container.innerHTML = '';
      DATA.virtualFiles.forEach((vf, i) => {
        const tab = document.createElement('div');
        tab.className = 'tab' + (i === currentTab ? ' active' : '');
        tab.textContent = vf.isTsx ? 'TSX' : vf.kind;
        tab.onclick = () => setTab(i);
        container.appendChild(tab);
      });
    }

    // Tooltip
    const tooltip = document.getElementById('tooltip');

    // Bidirectional hover highlighting
    document.addEventListener('mouseover', (e) => {
      if (currentMode !== 'full') return;
      const target = e.target;
      if (!target.classList || !target.classList.contains('mapped-region')) return;
      const segIdx = target.dataset.seg;

      // Highlight all elements with the same segment index
      document.querySelectorAll('.mapped-region[data-seg="' + segIdx + '"]').forEach(el => {
        el.classList.add('highlight');
      });

      // Show tooltip with mapping info
      const srcLine = parseInt(target.dataset.srcLine, 10);
      const srcCol = parseInt(target.dataset.srcCol, 10);
      const genLine = parseInt(target.dataset.genLine, 10);
      const genCol = parseInt(target.dataset.genCol, 10);
      tooltip.textContent = 'Source ' + (srcLine + 1) + ':' + srcCol + ' \\u2194 Generated ' + (genLine + 1) + ':' + genCol;
      tooltip.style.display = 'block';
    });

    document.addEventListener('mousemove', (e) => {
      if (tooltip.style.display === 'block') {
        tooltip.style.left = (e.clientX + 12) + 'px';
        tooltip.style.top = (e.clientY - 28) + 'px';
      }
    });

    document.addEventListener('mouseout', (e) => {
      if (currentMode !== 'full') return;
      const target = e.target;
      if (!target.classList || !target.classList.contains('mapped-region')) return;
      document.querySelectorAll('.highlight').forEach(el => {
        el.classList.remove('highlight');
      });
      tooltip.style.display = 'none';
    });

    // Click-to-scroll: clicking a segment scrolls the other pane to the counterpart
    document.addEventListener('click', (e) => {
      if (currentMode !== 'full') return;
      const target = e.target;
      if (!target.classList || !target.classList.contains('mapped-region')) return;
      const segIdx = target.dataset.seg;

      // Determine which pane we're in and find counterpart in the other pane
      const inSource = target.closest('#source-scroll');
      const otherScroll = inSource
        ? document.getElementById('generated-scroll')
        : document.getElementById('source-scroll');

      const counterpart = otherScroll.querySelector('.mapped-region[data-seg="' + segIdx + '"]');
      if (counterpart) {
        counterpart.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }
    });

    function copyPane(target) {
      let text;
      if (target === 'source') {
        text = DATA.sourceCode;
      } else if (target === 'generated') {
        const vf = DATA.virtualFiles[currentTab];
        text = vf ? vf.code : '';
      } else if (target === 'sourcemap') {
        const vf = DATA.virtualFiles[currentTab];
        text = vf && vf.sourceMap ? vf.sourceMap : '';
      }
      if (!text) return;
      navigator.clipboard.writeText(text).then(() => {
        // Flash the button to indicate success
        const btn = event.target;
        btn.classList.add('copied');
        const orig = btn.textContent;
        btn.textContent = 'Copied!';
        setTimeout(() => {
          btn.classList.remove('copied');
          btn.textContent = orig;
        }, 1500);
      });
    }

    buildTabs();
    // Default to full visualization mode
    setMode('full');
  </script>
</body>
</html>`;
  }

  dispose(): void {
    this.panel?.dispose();
  }
}

function generateSegmentColors(count: number): string {
  const hueStep = 360 / count;
  return Array.from(
    { length: count },
    (_, i) => `.seg-color-${i} { background: hsla(${Math.round(i * hueStep)}, 70%, 50%, 0.25); }`,
  ).join("\n    ");
}
