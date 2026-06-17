/**
 * Extension-host log canary for the DX driver.
 *
 * The DX log collector reads `VERTER_E2E_LOG_FILE`, populated by the extension's E2E
 * log hook (packages/vue-vscode/src/extension.ts ~101-131), which wraps the
 * LogOutputChannel `log.info/warn/error/debug/trace` methods. The canary proves whether
 * a deterministic SERVER `tracing::warn!` line actually reaches that file. It is
 * provider-independent: the trigger is an MCP CLI deprecation, not a type-provider state.
 *
 * FORCING (deterministic, provider-independent): the canary launch pins
 * `verter.mcp.enabled=true` and `verter.typeProvider=off`, so `buildServerOptions`
 * passes `--mcp-port=0` and `--type-provider=off` to the server (extension.ts:1053/1061)
 * and logs those exact args through the hooked `log.info` `[buildServerOptions]` line
 * (extension.ts:1066). The server then emits an UNCONDITIONAL MCP-deprecation
 * `tracing::warn!` whenever `--mcp-port` is present (crates/verter_lsp/src/main.rs:63) —
 * to stderr, the same capture surface as every other server WARN. No global-TypeScript
 * fallthrough or provider-unavailability is involved.
 *
 * PROOF the forced config took effect: the captured `[buildServerOptions]` line carries
 * `--mcp-port=0` (forced by `verter.mcp.enabled=true`) AND `--type-provider=off` (forced
 * by `verter.typeProvider=off`, proving provider discovery is not the trigger). All three
 * substrings are required — a stray WARN without this proof is never accepted.
 *
 * This module is the canary's VERDICT logic only — pure string analysis over the
 * captured log. It does NOT patch the extension. The server writes `tracing` to stderr,
 * which the language client surfaces through the output channel via `append`/`appendLine`
 * — methods the current E2E hook does NOT wrap — so if the proof is present but the WARN
 * is absent, the verdict is `gated`: capturing it needs a product hook patch (sign-off).
 */

/** The type provider the canary pins; `--type-provider=off` proves provider discovery is not the trigger. */
export const CANARY_TYPE_PROVIDER = "off" as const;

/** Substring of the server's MCP-deprecation WARN (crates/verter_lsp/src/main.rs:63). */
export const CANARY_WARN_NEEDLES = ["--mcp-port is no longer honoured by verter-lsp"] as const;

/**
 * Substrings proving the forced MCP config reached the server args. The extension's
 * hooked `log.info` `[buildServerOptions]` line carries the launch args, so the captured
 * log must show the marker, `--mcp-port=0` (forced by `verter.mcp.enabled=true`), and
 * `--type-provider=${CANARY_TYPE_PROVIDER}` (forced by `verter.typeProvider=off`,
 * proving provider discovery is not the trigger). ALL are required.
 */
export const CANARY_PROOF_NEEDLES = [
  "[buildServerOptions]",
  "--mcp-port=0",
  `--type-provider=${CANARY_TYPE_PROVIDER}`,
] as const;

/** The diagnosis message for a proven capture gap (a product sign-off item). */
export const CANARY_GATED_REASON =
  "extension log-hook patch needs sign-off: the server's MCP-deprecation tracing::warn! " +
  "(`--mcp-port is no longer honoured by verter-lsp`) reaches the LSP output channel via " +
  "append/appendLine, but the VERTER_E2E_LOG_FILE hook wraps only " +
  "log.info/warn/error/debug/trace — so the forced server WARN was emitted but not captured. " +
  "Fix (product change, extension.ts ~101-131): also mirror append/appendLine into the file, " +
  "or add a dedicated server log sink.";

/** Inconclusive: a server WARN was captured but the forcing proof is absent (a stray WARN). */
export const CANARY_STRAY_WARN_REASON =
  "canary cannot confirm forcing: a server MCP-deprecation WARN was captured, but the launch " +
  "proof ([buildServerOptions] with --mcp-port=0 and --type-provider=" +
  `${CANARY_TYPE_PROVIDER}) is ABSENT — a stray WARN is never accepted as proof the forced ` +
  "config produced it. Check the canary launch logged its server args via log.info.";

/** Inconclusive: neither the forcing proof nor the WARN is present (config drift / no launch). */
export const CANARY_NOT_FORCED_REASON =
  "canary did not force the MCP config: the launch proof ([buildServerOptions] with " +
  "--mcp-port=0 and --type-provider=" +
  `${CANARY_TYPE_PROVIDER}) is absent (config drift). Pin verter.mcp.enabled=true and ` +
  "verter.typeProvider=off, and set VERTER_E2E_TYPE_PROVIDER=off so inherited env cannot override.";

/** The facts the canary derives from the captured log file. */
export interface CanaryFacts {
  /** The forced MCP config reached the server args (`--mcp-port=0` + `--type-provider=off`). */
  readonly configForced: boolean;
  /** The server's MCP-deprecation WARN line was captured in `VERTER_E2E_LOG_FILE`. */
  readonly warnCaptured: boolean;
}

/** Options for {@link summarizeCanaryLog}. */
export interface SummarizeCanaryOptions {
  readonly warnNeedles?: readonly string[];
  readonly proofNeedles?: readonly string[];
}

/** Reduce captured log text to the canary facts. */
export function summarizeCanaryLog(
  logText: string,
  opts: SummarizeCanaryOptions = {},
): CanaryFacts {
  const warnNeedles = opts.warnNeedles ?? CANARY_WARN_NEEDLES;
  const proofNeedles = opts.proofNeedles ?? CANARY_PROOF_NEEDLES;
  return {
    // ALL proof needles required — the [buildServerOptions] line carries every forced arg.
    configForced: proofNeedles.every((n) => logText.includes(n)),
    warnCaptured: warnNeedles.some((n) => logText.includes(n)),
  };
}

/** Canary verdict status. */
export type CanaryStatus = "ok" | "gated" | "inconclusive";

/** The canary's verdict. */
export interface CanaryVerdict {
  readonly status: CanaryStatus;
  /** Whether the WARN line was captured. */
  readonly captured: boolean;
  /** Human-readable explanation (always set for non-`ok` verdicts). */
  readonly reason?: string;
}

/**
 * Decide the canary verdict:
 *  - `ok` — the forced config reached the server AND its WARN was captured (hook sufficient);
 *  - `gated` — the forced config reached the server but the WARN was NOT captured (a product
 *    hook patch needs sign-off): the WARN was emitted server-side but did not reach the file;
 *  - `inconclusive` — the forcing proof is absent, so capture cannot be judged. NEVER a pass.
 *    A captured WARN WITHOUT the proof is a stray WARN, still inconclusive — the proof, not a
 *    bare WARN, is the gate. In a requested canary run the caller treats it as a visible failure.
 */
export function diagnoseCanary(facts: CanaryFacts): CanaryVerdict {
  if (!facts.configForced) {
    return {
      status: "inconclusive",
      captured: facts.warnCaptured,
      reason: facts.warnCaptured ? CANARY_STRAY_WARN_REASON : CANARY_NOT_FORCED_REASON,
    };
  }
  if (facts.warnCaptured) {
    return { status: "ok", captured: true };
  }
  return { status: "gated", captured: false, reason: CANARY_GATED_REASON };
}
