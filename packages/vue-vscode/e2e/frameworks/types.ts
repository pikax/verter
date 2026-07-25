import type { ContractFramework } from "../lib/frameworkContractManifest";
import type { CompletionExpectation, CompletionProbe } from "../lib/completionContract";

export interface ContractAnchor {
  readonly file: string;
  readonly token: string;
  readonly occurrence?: number;
}

export interface LocalCarrierCase {
  readonly file: string;
  readonly declaration: ContractAnchor;
  readonly markupUse: ContractAnchor;
  readonly allReferences: readonly ContractAnchor[];
  readonly hoverNeedles: readonly string[];
}

export interface FrameworkContractDescriptor {
  readonly framework: ContractFramework;
  readonly fixture: string;
  readonly entry: string;
  readonly languageId: string;
  readonly ts: LocalCarrierCase;
  readonly js: LocalCarrierCase;
  /**
   * A binding declared in the script and used ONLY inside an event-handler expression
   * (`@click="..."` / `onclick={...}`).
   *
   * The `ts`/`js` cases anchor on an interpolation, so every feature they exercise is
   * satisfied by lowering interpolations alone. Directive and event expression VALUES are
   * a separate region of the generated TS surface, and the Carrier IDE TS Surface Principle
   * requires them to behave identically. A binding reachable only through the handler is
   * the discriminating case: if the handler region is missing from the surface, definition,
   * references, rename, hover, and ctrl+click all degrade here while the interpolation
   * cases stay green.
   */
  readonly eventHandler: LocalCarrierCase;
  /**
   * Completion gestures, in the order a user meets them. Each probe names a caret a
   * user reaches by typing, so a completion source that regresses for one gesture
   * cannot hide behind another gesture that still answers.
   */
  readonly completionProbes: readonly CompletionProbe[];
  /** The file the completion probes type into (asserted diagnostic-clean). */
  readonly completionProbeFile: string;
  /**
   * The subset of probes whose three-part expectation is ASSERTED, keyed by probe id.
   *
   * A probe is surveyed (must answer at all) as soon as it exists; it graduates to an
   * asserted expectation only when the gesture's content is correct, because a required
   * capability that cannot pass takes its whole route down with it. Gestures that answer
   * with the wrong content are recorded as defects in the plan document instead — never
   * as a test that characterises the wrong answer as correct.
   */
  readonly assertedCompletions: Readonly<Record<string, CompletionExpectation>>;
  readonly directParentTag: ContractAnchor;
  readonly directChildFile: string;
  readonly directConsumerUse: ContractAnchor;
  readonly directConsumerPropUse: ContractAnchor;
  readonly directChildPropDeclaration: ContractAnchor;
  readonly directComponentHoverNeedles: readonly string[];
  readonly barrelParentTag: ContractAnchor;
  readonly barrelChildFile: string;
  readonly barrelConsumerUse: ContractAnchor;
  readonly barrelConsumerPropUse: ContractAnchor;
  readonly barrelChildPropDeclaration: ContractAnchor;
  readonly publicTypeConsumer: string;
  readonly barrelComponentHoverNeedles: readonly string[];
}
