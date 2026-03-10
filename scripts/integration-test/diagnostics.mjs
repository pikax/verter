import path from 'node:path';

export const DIAGNOSTICS_SCHEMA = 'verter.typecheck-diagnostics.v1';
export const DIAGNOSTIC_DIFF_SCHEMA = 'verter.typecheck-diagnostics-diff.v1';
export const REVIEW_QUEUE_SCHEMA = 'verter.review-queue.v1';

const ANSI_RE = /\u001B\[[0-9;]*m/gu;

function normalizePath(value, cwd) {
  if (!value) return value;
  let normalized = value.trim().replace(/\\/g, '/');
  if (path.isAbsolute(value)) {
    normalized = path.relative(cwd, value).replace(/\\/g, '/');
  }
  return normalized;
}

function makeDiagnosticKey(entry) {
  return [entry.file, entry.line, entry.column, entry.code, entry.message].join('|');
}

function classifyDiagnostic(entry) {
  const file = entry.file || '';
  const message = entry.message || '';
  if (/__VLS|__verter|virtual|generated/u.test(file)) return 'verter_only_suspect';
  if (/node_modules/u.test(file)) return 'env_or_config';
  if (/Cannot find module|Cannot find type definition file|Cannot find name/u.test(message)) {
    return 'env_or_config';
  }
  if (/Duplicate identifier|Subsequent property declarations/u.test(message)) {
    return 'verter_only_suspect';
  }
  return 'verter_only_likely_legit';
}

export function stripAnsi(value) {
  return value.replace(ANSI_RE, '');
}

export function parseTypeScriptDiagnostics(output, { tool, pass, cwd }) {
  const clean = stripAnsi(output);
  const lines = clean.split(/\r?\n/u);
  const diagnostics = [];

  for (const line of lines) {
    const colonMatch = line.match(/^(.*):(\d+):(\d+)\s+-\s+error\s+TS(\d+):\s+(.+)$/u);
    const parenMatch = line.match(/^(.*)\((\d+),(\d+)\):\s+error\s+TS(\d+):\s+(.+)$/u);
    const match = colonMatch || parenMatch;
    if (!match) continue;

    const [, rawFile, rawLine, rawColumn, rawCode, rawMessage] = match;
    diagnostics.push({
      tool,
      pass,
      file: normalizePath(rawFile, cwd),
      line: Number(rawLine),
      column: Number(rawColumn),
      code: `TS${rawCode}`,
      message: rawMessage.trim(),
      rawLine: line.trim(),
    });
  }

  return diagnostics;
}

export function normalizeTypeCheckArtifacts(typeCheck, cwd) {
  if (!typeCheck) {
    return {
      schema: DIAGNOSTICS_SCHEMA,
      tsconfig: null,
      runs: [],
      diagnostics: [],
    };
  }

  const runs = [];
  const diagnostics = [];

  for (const toolKey of ['vueTsc', 'verterTsc']) {
    const toolName = toolKey === 'vueTsc' ? 'vue-tsc' : 'verter-tsc';
    for (const pass of ['cold', 'warm']) {
      const result = typeCheck[toolKey]?.[pass];
      if (!result) continue;

      const stdout = String(result.stdout ?? '');
      const stderr = String(result.stderr ?? '');
      const combined = [stdout, stderr].filter(Boolean).join('\n');
      const parsed = parseTypeScriptDiagnostics(combined, {
        tool: toolName,
        pass,
        cwd,
      });

      runs.push({
        tool: toolName,
        pass,
        exitCode: result.exitCode,
        timedOut: Boolean(result.timedOut),
        errorCount: result.errorCount,
        ms: result.ms,
        stdout,
        stderr,
      });

      diagnostics.push(...parsed);

      if (parsed.length === 0 && (result.exitCode !== 0 || result.timedOut)) {
        diagnostics.push({
          tool: toolName,
          pass,
          file: null,
          line: null,
          column: null,
          code: result.timedOut ? 'TIMEOUT' : 'PROCESS',
          message: result.timedOut
            ? 'Type check timed out'
            : `Exited with code ${result.exitCode} without parseable diagnostics`,
          rawLine: stripAnsi(combined).split(/\r?\n/u).find(Boolean) ?? '',
          synthetic: true,
        });
      }
    }
  }

  return {
    schema: DIAGNOSTICS_SCHEMA,
    tsconfig: typeCheck.tsconfig ?? null,
    runs,
    diagnostics,
  };
}

export function buildDiagnosticDiff(normalized) {
  const warmDiagnostics = normalized.diagnostics.filter((entry) => entry.pass === 'warm');
  const vueDiagnostics = warmDiagnostics.filter((entry) => entry.tool === 'vue-tsc');
  const verterDiagnostics = warmDiagnostics.filter((entry) => entry.tool === 'verter-tsc');

  const vueByKey = new Map(vueDiagnostics.map((entry) => [makeDiagnosticKey(entry), entry]));
  const verterByKey = new Map(verterDiagnostics.map((entry) => [makeDiagnosticKey(entry), entry]));

  const items = [];

  for (const [key, entry] of vueByKey) {
    if (verterByKey.has(key)) {
      items.push({ classification: 'shared', diagnostic: entry });
      verterByKey.delete(key);
    } else {
      items.push({ classification: 'vue_only', diagnostic: entry });
    }
  }

  for (const entry of verterByKey.values()) {
    const classification = entry.code === 'PROCESS'
      ? 'tool_crash'
      : classifyDiagnostic(entry);
    items.push({ classification, diagnostic: entry });
  }

  return {
    schema: DIAGNOSTIC_DIFF_SCHEMA,
    summary: items.reduce((acc, item) => {
      acc[item.classification] = (acc[item.classification] || 0) + 1;
      return acc;
    }, {}),
    items,
  };
}

export function buildReviewQueue(diff, { repoRoot = null, projectName = null } = {}) {
  const items = [];
  let index = 0;

  for (const item of diff.items) {
    if (!['verter_only_likely_legit', 'verter_only_suspect', 'env_or_config', 'tool_crash'].includes(item.classification)) {
      continue;
    }

    const diagnostic = item.diagnostic;
    const comparisonSuggested = Boolean(repoRoot && diagnostic.file && diagnostic.file.endsWith('.vue'));
    const comparisonCommand = comparisonSuggested
      ? `node scripts/verter-compare-matrix.mjs --project "${repoRoot}" --component-filter "${diagnostic.file}"`
      : null;

    items.push({
      id: `${projectName ?? 'project'}-${String(index + 1).padStart(4, '0')}`,
      status: 'pending',
      classification: item.classification,
      tool: diagnostic.tool,
      pass: diagnostic.pass,
      file: diagnostic.file,
      line: diagnostic.line,
      column: diagnostic.column,
      code: diagnostic.code,
      message: diagnostic.message,
      rawLine: diagnostic.rawLine,
      comparisonSuggested,
      comparisonCommand,
    });
    index += 1;
  }

  return {
    schema: REVIEW_QUEUE_SCHEMA,
    items,
  };
}
