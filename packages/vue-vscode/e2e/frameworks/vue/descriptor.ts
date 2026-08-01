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
  // `renderTyped` is declared in `<script setup>` and used ONLY in `@click="renderTyped"`.
  eventHandler: {
    file: "src/App.vue",
    declaration: { file: "src/App.vue", token: "renderTyped", occurrence: 0 },
    markupUse: { file: "src/App.vue", token: "renderTyped", occurrence: 1 },
    allReferences: [
      { file: "src/App.vue", token: "renderTyped", occurrence: 0 },
      { file: "src/App.vue", token: "renderTyped", occurrence: 1 },
    ],
    hoverNeedles: ["renderTyped", "string"],
  },
  directParentTag: { file: "src/DirectParent.vue", token: "DirectChild", occurrence: 2 },
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
    // Occurrence 0 lands inside the `barrelPropControl` identifier (substring
    // match); the intended usage is the `["barrelProp"]` type-index access.
    occurrence: 1,
  },
  barrelChildPropDeclaration: {
    file: "src/components/BarrelChild.vue",
    token: "barrelProp",
    occurrence: 0,
  },
  publicTypeConsumer: "src/barrel-consumer.ts",
  barrelComponentHoverNeedles: ["BarrelChild", "barrelProp", "string"],
  completionProbeFile: "src/CompletionProbes.vue",
  completionProbes: [
    {
      id: "component-attr",
      file: "src/CompletionProbes.vue",
      caretAfter: `data-probe="component-attr"`,
      typed: " :",
      triggerCharacter: ":",
      gesture: "typing `:` inside a component tag",
    },
    {
      id: "component-event",
      file: "src/CompletionProbes.vue",
      caretAfter: `data-probe="component-event"`,
      typed: " @",
      triggerCharacter: "@",
      gesture: "typing `@` inside a component tag",
    },
    {
      id: "component-slot",
      file: "src/CompletionProbes.vue",
      caretAfter: `data-probe="component-slot"`,
      typed: " #",
      triggerCharacter: "#",
      gesture: "typing `#` inside a component tag (slot names)",
    },
    {
      id: "component-directive",
      file: "src/CompletionProbes.vue",
      caretAfter: `data-probe="component-directive"`,
      typed: " v-",
      triggerCharacter: "-",
      gesture: "typing `v-` inside a component tag (directives)",
    },
    {
      id: "intrinsic-attr",
      file: "src/CompletionProbes.vue",
      caretAfter: `data-probe="intrinsic-attr"`,
      typed: " :",
      triggerCharacter: ":",
      gesture: "typing `:` inside an intrinsic element",
    },
    {
      id: "intrinsic-event",
      file: "src/CompletionProbes.vue",
      caretAfter: `data-probe="intrinsic-event"`,
      typed: " @",
      triggerCharacter: "@",
      gesture: "typing `@` inside an intrinsic element",
    },
    {
      id: "member-in-directive",
      file: "src/CompletionProbes.vue",
      caretAfter: `:contract-prop="probeValue.`,
      typed: "",
      triggerCharacter: ".",
      gesture: "member completion INSIDE a directive value",
    },
    {
      id: "slot-scope",
      file: "src/CompletionProbes.vue",
      caretAfter: `v-slot="{ `,
      typed: "",
      gesture: 'destructuring slot props in `v-slot="{ … }"`',
    },
    {
      id: "dynamic-is",
      file: "src/CompletionProbes.vue",
      caretAfter: `:is="`,
      typed: "",
      gesture: 'choosing a dynamic component in `<component :is="…">`',
    },
    {
      id: "script-member",
      file: "src/CompletionProbes.vue",
      caretAfter: "= probeValue.",
      typed: "",
      triggerCharacter: ".",
      gesture: "member completion in the plain script region (control)",
    },
  ],
  assertedCompletions: {
    // Typing `@` on a component offers its DECLARED emit, and the vnode/HTML sources
    // that were already there are not displaced by it. A script binding must never
    // appear in an attribute-NAME position.
    "component-event": {
      mustOffer: ["@host-ping"],
      mustNotOffer: ["probeValue", "probeDynamic"],
      mustNotDisplace: ["class", "style", "key"],
    },
    // A member list must contain the members and NOT the object itself — that is what
    // separates a real member completion from a scope dump that happens to include them.
    "member-in-directive": {
      mustOffer: ["probeLabel", "probeCount"],
      mustNotOffer: ["probeValue"],
      mustNotDisplace: [],
    },
    "slot-scope": {
      mustOffer: ["hostDatum"],
      mustNotOffer: ["probeValue"],
      mustNotDisplace: [],
    },
    "dynamic-is": {
      mustOffer: ["RegionSlotHost", "DirectChild"],
      mustNotOffer: ["host-prop"],
      mustNotDisplace: [],
    },
    "script-member": {
      mustOffer: ["probeLabel", "probeCount"],
      mustNotOffer: ["probeValue"],
      mustNotDisplace: [],
    },
  },
};
