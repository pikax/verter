import { describe, expect, it } from "vitest";

import {
  CANARY_GATED_REASON,
  CANARY_NOT_FORCED_REASON,
  CANARY_STRAY_WARN_REASON,
  CANARY_TYPE_PROVIDER,
  diagnoseCanary,
  summarizeCanaryLog,
} from "./dxLogCanary";

// Faithful captured-log fragments mirroring the real product output: the extension's
// hooked `log.info` `[buildServerOptions]` line (extension.ts:1066) and the server's
// MCP-deprecation WARN (crates/verter_lsp/src/main.rs:63). The proof line carries every
// forced arg, exactly as `buildServerOptions` serializes them via JSON.stringify(args).
const PROOF_LINE =
  `[INFO] [buildServerOptions] typeProvider=${CANARY_TYPE_PROVIDER}, tsdk=/x/lib (bundled), ` +
  `args=["--type-provider=${CANARY_TYPE_PROVIDER}","--tsdk=/x/lib",` +
  `"--plugin-path=/x/node_modules","--mcp-port=0","--mcp-lint-preset=recommended"]`;
const WARN_LINE =
  "[WARN] --mcp-port is no longer honoured by verter-lsp. The LSP binary no longer embeds the MCP server.";
// Provider-host noise that MUST be irrelevant to the canary (it keys on the MCP CLI, not
// on the provider). A host with global TypeScript looks exactly like this — and changes nothing.
const TS_GLOBAL_NOISE = "[INFO] Type provider status: tsserver (global TypeScript 5.6.2)";

describe("summarizeCanaryLog", () => {
  it("marks configForced only when ALL proof needles are present", () => {
    expect(summarizeCanaryLog(PROOF_LINE).configForced).toBe(true);
  });

  it("does NOT mark configForced when --mcp-port=0 is absent (a partial proof line)", () => {
    const partial =
      `[INFO] [buildServerOptions] typeProvider=${CANARY_TYPE_PROVIDER}, ` +
      `args=["--type-provider=${CANARY_TYPE_PROVIDER}","--tsdk=/x/lib"]`;
    expect(summarizeCanaryLog(partial).configForced).toBe(false);
  });

  it("does NOT mark configForced when the provider proof is not off (provider discovery is involved)", () => {
    const wrongProvider =
      "[INFO] [buildServerOptions] typeProvider=tsgo, " +
      'args=["--type-provider=tsgo","--mcp-port=0"]';
    expect(summarizeCanaryLog(wrongProvider).configForced).toBe(false);
  });

  it("detects the captured MCP-deprecation WARN", () => {
    expect(summarizeCanaryLog(WARN_LINE).warnCaptured).toBe(true);
  });

  it("reports warnCaptured=false when the WARN is absent (proof line only)", () => {
    const s = summarizeCanaryLog(PROOF_LINE);
    expect(s.configForced).toBe(true);
    expect(s.warnCaptured).toBe(false);
  });
});

// The closed test matrix from the binding canary ruling. Each row is a distinct,
// discriminating verdict — status AND the named reason, not just the status.
describe("diagnoseCanary — closed matrix", () => {
  it("Forced + captured ⇒ ok, captured=true", () => {
    const v = diagnoseCanary({ configForced: true, warnCaptured: true });
    expect(v.status).toBe("ok");
    expect(v.captured).toBe(true);
  });

  it("Forced + emitted-but-not-captured ⇒ gated, reason names the product hook gap", () => {
    const v = diagnoseCanary({ configForced: true, warnCaptured: false });
    expect(v.status).toBe("gated");
    expect(v.captured).toBe(false);
    expect(v.reason).toBe(CANARY_GATED_REASON);
    expect(v.reason).toMatch(/append\/appendLine/);
    expect(v.reason).toMatch(/sign-off/);
  });

  it("WARN without proof ⇒ inconclusive (stray WARN is never proof), LOUD failure", () => {
    const v = diagnoseCanary({ configForced: false, warnCaptured: true });
    expect(v.status).toBe("inconclusive");
    expect(v.status).not.toBe("ok");
    // A captured WARN that the proof does not back must NOT be accepted as a pass.
    expect(v.reason).toBe(CANARY_STRAY_WARN_REASON);
    expect(v.reason).toMatch(/stray WARN/);
  });

  it("Not forced ⇒ inconclusive naming the missing --mcp-port=0 launch proof, LOUD failure", () => {
    const v = diagnoseCanary({ configForced: false, warnCaptured: false });
    expect(v.status).toBe("inconclusive");
    expect(v.status).not.toBe("ok");
    expect(v.reason).toBe(CANARY_NOT_FORCED_REASON);
    expect(v.reason).toMatch(/--mcp-port=0/);
    expect(v.reason).toMatch(/drift/);
  });
});

describe("diagnoseCanary — end-to-end over captured-log fixtures", () => {
  it("proof line + WARN ⇒ ok", () => {
    const v = diagnoseCanary(summarizeCanaryLog([PROOF_LINE, WARN_LINE].join("\n")));
    expect(v.status).toBe("ok");
    expect(v.captured).toBe(true);
  });

  it("proof line, WARN absent ⇒ gated", () => {
    const v = diagnoseCanary(summarizeCanaryLog(PROOF_LINE));
    expect(v.status).toBe("gated");
  });

  it("WARN present, proof line absent ⇒ inconclusive (stray WARN)", () => {
    const v = diagnoseCanary(summarizeCanaryLog(WARN_LINE));
    expect(v.status).toBe("inconclusive");
    expect(v.reason).toBe(CANARY_STRAY_WARN_REASON);
  });

  it("empty log ⇒ inconclusive (not forced)", () => {
    const v = diagnoseCanary(summarizeCanaryLog(""));
    expect(v.status).toBe("inconclusive");
    expect(v.reason).toBe(CANARY_NOT_FORCED_REASON);
  });
});

// The MCP-WARN canary is provider-independent: a TS-global host must not change any
// verdict, and there is NO "expected TS-global inconclusive" special case.
describe("TS-global host does not affect the canary (provider-independent)", () => {
  it("a TS-global provider line plus the full proof + WARN still ⇒ ok", () => {
    const v = diagnoseCanary(
      summarizeCanaryLog([TS_GLOBAL_NOISE, PROOF_LINE, WARN_LINE].join("\n")),
    );
    expect(v.status).toBe("ok");
  });

  it("a TS-global provider line WITHOUT the MCP proof is inconclusive, never a special pass", () => {
    const v = diagnoseCanary(summarizeCanaryLog(TS_GLOBAL_NOISE));
    expect(v.status).toBe("inconclusive");
    expect(v.status).not.toBe("ok");
    // No TS-global special-casing: the verdict is the generic not-forced reason.
    expect(v.reason).toBe(CANARY_NOT_FORCED_REASON);
  });
});
