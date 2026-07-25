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
  // `renderTyped` is declared in `<script>` and used ONLY in `onclick={renderTyped}`.
  eventHandler: {
    file: "src/App.svelte",
    declaration: { file: "src/App.svelte", token: "renderTyped", occurrence: 0 },
    markupUse: { file: "src/App.svelte", token: "renderTyped", occurrence: 1 },
    allReferences: [
      { file: "src/App.svelte", token: "renderTyped", occurrence: 0 },
      { file: "src/App.svelte", token: "renderTyped", occurrence: 1 },
    ],
    hoverNeedles: ["renderTyped", "string"],
  },
  directParentTag: { file: "src/DirectParent.svelte", token: "DirectChild", occurrence: 2 },
  directChildFile: "src/components/DirectChild.svelte",
  directConsumerUse: { file: "src/direct-consumer.ts", token: "DirectChild", occurrence: 1 },
  directConsumerPropUse: {
    file: "src/direct-consumer.ts",
    token: "contractProp",
    occurrence: 0,
  },
  directChildPropDeclaration: {
    file: "src/components/DirectChild.svelte",
    token: "contractProp",
    occurrence: 1,
  },
  directComponentHoverNeedles: ["DirectChild", "contractProp", "string"],
  barrelParentTag: { file: "src/BarrelParent.svelte", token: "BarrelChild", occurrence: 1 },
  barrelChildFile: "src/components/BarrelChild.svelte",
  barrelConsumerUse: { file: "src/barrel-consumer.ts", token: "BarrelChild", occurrence: 1 },
  barrelConsumerPropUse: {
    file: "src/barrel-consumer.ts",
    token: "barrelProp",
    // Occurrence 0 lands inside the `barrelPropControl` identifier (substring
    // match); the intended usage is the `["barrelProp"]` type-index access.
    occurrence: 1,
  },
  barrelChildPropDeclaration: {
    file: "src/components/BarrelChild.svelte",
    token: "barrelProp",
    occurrence: 1,
  },
  publicTypeConsumer: "src/barrel-consumer.ts",
  barrelComponentHoverNeedles: ["BarrelChild", "barrelProp", "string"],
  completionProbeFile: "src/CompletionProbes.svelte",
  completionProbes: [
    {
      id: "intrinsic-bind",
      file: "src/CompletionProbes.svelte",
      caretAfter: `data-probe="intrinsic-bind"`,
      typed: " bind:",
      triggerCharacter: ":",
      gesture: "typing `bind:` on an intrinsic element",
    },
    {
      id: "intrinsic-event",
      file: "src/CompletionProbes.svelte",
      caretAfter: `data-probe="intrinsic-event"`,
      typed: " on",
      gesture: "typing `on` on an intrinsic element (Svelte 5 attribute form)",
    },
    {
      id: "intrinsic-class",
      file: "src/CompletionProbes.svelte",
      caretAfter: `data-probe="intrinsic-class"`,
      typed: " class:",
      triggerCharacter: ":",
      gesture: "typing `class:` on an intrinsic element",
    },
    {
      id: "component-prop",
      file: "src/CompletionProbes.svelte",
      caretAfter: `data-probe="component-prop"`,
      typed: " ",
      gesture: "attribute position on a component tag (declared props)",
    },
    {
      id: "each-iterable",
      file: "src/CompletionProbes.svelte",
      caretAfter: "{#each ",
      typed: "",
      gesture: "the iterable expression position of `{#each …}`",
    },
    {
      id: "member-in-markup",
      file: "src/CompletionProbes.svelte",
      caretAfter: "<span>{probeRow.",
      typed: "",
      triggerCharacter: ".",
      gesture: "member completion on an `{#each}`-declared binding",
    },
    {
      id: "script-member",
      file: "src/CompletionProbes.svelte",
      caretAfter: "= probeValue.",
      typed: "",
      triggerCharacter: ".",
      gesture: "member completion in the plain script region (control)",
    },
  ],
  assertedCompletions: {
    "member-in-markup": {
      mustOffer: ["probeLabel", "probeCount"],
      mustNotOffer: ["probeRow"],
      mustNotDisplace: [],
    },
    "script-member": {
      mustOffer: ["probeLabel", "probeCount"],
      mustNotOffer: ["probeValue"],
      mustNotDisplace: [],
    },
  },
};
