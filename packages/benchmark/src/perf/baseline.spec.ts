import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { resolveMeasurementBaseline } from "./baseline.js";

describe("runtime measurement baseline", () => {
  it("resolves the candidate parent and explicit refs without retaining a history pin", () => {
    const cwd = mkdtempSync(join(tmpdir(), "verter-perf-baseline-"));
    const git = (...args: string[]): string =>
      execFileSync("git", ["-c", `core.hooksPath=${join(cwd, "disabled-hooks")}`, ...args], {
        cwd,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      }).trim();
    const commit = (title: string): string => {
      git(
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgSign=false",
        "commit",
        "--allow-empty",
        "-m",
        title,
      );
      return git("rev-parse", "HEAD");
    };
    try {
      git("init", "--quiet");
      const base = commit("baseline");
      expect(() => resolveMeasurementBaseline(cwd, base)).toThrow();
      git("tag", "measurement-base");
      const parent = commit("parent");
      const candidate = commit("candidate");
      expect(resolveMeasurementBaseline(cwd, candidate)).toEqual({ candidate, baseline: parent });
      expect(resolveMeasurementBaseline(cwd, "HEAD", "measurement-base")).toEqual({
        candidate,
        baseline: base,
      });
      // The resolved result remains fixed even when the branch moves.
      const frozen = resolveMeasurementBaseline(cwd, "HEAD");
      commit("later");
      expect(resolveMeasurementBaseline(cwd, frozen.candidate, frozen.baseline)).toEqual(frozen);
      expect(() => resolveMeasurementBaseline(cwd, candidate, candidate)).toThrow(
        /equals candidate/,
      );
      expect(() => resolveMeasurementBaseline(cwd, candidate, "missing-ref")).toThrow();
      expect(() => resolveMeasurementBaseline(cwd, candidate, "--help")).toThrow();
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});
