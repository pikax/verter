import type { FrameworkContractDescriptor } from "../types";

export const vueContract: FrameworkContractDescriptor = {
  framework: "vue",
  fixture: "vue-contract",
  entry: "src/App.vue",
  languageId: "vue",
  ts: {
    file: "src/App.vue",
    declaration: { file: "src/App.vue", token: "typedValue", occurrence: 0 },
    markupUse: { file: "src/App.vue", token: "typedValue", occurrence: 3 },
    allReferences: [
      { file: "src/App.vue", token: "typedValue", occurrence: 0 },
      { file: "src/App.vue", token: "typedValue", occurrence: 1 },
      { file: "src/App.vue", token: "typedValue", occurrence: 2 },
      { file: "src/App.vue", token: "typedValue", occurrence: 3 },
      { file: "src/App.vue", token: "typedValue", occurrence: 4 },
    ],
    hoverNeedles: ["typedValue", "ContractValue"],
  },
  js: {
    file: "src/JavaScriptCase.vue",
    declaration: { file: "src/JavaScriptCase.vue", token: "jsValue", occurrence: 0 },
    markupUse: { file: "src/JavaScriptCase.vue", token: "jsValue", occurrence: 2 },
    allReferences: [
      { file: "src/JavaScriptCase.vue", token: "jsValue", occurrence: 0 },
      { file: "src/JavaScriptCase.vue", token: "jsValue", occurrence: 1 },
      { file: "src/JavaScriptCase.vue", token: "jsValue", occurrence: 2 },
    ],
    hoverNeedles: ["jsValue", "label", "string"],
  },
  directParentTag: { file: "src/DirectParent.vue", token: "DirectChild", occurrence: 1 },
  directChildFile: "src/components/DirectChild.vue",
  directConsumerUse: { file: "src/direct-consumer.ts", token: "DirectChild", occurrence: 1 },
  directConsumerPropUse: {
    file: "src/direct-consumer.ts",
    token: "contractProp",
    occurrence: 0,
  },
  directChildPropDeclaration: {
    file: "src/components/DirectChild.vue",
    token: "contractProp",
    occurrence: 0,
  },
  directComponentHoverNeedles: ["DirectChild", "contractProp", "string"],
  barrelParentTag: { file: "src/BarrelParent.vue", token: "BarrelChild", occurrence: 1 },
  barrelChildFile: "src/components/BarrelChild.vue",
  barrelConsumerUse: { file: "src/barrel-consumer.ts", token: "BarrelChild", occurrence: 1 },
  barrelConsumerPropUse: {
    file: "src/barrel-consumer.ts",
    token: "barrelProp",
    occurrence: 0,
  },
  barrelChildPropDeclaration: {
    file: "src/components/BarrelChild.vue",
    token: "barrelProp",
    occurrence: 0,
  },
  publicTypeConsumer: "src/barrel-consumer.ts",
  barrelComponentHoverNeedles: ["BarrelChild", "barrelProp", "string"],
};
