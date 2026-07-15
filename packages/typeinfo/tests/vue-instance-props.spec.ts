/**
 * Vue worked example: a Vue SFC declares its props through
 * `defineProps<{ msg: string }>()`; evaluating
 * `InstanceType<typeof default>['$props']` against that SFC's scope
 * via `TypeInfoSession.evaluateTypeExpression` MUST resolve to an
 * Object descriptor carrying a `msg: string` property.
 *
 * This test exercises the actual `.vue` SFC pipeline end-to-end:
 * the host parses the script-setup block, the substrate
 * (`vue_default_synth`) synthesises the implicit `default` value
 * symbol from the file's type-based macros, and
 * `evaluate_type_expression` inlines the scope's eval-source into
 * the scratch file so `typeof default` resolves to the synthesised
 * Object surface.
 *
 * REGRESSION — fails if the `.vue` scope's default does not publish
 * `$props` natively, or if the typeinfo scratch file does not
 * inherit the scope's eval-source as a prelude. In either case the
 * `'$props'` projection would not reduce and the result would be
 * `IndexedAccess { object: ..., index: '$props' }` instead of an
 * Object descriptor.
 */

import { describe, expect, it } from "vitest";

import { TypeInfoSession } from "../src/index.js";

const VUE_SFC_SOURCE = `<script setup lang="ts">
defineProps<{ msg: string }>();
</script>
<template>
  <div>{{ msg }}</div>
</template>
`;

describe("TypeInfoSession Vue instance props worked example", () => {
  it("evaluates InstanceType<typeof default>['$props'] against a real .vue SFC scope", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      canonicalId: "/fixtures/MyButton.vue",
      inputId: "/fixtures/MyButton.vue",
      source: Buffer.from(VUE_SFC_SOURCE, "utf-8"),
    });

    const result = session.evaluateTypeExpression({
      scope: "/fixtures/MyButton.vue",
      expression: "InstanceType<typeof default>['$props']",
      mode: "expanded",
      cacheable: false,
    });

    // Discriminating contract: the result MUST be an `object`
    // descriptor whose property list contains `msg: string`. A
    // regression would produce an `indexed-access` (or similar
    // non-object) shape because `default` would not resolve to a
    // concrete surface in the scratch's scope.
    expect(result.type).toBeDefined();
    expect(result.type?.kind).toBe("object");
    if (result.type?.kind === "object") {
      const propertyNames = result.type.properties.map((p) => p.name);
      expect(propertyNames).toContain("msg");
      const msg = result.type.properties.find((p) => p.name === "msg");
      expect(msg).toBeDefined();
      expect(msg!.type.kind).toBe("primitive");
      if (msg!.type.kind === "primitive") {
        expect(msg!.type.name).toBe("string");
      }
    }

    session.host.close();
  });
});
