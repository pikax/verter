import type { FileAnalysisSnapshot } from "@verter/language-shared";

export interface SsrIssue {
  severity: "error" | "warning" | "info";
  type: string;
  detail: string;
}

export interface SsrReadinessResult {
  score: number;
  issues: SsrIssue[];
}

const CLIENT_ONLY_HOOKS = new Set([
  "OnMounted",
  "OnUpdated",
  "OnBeforeMount",
  "OnBeforeUnmount",
  "OnActivated",
  "OnDeactivated",
]);

export function computeSsrReadiness(analysis: FileAnalysisSnapshot): SsrReadinessResult {
  let score = 100;
  const issues: SsrIssue[] = [];

  // Client-only lifecycle hooks: -15 each
  for (const call of analysis.vueApiCalls ?? []) {
    if (CLIENT_ONLY_HOOKS.has(call.api)) {
      score -= 15;
      issues.push({
        severity: "error",
        type: "client-only-lifecycle",
        detail: `\`${call.api}\` never fires during SSR`,
      });
    }
  }

  // DOM queries: -20 each
  for (const query of analysis.domQueryCalls ?? []) {
    score -= 20;
    issues.push({
      severity: "error",
      type: "dom-query",
      detail: `\`${query.kind}\` has no DOM on server`,
    });
  }

  // CSS var manipulations: -10 each
  for (const manip of analysis.cssVarManipulations ?? []) {
    score -= 10;
    issues.push({
      severity: "warning",
      type: "css-var-manipulation",
      detail: `\`${manip.kind}\` requires DOM access`,
    });
  }

  // Async setup without onServerPrefetch: -5
  const ASYNC_SETUP_FLAG = 1 << 0;
  const hasAsyncSetup = (analysis.scriptFlags & ASYNC_SETUP_FLAG) !== 0;
  const hasServerPrefetch = (analysis.vueApiCalls ?? []).some((c) => c.api === "OnServerPrefetch");

  if (hasAsyncSetup && !hasServerPrefetch) {
    score -= 5;
    issues.push({
      severity: "info",
      type: "missing-server-prefetch",
      detail: "Async setup without `onServerPrefetch` — data won't be pre-fetched on server",
    });
  }

  // useTemplateRef: -5 each
  for (const call of analysis.vueApiCalls ?? []) {
    if (call.api === "UseTemplateRef") {
      score -= 5;
      issues.push({
        severity: "warning",
        type: "template-ref",
        detail: "Template refs are `null` during SSR",
      });
    }
  }

  // Bonus: has onServerPrefetch: +5
  if (hasServerPrefetch) {
    score += 5;
  }

  // Clamp to [0, 100]
  score = Math.max(0, Math.min(100, score));

  return { score, issues };
}
