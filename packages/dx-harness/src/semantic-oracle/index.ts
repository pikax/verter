/**
 * The curated semantic-oracle runner: the descriptor {@link ./model}, the pure
 * fact extractors {@link ./facts}, the anchor → byte-offset {@link ./prepare}, and
 * the orchestration {@link ./runner} that drives verter-on-`.vue` against the
 * tsgo/tsserver-on-`.ts` gold standard and classifies through the
 * `vueSemanticValidity` diff.
 */

export {
  ORACLE_FAMILIES,
  ORACLE_QUERY_METHODS,
  isOracleFamily,
  isOracleQueryMethod,
  type OracleBinding,
  type OracleFamily,
  type OracleQueryMethod,
  type SemanticOracle,
} from "./model.js";

export {
  prepareOracleSource,
  requireOracleByteOffset,
  type PreparedOracleSource,
} from "./prepare.js";

export {
  bridgeCompletionFact,
  bridgeDefinitionFact,
  bridgeDiagnosticsFact,
  bridgeHoverFact,
  verterCompletionFact,
  verterDefinitionFact,
  verterHoverFact,
} from "./facts.js";

export {
  OracleError,
  resolveLspPosition,
  resolveOracleQuery,
  runResolvedOracleQuery,
  runSemanticOracle,
  type OracleBridgeClient,
  type OracleProviders,
  type OracleSourceContext,
  type OracleVerterClient,
  type ResolvedOracleQuery,
} from "./runner.js";
