/**
 * Trace validator: checks a raw trace log against a desired trace spec.
 *
 * Supports:
 * - required events/patterns (must appear)
 * - forbidden events/patterns (must NOT appear)
 * - max count thresholds per event name
 * - max duration thresholds per event name
 * - normalized path matching (strips pnpm store hashes, absolute prefixes)
 * - notes explaining why each invariant exists
 */

// ── Trace spec types ──────────────────────────────────────────────────

export interface TraceSpec {
  /** Component name (e.g. "Accordion") */
  component: string;
  /** Path to the component file (normalized) */
  componentPath: string;
  /** Overall max duration in ms for the entire resolve_component_meta span */
  maxTotalDurationMs: number;
  /** Required event patterns — at least one matching event must appear */
  required: TraceAssertion[];
  /** Forbidden event patterns — no matching event may appear */
  forbidden: TraceAssertion[];
  /** Max count thresholds per event name pattern */
  maxCounts: TraceCountAssertion[];
  /** Max duration thresholds per event name */
  maxDurations: TraceDurationAssertion[];
  /** Result correctness assertions — extracted from extract_component_meta_declared_surface */
  expectedResult?: TraceResultAssertion;
}

export interface TraceResultAssertion {
  /** Minimum expected props count. Fails if actual < this. */
  minProps: number;
  /** Minimum expected events count. Fails if actual < this. */
  minEvents?: number;
  /** Minimum expected slots count. Fails if actual < this. */
  minSlots?: number;
  /** require has_evaluated_types=true in resolve_component_meta_result */
  requireEvaluatedTypes?: boolean;
  /** Why these thresholds exist */
  note: string;
}

export interface TraceAssertion {
  /** Event name pattern (exact match or regex if wrapped in /.../) */
  namePattern: string;
  /** Optional detail pattern to further filter (exact substring or regex) */
  detailPattern?: string;
  /** Why this assertion exists */
  note: string;
}

export interface TraceCountAssertion {
  /** Event name pattern */
  namePattern: string;
  /** Optional detail pattern */
  detailPattern?: string;
  /** Maximum allowed count */
  maxCount: number;
  /** Why this threshold exists */
  note: string;
}

export interface TraceDurationAssertion {
  /** Event name to check duration on (only start/end pairs have duration) */
  namePattern: string;
  /** Optional detail pattern */
  detailPattern?: string;
  /** Maximum allowed duration in ms for any single occurrence */
  maxDurationMs: number;
  /** Why this threshold exists */
  note: string;
}

// ── Parsed trace event ────────────────────────────────────────────────

export interface ParsedTraceEvent {
  type: "start" | "end" | "point";
  trace: string;
  span: string;
  parent: string;
  request: string;
  depth: number;
  thread: string;
  name: string;
  detail: string;
  durMs?: number;
}

export interface CoreEvent {
  name: string;
  detail: string;
  raw: string;
}

// ── Validation result ─────────────────────────────────────────────────

export interface ValidationResult {
  component: string;
  passed: boolean;
  failures: ValidationFailure[];
  summary: TraceSummary;
}

export interface ValidationFailure {
  kind:
    | "missing_required"
    | "forbidden_present"
    | "count_exceeded"
    | "duration_exceeded"
    | "total_duration_exceeded"
    | "result_incorrect";
  assertion: string;
  note: string;
  actual?: string;
}

export interface TraceSummary {
  totalDurationMs: number;
  eventCounts: Map<string, number>;
  maxDurations: Map<string, number>;
  uniqueFiles: Set<string>;
  /** Extracted from extract_component_meta_declared_surface event */
  declaredSurface?: { props: number; events: number; slots: number };
  /** Whether resolve_component_meta_result has has_evaluated_types=true */
  hasEvaluatedTypes: boolean;
}

// ── Parser ────────────────────────────────────────────────────────────

