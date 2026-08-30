/** @ai-generated - Verifies the staged Svelte closure through a real tsserver LSP session. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  buildComponentFixture,
  loadEnduranceConfig,
  type EnduranceLane,
} from "../src/endurance/index.js";
import {
  extractQuiescenceCounters,
  isQuiescenceWarnLine,
  pollUntilQuiesced,
} from "../src/core/quiescence.js";
import { GET_STATISTICS_METHOD } from "../src/core/startupGate.js";
import { disposeRig, materializeRig, type EnduranceRig } from "./endurance.helpers.js";

interface Diagnostic {
  readonly code?: string | number;
  readonly message?: string;
}

interface DiagnosticActivity {
  readonly byUri: Map<string, readonly Diagnostic[]>;
  readonly publishedUris: Set<string>;
  revision: number;
  lastAt: number;
}

function waitForDiagnosticsSettled(
  activity: DiagnosticActivity,
  expectedUris: ReadonlySet<string>,
  stableMs: number,
  timeoutMs: number,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = () => {
      const missing = [...expectedUris].filter((uri) => !activity.publishedUris.has(uri));
      if (missing.length === 0 && Date.now() - activity.lastAt >= stableMs) {
        if (timer) clearTimeout(timer);
        resolve();
        return;
      }
      if (Date.now() >= deadline) {
        reject(
          new Error(
            `publishDiagnostics did not settle (revision=${activity.revision}, missing=${JSON.stringify(missing)})`,
          ),
        );
        return;
      }
      timer = setTimeout(poll, 25);
      timer.unref?.();
    };
    poll();
  });
}

const config = loadEnduranceConfig();
const lane: EnduranceLane = { id: "svelte-ts", framework: "svelte", mode: "ts" };

describe.skipIf(config.route !== "tsserver")(
  "endurance: staged Svelte diagnostics [svelte-ts/tsserver]",
  () => {
    let rig: EnduranceRig | undefined;
    const activity: DiagnosticActivity = {
      byUri: new Map(),
      publishedUris: new Set(),
      revision: 0,
      lastAt: Date.now(),
    };

    beforeAll(async () => {
      const fixture = buildComponentFixture(lane);
      rig = await materializeRig(fixture.files, config, lane);
      rig.handle.client.onNotification("textDocument/publishDiagnostics", (params: any) => {
        if (typeof params?.uri !== "string") return;
        const diagnostics = Array.isArray(params.diagnostics) ? params.diagnostics : [];
        activity.byUri.set(params.uri, diagnostics);
        activity.publishedUris.add(params.uri);
        activity.revision += 1;
        activity.lastAt = Date.now();
      });

      const expectedUris = new Set([
        rig.session.uriFor(fixture.childPath),
        rig.session.uriFor(fixture.parentPath),
      ]);
      rig.session.openFile(fixture.childPath);
      rig.session.openFile(fixture.parentPath);

      let stderrCursor = rig.handle.client.stderr.lines().length;
      const quiescence = await pollUntilQuiesced(
        async () =>
          extractQuiescenceCounters(
            await rig!.handle.client.sendRequest(GET_STATISTICS_METHOD, {}, 10_000),
          ),
        () => {
          const lines = rig!.handle.client.stderr.lines();
          const fresh = lines.slice(stderrCursor);
          stderrCursor = lines.length;
          return fresh.filter((line) => isQuiescenceWarnLine(line));
        },
        { timeoutMs: 30_000 },
      );
      expect(quiescence.quiesced, quiescence.decision.reason).toBe(true);
      await waitForDiagnosticsSettled(activity, expectedUris, 600, 30_000);
    }, 120_000);

    afterAll(async () => {
      if (rig) await disposeRig(rig);
    });

    it("publishes settled diagnostics without missing declaration infrastructure", () => {
      const diagnostics = [...activity.byUri.values()].flat();
      const violations = diagnostics.filter((diagnostic) => {
        const code = diagnostic.code === undefined ? "" : String(diagnostic.code);
        const message = diagnostic.message ?? "";
        return (
          ["2307", "2688", "7026", "svelte-package-missing"].includes(code) ||
          /Cannot find module|Cannot find type definition file|JSX element implicitly has type 'any'|`svelte` is not installed/i.test(
            message,
          )
        );
      });
      expect(
        violations,
        `staged Svelte fixture diagnostics:\n${JSON.stringify(diagnostics, null, 2)}`,
      ).toEqual([]);
    });
  },
);
