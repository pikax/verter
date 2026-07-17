import type { ContractFramework } from "../lib/frameworkContractManifest";

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
