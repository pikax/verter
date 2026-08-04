/**
 * @ai-generated - Verifies representative Vue SFC classifications from the
 * generated Monarch tokenizer after its grammar-authority migration.
 */

// @vitest-environment happy-dom

import { describe, expect, it } from "vitest";
import * as monaco from "monaco-editor-core";

import { registerVueLanguage } from "./vueLanguage";

describe("generated Vue Monarch language", () => {
  it("classifies representative script, template, and style tokens", () => {
    registerVueLanguage();
    const fixture = `<script setup lang="ts" generic="T extends string">
const count: number = 1
</script>
<template><button :disabled="count === 0">{{ count + 1 }}</button></template>
<style scoped>.button { color: #fff; }</style>`;
    const classifications = monaco.editor
      .tokenize(fixture, "vue")
      .flat()
      .map((token) => token.type);

    expect(classifications).toEqual(
      expect.arrayContaining([
        "attribute.name.vue",
        "attribute.value.vue",
        "delimiter.html.vue",
        "delimiter.interpolation.vue",
        "identifier.vue",
        "keyword.vue",
        "number.vue",
        "tag.class.vue",
        "tag.vue",
        "type.vue",
      ]),
    );
  });
});