const TRACE_LINE_RE =
  /^\[verter-meta-trace\]\s+event=(\w+)\s+trace=(\S+)\s+span=(\S+)\s+parent=(\S+)\s+request=(\S+)\s+subrequest=\S+\s+caller=\S+\s+depth=(\d+)\s+thread=(\S+)\s+name="([^"]+)"\s+detail="([^"]*)"/;
const DUR_RE = /dur_ms=([\d.]+)/;
const CORE_EVENT_RE = /^core_event\s+name=(\S+)\s+(.*)/;

export function parseTraceLine(line: string): ParsedTraceEvent | null {
  const m = line.match(TRACE_LINE_RE);
  if (!m) return null;
  const durMatch = line.match(DUR_RE);
  return {
    type: m[1] as "start" | "end" | "point",
    trace: m[2],
    span: m[3],
    parent: m[4],
    request: m[5],
    depth: parseInt(m[6], 10),
    thread: m[7],
    name: m[8],
    detail: m[9],
    durMs: durMatch ? parseFloat(durMatch[1]) : undefined,
  };
}

export function parseCoreEvent(line: string): CoreEvent | null {
  const m = line.match(CORE_EVENT_RE);
  if (!m) return null;
  return { name: m[1], detail: m[2], raw: line };
}

export function parseTraceLog(content: string): {
  events: ParsedTraceEvent[];
  coreEvents: CoreEvent[];
} {
  const events: ParsedTraceEvent[] = [];
  const coreEvents: CoreEvent[] = [];
  for (const line of content.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const traceEvent = parseTraceLine(trimmed);
    if (traceEvent) {
      events.push(traceEvent);
      continue;
    }
    const coreEvent = parseCoreEvent(trimmed);
    if (coreEvent) {
      coreEvents.push(coreEvent);
    }
  }
  return { events, coreEvents };
}

// ── Path normalization ────────────────────────────────────────────────

/**
 * Normalize a path from trace detail for matching:
 * - Convert backslashes to forward slashes
 * - Strip pnpm store hash suffixes
 * - Strip absolute workspace prefix, keeping relative-to-project path
 */
export function normalizePath(raw: string, workspaceRoot?: string): string {
  let p = raw.replace(/\\/g, "/");
  // Strip pnpm .pnpm/<pkg>@<ver>_<hash>/node_modules/ → node_modules/
  p = p.replace(/node_modules\/\.pnpm\/[^/]+\/node_modules\//g, "node_modules/");
  if (workspaceRoot) {
    const root = workspaceRoot.replace(/\\/g, "/").replace(/\/$/, "") + "/";
    if (p.startsWith(root)) {
      p = p.slice(root.length);
    }
  }
  return p;
}

// ── Pattern matching ──────────────────────────────────────────────────

function matchesPattern(value: string, pattern: string): boolean {
  if (pattern.startsWith("/") && pattern.endsWith("/")) {
    return new RegExp(pattern.slice(1, -1)).test(value);
  }
  return value.includes(pattern);
}

function eventMatchesAssertion(
  event: ParsedTraceEvent | CoreEvent,
  namePattern: string,
  detailPattern?: string,
): boolean {
  const name = "type" in event ? event.name : event.name;
  if (!matchesPattern(name, namePattern)) return false;
  if (detailPattern) {
    const detail = event.detail;
    if (!matchesPattern(detail, detailPattern)) return false;
  }
  return true;
}

// ── Validator ─────────────────────────────────────────────────────────

export function buildSummary(events: ParsedTraceEvent[], coreEvents: CoreEvent[]): TraceSummary {
  const eventCounts = new Map<string, number>();
  const maxDurations = new Map<string, number>();
  const uniqueFiles = new Set<string>();

  // Count trace events
  for (const e of events) {
    eventCounts.set(e.name, (eventCounts.get(e.name) ?? 0) + 1);
    if (e.durMs !== undefined) {
      const current = maxDurations.get(e.name) ?? 0;
      if (e.durMs > current) maxDurations.set(e.name, e.durMs);
    }
    // Extract file paths from detail
    const pathMatch = e.detail.match(/(?:path|owner|canonical)=(\S+)/);
    if (pathMatch) uniqueFiles.add(pathMatch[1]);
  }

  // Count core events
  for (const e of coreEvents) {
    const key = `core:${e.name}`;
    eventCounts.set(key, (eventCounts.get(key) ?? 0) + 1);
  }

  // Find total duration from resolve_component_meta
  let totalDurationMs = 0;
  for (const e of events) {
    if (e.name === "resolve_component_meta" && e.type === "end" && e.durMs !== undefined) {
      totalDurationMs = Math.max(totalDurationMs, e.durMs);
    }
  }

  // Extract declared surface from extract_component_meta_declared_surface
  let declaredSurface: { props: number; events: number; slots: number } | undefined;
  for (const e of events) {
    if (e.name === "extract_component_meta_declared_surface") {
      const propsMatch = e.detail.match(/props=(\d+)/);
      const eventsMatch = e.detail.match(/events=(\d+)/);
      const slotsMatch = e.detail.match(/slots=(\d+)/);
      if (propsMatch) {
        declaredSurface = {
          props: parseInt(propsMatch[1], 10),
          events: eventsMatch ? parseInt(eventsMatch[1], 10) : 0,
          slots: slotsMatch ? parseInt(slotsMatch[1], 10) : 0,
        };
      }
    }
  }

  // Check for has_evaluated_types in resolve_component_meta_result
  let hasEvaluatedTypes = false;
  for (const e of events) {
    if (
      e.name === "resolve_component_meta_result" &&
      e.detail.includes("has_evaluated_types=true")
    ) {
      hasEvaluatedTypes = true;
    }
  }

  return {
    totalDurationMs,
    eventCounts,
    maxDurations,
    uniqueFiles,
    declaredSurface,
    hasEvaluatedTypes,
  };
}

export function validateTrace(
  spec: TraceSpec,
  events: ParsedTraceEvent[],
  coreEvents: CoreEvent[],
): ValidationResult {
  const failures: ValidationFailure[] = [];
  const summary = buildSummary(events, coreEvents);
  const allEvents = [...events, ...coreEvents] as (ParsedTraceEvent | CoreEvent)[];

  // Check total duration
  if (summary.totalDurationMs > spec.maxTotalDurationMs) {
    failures.push({
      kind: "total_duration_exceeded",
      assertion: `maxTotalDurationMs=${spec.maxTotalDurationMs}`,
      note: `Total resolve_component_meta took ${summary.totalDurationMs.toFixed(1)}ms, max allowed is ${spec.maxTotalDurationMs}ms`,
      actual: `${summary.totalDurationMs.toFixed(1)}ms`,
    });
  }

  // Check required patterns
  for (const req of spec.required) {
    const found = allEvents.some((e) =>
      eventMatchesAssertion(e, req.namePattern, req.detailPattern),
    );
    if (!found) {
      failures.push({
        kind: "missing_required",
        assertion: `required: name=${req.namePattern}${req.detailPattern ? ` detail=${req.detailPattern}` : ""}`,
        note: req.note,
      });
    }
  }

  // Check forbidden patterns
  for (const forbid of spec.forbidden) {
    const matches = allEvents.filter((e) =>
      eventMatchesAssertion(e, forbid.namePattern, forbid.detailPattern),
    );
    if (matches.length > 0) {
      failures.push({
        kind: "forbidden_present",
        assertion: `forbidden: name=${forbid.namePattern}${forbid.detailPattern ? ` detail=${forbid.detailPattern}` : ""}`,
        note: forbid.note,
        actual: `${matches.length} occurrences found`,
      });
    }
  }

  // Check max count thresholds
  for (const countSpec of spec.maxCounts) {
    const count = allEvents.filter((e) =>
      eventMatchesAssertion(e, countSpec.namePattern, countSpec.detailPattern),
    ).length;
    if (count > countSpec.maxCount) {
      failures.push({
        kind: "count_exceeded",
        assertion: `maxCount: name=${countSpec.namePattern} max=${countSpec.maxCount}`,
        note: countSpec.note,
        actual: `${count} occurrences (max ${countSpec.maxCount})`,
      });
    }
  }

  // Check max duration thresholds
  for (const durSpec of spec.maxDurations) {
    const matching = events.filter(
      (e) =>
        e.durMs !== undefined &&
        eventMatchesAssertion(e, durSpec.namePattern, durSpec.detailPattern),
    );
    for (const m of matching) {
      if (m.durMs! > durSpec.maxDurationMs) {
        failures.push({
          kind: "duration_exceeded",
          assertion: `maxDuration: name=${durSpec.namePattern} max=${durSpec.maxDurationMs}ms`,
          note: durSpec.note,
          actual: `${m.durMs!.toFixed(1)}ms on ${m.detail.slice(0, 120)}`,
        });
      }
    }
  }

  // Check result correctness
  if (spec.expectedResult) {
    const er = spec.expectedResult;
    if (!summary.declaredSurface) {
      failures.push({
        kind: "result_incorrect",
        assertion: "expectedResult: extract_component_meta_declared_surface must appear",
        note: er.note,
        actual: "no declared surface event found — component meta extraction may have failed",
      });
    } else {
      if (summary.declaredSurface.props < er.minProps) {
        failures.push({
          kind: "result_incorrect",
          assertion: `expectedResult: minProps=${er.minProps}`,
          note: er.note,
          actual: `${summary.declaredSurface.props} props (expected >= ${er.minProps})`,
        });
      }
      if (er.minEvents !== undefined && summary.declaredSurface.events < er.minEvents) {
        failures.push({
          kind: "result_incorrect",
          assertion: `expectedResult: minEvents=${er.minEvents}`,
          note: er.note,
          actual: `${summary.declaredSurface.events} events (expected >= ${er.minEvents})`,
        });
      }
      if (er.minSlots !== undefined && summary.declaredSurface.slots < er.minSlots) {
        failures.push({
          kind: "result_incorrect",
          assertion: `expectedResult: minSlots=${er.minSlots}`,
          note: er.note,
          actual: `${summary.declaredSurface.slots} slots (expected >= ${er.minSlots})`,
        });
      }
    }
    if (er.requireEvaluatedTypes && !summary.hasEvaluatedTypes) {
      failures.push({
        kind: "result_incorrect",
        assertion: "expectedResult: requireEvaluatedTypes=true",
        note: er.note,
        actual: "has_evaluated_types=false — type expansion may have failed or been skipped",
      });
    }
  }

  return {
    component: spec.component,
    passed: failures.length === 0,
    failures,
    summary,
  };
}

// ── Spec loader ───────────────────────────────────────────────────────

export interface LoadTraceSpecOptions {
  /** Require at least one forbidden assertion (default: false). */
  requireForbidden?: boolean;
  /** Require at least one maxCount assertion (default: false). */
  requireMaxCounts?: boolean;
}

export function loadTraceSpec(jsonContent: string, options?: LoadTraceSpecOptions): TraceSpec {
  const raw = JSON.parse(jsonContent);
  if (!raw.component || !raw.componentPath || !raw.maxTotalDurationMs) {
    throw new Error(
      "Invalid trace spec: missing required fields (component, componentPath, maxTotalDurationMs)",
    );
  }
  const spec: TraceSpec = {
    component: raw.component,
    componentPath: raw.componentPath,
    maxTotalDurationMs: raw.maxTotalDurationMs,
    required: raw.required ?? [],
    forbidden: raw.forbidden ?? [],
    maxCounts: raw.maxCounts ?? [],
    maxDurations: raw.maxDurations ?? [],
    expectedResult: raw.expectedResult ?? undefined,
  };
  if (options?.requireForbidden && spec.forbidden.length === 0) {
    throw new Error(
      `Trace spec for ${spec.component} has no forbidden assertions. ` +
        `Negative assertions are required to guard against legacy fallback paths.`,
    );
  }
  if (options?.requireMaxCounts && spec.maxCounts.length === 0) {
    throw new Error(
      `Trace spec for ${spec.component} has no maxCount assertions. ` +
        `Count thresholds are required to detect performance regressions.`,
    );
  }
  return spec;
}

// ── Report formatter ──────────────────────────────────────────────────

export function formatValidationResult(result: ValidationResult): string {
  const lines: string[] = [];
  const status = result.passed ? "PASS" : "FAIL";
  lines.push(`[${status}] ${result.component}`);
  lines.push(`  Total duration: ${result.summary.totalDurationMs.toFixed(1)}ms`);
  if (result.summary.declaredSurface) {
    const s = result.summary.declaredSurface;
    lines.push(`  Result: ${s.props} props, ${s.events} events, ${s.slots} slots`);
  }
  lines.push(`  Unique files touched: ${result.summary.uniqueFiles.size}`);

  if (result.failures.length > 0) {
    lines.push(`  Failures (${result.failures.length}):`);
    for (const f of result.failures) {
      lines.push(`    - [${f.kind}] ${f.assertion}`);
      if (f.actual) lines.push(`      actual: ${f.actual}`);
      lines.push(`      note: ${f.note}`);
    }
  }

  // Top event counts
  const sorted = [...result.summary.eventCounts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 15);
  lines.push(`  Top events:`);
  for (const [name, count] of sorted) {
    const dur = result.summary.maxDurations.get(name);
    lines.push(`    ${name}: ${count}${dur ? ` (max ${dur.toFixed(1)}ms)` : ""}`);
  }

  return lines.join("\n");
}
