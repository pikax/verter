import type { FrameworkContractDescriptor } from "../types";

export const svelteContract: FrameworkContractDescriptor = {
  framework: "svelte",
  fixture: "svelte-contract",
  entry: "src/App.svelte",
  languageId: "svelte",
  ts: {
    file: "src/App.svelte",
    declaration: { file: "src/App.svelte", token: "typedValue", occurrence: 0 },
    markupUse: { file: "src/App.svelte", token: "typedValue", occurrence: 3 },
    allReferences: [
      { file: "src/App.svelte", token: "typedValue", occurrence: 0 },
      { file: "src/App.svelte", token: "typedValue", occurrence: 1 },
      { file: "src/App.svelte", token: "typedValue", occurrence: 2 },
      { file: "src/App.svelte", token: "typedValue", occurrence: 3 },
      { file: "src/App.svelte", token: "typedValue", occurrence: 4 },
    ],
    hoverNeedles: ["typedValue", "ContractValue"],
  },
  js: {
    file: "src/JavaScriptCase.svelte",
    declaration: { file: "src/JavaScriptCase.svelte", token: "jsValue", occurrence: 0 },
    markupUse: { file: "src/JavaScriptCase.svelte", token: "jsValue", occurrence: 2 },
    allReferences: [
      { file: "src/JavaScriptCase.svelte", token: "jsValue", occurrence: 0 },
      { file: "src/JavaScriptCase.svelte", token: "jsValue", occurrence: 1 },
      { file: "src/JavaScriptCase.svelte", token: "jsValue", occurrence: 2 },
    ],
    hoverNeedles: ["jsValue", "label", "string"],
  },
  directParentTag: { file: "src/DirectParent.svelte", token: "DirectChild", occurrence: 1 },
  directChildFile: "src/components/DirectChild.svelte",
  directConsumerUse: { file: "src/direct-consumer.ts", token: "DirectChild", occurrence: 1 },
  directComponentHoverNeedles: ["DirectChild", "contractProp", "string"],
  barrelParentTag: { file: "src/BarrelParent.svelte", token: "BarrelChild", occurrence: 1 },
  barrelChildFile: "src/components/BarrelChild.svelte",
  barrelConsumerUse: { file: "src/barrel-consumer.ts", token: "BarrelChild", occurrence: 1 },
  publicTypeConsumer: "src/barrel-consumer.ts",
  barrelComponentHoverNeedles: ["BarrelChild", "barrelProp", "string"],
};
