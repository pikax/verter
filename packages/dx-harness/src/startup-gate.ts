/**
 * Dependency-light public subpath: `@verter/dx-harness/startup-gate`.
 *
 * The extension-host DX gate runs inside real VS Code as CommonJS, but it must
 * consume the SAME startup-readiness parser + matching-generation fold the raw-LSP
 * gate uses — there is exactly one startup-gate engine, never an in-host port. The
 * package barrel (`.`) re-exports the whole harness (collectors, differential,
 * report, the `@verter/lsp-test-client` dependency, …); loading that from the
 * extension host risks heavy transitive ESM loads.
 *
 * This subpath re-exports ONLY the two pure, zero-dependency, zero-I/O startup-gate
 * modules, so the in-host gate can `import()` it at runtime (true dynamic import,
 * surviving the suite's CommonJS emit) without pulling the rest of the harness. It
 * is the CJS-compatible escape hatch the module-boundary rule requires.
 */
export {
  ExtensionStartupGate,
  TYPE_PROVIDER_SYNC_COMPLETE_LOG_PATTERN,
  VERTER_READY_LOG_PATTERN,
  parseExtensionStartupLog,
  parseStartupLogLine,
} from "./core/extensionStartup.js";

export {
  GenerationGate,
  evaluateGenerationGate,
  type GenerationChannel,
  type GenerationEvent,
  type GenerationGateDecision,
} from "./core/generationGate.js";
