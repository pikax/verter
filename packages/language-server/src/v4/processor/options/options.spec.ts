import { createContext } from "@verter/core";
import { processOptions } from "./options.js";
import { type Position, SourceMapConsumer } from "source-map-js";
import { MagicString } from "vue/compiler-sfc";

import fs from "fs";
import { expectFindStringWithMap } from "../../../utils.test-utils.js";

describe("processor options", () => {
  function process(source: string, filename = "test.vue") {
    const context = createContext(source, filename);
    return processOptions(context);
  }

  describe("filename", () => {
    test.each(["ts", "js", "jsx", "tsx", ""])("correct filename %s", (lang) => {
      const result = process(
        `<script${lang ? ` lang="${lang}"` : ""}>export default {};</script>`
      );
      expect(result).toMatchObject({
        filename: "test.vue.options." + (lang || "js"),
      });
    });
  });

  it("should have the correct loc", () => {
    const result = process(
      `<script>export default {}</script><template><div></div></template>`
    );
    expect(result.loc).toMatchInlineSnapshot(`
      {
        "source": "<script>export default {}</script><template><div></div></template>",
      }
    `);
  });

  describe.skip("compile export", () => {
    test("script", () => {
      const result = process(`<script>export default {}</script>`);

      expect(result.content).toContain(
        "const ____VERTER_COMP_OPTION__COMPILED = {}"
      );
    });
    test("defineComponent", () => {
      const result = process(
        `<script>export default defineComponent({})</script>`
      );
      expect(result.content).toContain(
        "const ____VERTER_COMP_OPTION__COMPILED = defineComponent({})"
      );
    });

    test("script setup", () => {
      const result = process(
        `<script setup>defineProps({test: String})</script>`
      );
      expect(result.content).toContain(
        "const ____VERTER_COMP_OPTION__COMPILED = "
      );
    });

    test("script setup ts", () => {
      const result = process(
        `<script setup lang="ts" generic="T">defineProps<{test: T}>()</script>`
      );
      expect(result.content).toContain(
        "const ____VERTER_COMP_OPTION__COMPILED = defineComponent"
      );
    });

    test("export options", () => {
      const result = process(`<script>export default {}</script>`);

      expect(result.content).toContain(
        "export const ____VERTER_COMP_OPTION__ = __VERTER__defineComponent(____VERTER_COMP_OPTION__COMPILED)"
      );
    });
    test("export options ts", () => {
      const result = process(`<script lang="ts">export default {}</script>`);
      expect(result.content).toContain(
        "export const ____VERTER_COMP_OPTION__ = {}"
      );
    });
  });

  describe("imports", () => {
    it("keep", () => {
      const result = process(
        `<script lang="ts">import { defineComponent } from 'vue'; import 'test/test'; import type {Test} from 'xx';</script>`
      );
      expect(result.content).toContain(
        `import { defineComponent } from 'vue';import 'test/test';import type {Test} from 'xx';`
      );
    });

    it.skip("add compiled", () => {
      const result = process(`<script setup lang="ts"></script>`);
      expect(result.content).toContain(
        `import { defineComponent as _defineComponent } from 'vue'`
      );
    });

    it.skip("add generated", () => {
      const result = process(`<script setup lang="ts"></script>`);
      expect(result.content).toContain(
        `import { defineComponent as _defineComponent } from 'vue'`
      );
      expect(result.content).toContain(
        `import { defineComponent as __VERTER__defineComponent, type DefineComponent as __VERTER__DefineComponent } from 'vue';`
      );
    });

    it("move to top", () => {
      const result = process(
        '<script setup>import {ref} from "vue"; const a = ref(); import "test";</script>'
      );
      expect(result.content).toContain(
        `import {ref} from "vue";import "test";`
      );
    });
  });

  describe("setup", () => {
    describe("ts", () => {
      it("simple", () => {
        const result = process(
          `<script setup lang="ts">import { ref, Ref } from 'vue';\nconst a = ref();</script><template><div>test</div></template>`
        );
        expect(result.content).toContain(`const a = ref();`);
        expect(result.content).toContain(
          "export function ___VERTER___BindingContext() {"
        );

        expectFindStringWithMap("const a = ref();", result);
        expectFindStringWithMap(`import { ref, Ref } from 'vue';`, result);
      });

      it("async", () => {
        const result = process(
          `<script setup lang="ts">import { ref, Ref } from 'vue';\nconst a = ref();\nawait Promise.resolve()</script><template><div>test</div></template>`
        );

        expect(result.content).toContain(`const a = ref();`);
        expect(result.content).toContain(`await Promise.resolve()`);
        expect(result.content).toContain(
          "export async function ___VERTER___BindingContext() {"
        );

        expectFindStringWithMap("const a = ref();", result);
        expectFindStringWithMap(`await Promise.resolve()`, result);
      });

      it("generic", () => {
        const result = process(
          `<script setup lang="ts" generic="T">import { ref, Ref } from 'vue';\nconst a = ref() as Ref<T>;</script><template><div>test</div></template>`
        );

        expect(result.content).toContain(`const a = ref() as Ref<T>;`);
        expect(result.content).toContain(
          "export function ___VERTER___BindingContext<T>() {"
        );

        expectFindStringWithMap("const a = ref() as Ref<T>;", result);
      });

      it("async generic", () => {
        const result = process(
          `<script setup lang="ts" generic="T">import { ref, Ref } from 'vue';\nconst a = ref() as Ref<T>;\nawait Promise.resolve()</script><template><div>test</div></template>`
        );

        expect(result.content).toContain(`const a = ref() as Ref<T>;`);
        expect(result.content).toContain(
          "export async function ___VERTER___BindingContext<T>() {"
        );

        expectFindStringWithMap("const a = ref() as Ref<T>;", result);
      });

      it("multi script", () => {
        const result = process(
          `<script setup lang="ts">import { ref, Ref } from 'vue';\nconst a = ref();\nawait Promise.resolve()</script><script lang="ts">export default { specialOptions: true }</script><template><div>test</div></template>`
        );

        expect(result.content).toMatchInlineSnapshot(`
          "import { ref, Ref } from 'vue';
          export async function ___VERTER___BindingContext() {

          const a = ref();
          await Promise.resolve()
          return {} as {ref: typeof ref, Ref: typeof Ref, a: typeof a
          }

          }
          const ___VERTER___default = { specialOptions: true }"
        `);

        expect(result.content).toContain(`const a = ref();`);
        expect(result.content).toContain(
          "export async function ___VERTER___BindingContext() {"
        );
        expect(result.content).toContain(
          "const ___VERTER___default = { specialOptions: true }"
        );
        expect(result.content).not.toContain(
          "export default { specialOptions: true }"
        );

        expectFindStringWithMap("const a = ref();", result);
        expectFindStringWithMap(`import { ref, Ref } from 'vue';`, result);
      });

      describe("invalid syntax", () => {
        it("should handle invalid syntax", () => {
          const result = process(
            `<script setup lang="ts">import {ref} from 'vue';\nconst a = </script>`
          );

          expect(result.content).toContain(`const a = `);
          expect(result.content).toContain("ref: typeof ref, a: typeof a");
          expectFindStringWithMap("const a = ", result);
        });
      });
    });
    describe("js", () => {
      it("simple", () => {
        const result = process(
          `<script setup>import { ref } from 'vue';\nconst a = ref();</script><template><div>test</div></template>`
        );

        expect(result.content).toMatchInlineSnapshot(`
          "import { ref } from 'vue';/**
           * @returns {{ref: typeof ref, a: typeof a}} 
          */
          export function ___VERTER___BindingContext() {

          const a = ref();
          return {
          ref, a
          }

          }
          "
        `);

        expect(result.content).toContain(`const a = ref();`);
        expect(result.content).toContain(
          "function ___VERTER___BindingContext() {"
        );

        expect(result.content).not.toContain(`__VERTER__isDefineComponent`);

        expectFindStringWithMap("const a = ref();", result);
        expectFindStringWithMap(`import { ref } from 'vue';`, result);
      });

      it("async", () => {
        const result = process(
          `<script setup>import { ref } from 'vue';\nconst a = ref();\nawait Promise.resolve()</script><template><div>test</div></template>`
        );

        expect(result.content).toContain(`const a = ref();`);
        expect(result.content).toContain(`await Promise.resolve()`);
        expect(result.content).toContain(
          "async function ___VERTER___BindingContext() {"
        );

        expect(result.content).not.toContain(`__VERTER__isDefineComponent`);

        expectFindStringWithMap("const a = ref();", result);
        expectFindStringWithMap(`await Promise.resolve()`, result);
      });

      it("multi script", () => {
        const result = process(
          `<script setup>import { ref, Ref } from 'vue';\nconst a = ref();\nawait Promise.resolve()</script><script>export default { specialOptions: true }</script><template><div>test</div></template>`
        );

        expect(result.content).toContain(`const a = ref();`);
        expect(result.content).toContain(
          "export async function ___VERTER___BindingContext() {"
        );
        expect(result.content).toContain(
          "const ___VERTER___default = { specialOptions: true }"
        );
        expect(result.content).not.toContain(`__VERTER__isDefineComponent`);
        expect(result.content).not.toContain(
          "export default { specialOptions: true }"
        );

        expectFindStringWithMap("const a = ref();", result);
        expectFindStringWithMap(`import { ref, Ref } from 'vue';`, result);
      });

      it("export", () => {
        const result = process(
          `<script setup>const msg = '';</script><script>export const a = 1;</script>`
        );

        expect(result.content).toContain(`export const a = 1;`);
        expect(result.content).toContain(`const msg = '';`);

        expect(result.content).toContain(
          "@returns {{msg: typeof msg, a: typeof a}}"
        );

        expectFindStringWithMap("export const a = 1;", result);
        expectFindStringWithMap(`const msg = '';`, result);
      });
    });

    describe("invalid syntax", () => {
      it("should handle invalid syntax", () => {
        const result = process(
          `<script setup>import {ref} from 'vue';\nconst a = </script>`
        );

        expect(result.content).toContain(`const a = `);
        expect(result.content).toContain("ref: typeof ref, a: typeof a");
        expectFindStringWithMap("const a = ", result);
      });
    });
  });

  describe("options", () => {
    describe("ts", () => {
      it("simple", () => {
        const result = process(`<script lang="ts">export default {}</script>`);

        expect(result.content).toContain("const ___VERTER___default = {}");
        expect(result.content).toContain(
          "export function ___VERTER___BindingContext() { return {} }"
        );
      });

      it("generic", () => {
        const result = process(
          `<script options lang="ts" generic="T">export default { data(){ return { a: {} as T }}}</script>`
        );

        expect(result.content).toContain(
          "export function ___VERTER___BindingContext<T>() {"
        );
        expect(result.content).toContain(
          "return { data(){ return { a: {} as T }}}"
        );
      });
    });

    describe("js", () => {
      it("simple", () => {
        const result = process(`<script>export default {}</script>`);

        expect(result.content).toContain("const ___VERTER___default = {}");
        expect(result.content).toContain(
          "export function ___VERTER___BindingContext() { return {} }"
        );
      });
    });
  });
});
