import { describe, expect, it } from "vitest";
import { CLIENT_FRAMEWORKS, CLIENT_FRAMEWORK_LANGUAGE_IDS } from "@verter/language-shared";
import {
  frameworkDocumentSelector,
  frameworkClientLanguageIds,
  isCarrierComponentImport,
  isFrameworkCarrierLanguageId,
  shouldConfigureTypeScriptPluginForLanguageId,
} from "./frameworkWiring";

describe("framework wiring (manifest-driven)", () => {
  it("the manifest lists BOTH vue and svelte (svelte is no longer opt-in-gated)", () => {
    const ids = CLIENT_FRAMEWORKS.map((f) => f.frameworkId);
    expect(ids).toContain("vue");
    expect(ids).toContain("svelte");
  });

  it("derives the framework client language ids from the manifest", () => {
    const ids = frameworkClientLanguageIds();
    expect(ids).toContain("vue");
    expect(ids).toContain("svelte");
    // The ids are exactly the manifest's framework language ids.
    expect(ids).toEqual([...CLIENT_FRAMEWORK_LANGUAGE_IDS]);
  });

  it("builds the document selector from the manifest framework rows + the plain TS/JS base", () => {
    const selector = frameworkDocumentSelector();
    // Every framework client language id is selected for the file scheme.
    expect(selector).toContainEqual({ scheme: "file", language: "vue" });
    expect(selector).toContainEqual({ scheme: "file", language: "svelte" });
    // The plain TS/JS base is selected.
    expect(selector).toContainEqual({ scheme: "file", language: "javascript" });
    expect(selector).toContainEqual({ scheme: "file", language: "typescript" });
    // The React dialects are NOT in the LSP document selector — they are
    // activation / plugin-configure surfaces only (Vue selector preserved).
    expect(selector).not.toContainEqual({ scheme: "file", language: "javascriptreact" });
    expect(selector).not.toContainEqual({ scheme: "file", language: "typescriptreact" });
  });

  it("recognises a framework carrier document by its manifest client language id", () => {
    expect(isFrameworkCarrierLanguageId("vue")).toBe(true);
    expect(isFrameworkCarrierLanguageId("svelte")).toBe(true);
    expect(isFrameworkCarrierLanguageId("typescript")).toBe(false);
    expect(isFrameworkCarrierLanguageId(undefined)).toBe(false);
  });

  it("recognises a framework-carrier component import by ANY carrier extension", () => {
    // Carrier-generic: both `.vue` and `.svelte` component imports are carriers.
    expect(isCarrierComponentImport("./Foo.vue")).toBe(true);
    expect(isCarrierComponentImport("../components/Bar.svelte")).toBe(true);
    expect(isCarrierComponentImport("@/widgets/Baz.vue")).toBe(true);
    // A non-carrier import (npm package, plain module, bare specifier) is not.
    expect(isCarrierComponentImport("vue")).toBe(false);
    expect(isCarrierComponentImport("./helpers/util")).toBe(false);
    expect(isCarrierComponentImport("./types.ts")).toBe(false);
    expect(isCarrierComponentImport(undefined)).toBe(false);
  });

  it("configures the built-in TS plugin for the manifest trigger language ids", () => {
    // The base TS/JS surface triggers the built-in TS plugin configure.
    expect(shouldConfigureTypeScriptPluginForLanguageId("typescript")).toBe(true);
    expect(shouldConfigureTypeScriptPluginForLanguageId("typescriptreact")).toBe(true);
    expect(shouldConfigureTypeScriptPluginForLanguageId("javascript")).toBe(true);
    expect(shouldConfigureTypeScriptPluginForLanguageId("javascriptreact")).toBe(true);
    // A framework document must publish the membership/source-owner policy
    // before either editor TypeScript or the managed provider can answer it.
    expect(shouldConfigureTypeScriptPluginForLanguageId("vue")).toBe(true);
    expect(shouldConfigureTypeScriptPluginForLanguageId("svelte")).toBe(true);
    // A non-trigger language is not configured.
    expect(shouldConfigureTypeScriptPluginForLanguageId("plaintext")).toBe(false);
    expect(shouldConfigureTypeScriptPluginForLanguageId(undefined)).toBe(false);
  });
});
