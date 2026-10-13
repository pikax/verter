import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** Freeze both checkout identities before this measurement builds either side. */
export function resolveMeasurementBaseline(
  cwd: string,
  candidateRef: string,
  baselineRef?: string,
): { candidate: string; baseline: string } {
  const commit = (ref: string): string =>
    execFileSync("git", ["rev-parse", "--verify", "--end-of-options", `${ref}^{commit}`], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  const candidate = commit(candidateRef);
  const baseline = commit(baselineRef?.trim() || `${candidate}^`);
  if (candidate === baseline) {
    throw new Error(
      "Comparison baseline equals candidate; refusing a self-check as a regression gate.",
    );
  }
  return { candidate, baseline };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const identities = resolveMeasurementBaseline(
    process.cwd(),
    process.env.CANDIDATE_REF || "HEAD",
    process.env.BASELINE_REF,
  );
  console.log(JSON.stringify(identities));
  if (process.env.GITHUB_OUTPUT) {
    appendFileSync(process.env.GITHUB_OUTPUT, `ref=${identities.baseline}\n`);
  }
}
