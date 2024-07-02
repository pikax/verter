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
        filename: "test.vue.options." + (lang?.replace("js", "ts") || "js"),
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

        expect(result.content).toContain(
          `export const ___VERTER___default = ___VERTER___defineComponent({});`
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

        expect(result.content).toContain(`const a = ref();`);
        expect(result.content).toContain(
          "export async function ___VERTER___BindingContext() {"
        );
        expect(result.content).toContain(
          "export const ___VERTER___default = ___VERTER___defineComponent({ specialOptions: true })"
        );
        expect(result.content).not.toContain(
          "export default { specialOptions: true }"
        );

        expectFindStringWithMap("const a = ref();", result);
        expectFindStringWithMap(`import { ref, Ref } from 'vue';`, result);
      });

      describe("return BindingContext", () => {
        describe("props", () => {
          it("not have props", () => {
            const result = process(
              `<script setup lang='ts'>const test = { a: 1, b: 1}</script>`
            );

            expect(result.content).toContain("{test: typeof test\n}");
            expect(result.content).not.toContain("} & typeof");
          });

          it("should have props variable", () => {
            const result = process(
              `<script setup lang='ts'>const bespokProps = defineProps({});\nconst test = { a: 1, b: 1}</script>`
            );

            expect(result.content).toContain("} & typeof bespokProps");

            expect(result.content).toContain(
              "{bespokProps: typeof bespokProps, test: typeof test\n}"
            );
          });

          it("should add implicitly add a variable to props", () => {
            const result = process(
              `<script setup lang='ts'>defineProps({});\nconst test = { a: 1, b: 1}</script>`
            );

            expect(result.content).toContain("{test: typeof test\n}");
            expect(result.content).toContain("} & typeof ___VERTER___props");
          });
        });

        describe("emits", () => {
          it("not have emits", () => {
            const result = process(
              `<script setup lang='ts'>const test = { a: 1, b: 1}</script>`
            );

            expect(result.content).toContain("{test: typeof test\n}");
            expect(result.content).not.toContain("} & typeof");
          });

          it("should have emits variable", () => {
            const result = process(
              `<script setup lang='ts'>const bespokeEmits = defineEmits([]);\nconst test = { a: 1, b: 1}</script>`
            );

            expect(result.content).toContain(
              "{bespokeEmits: typeof bespokeEmits, test: typeof test\n}"
            );
            expect(result.content).toContain(
              "} & { $emit: typeof bespokeEmits }"
            );
          });

          it("should add implicitly add a variable to emits", () => {
            const result = process(
              `<script setup lang='ts'>defineEmits([]);\nconst test = { a: 1, b: 1}</script>`
            );

            expect(result.content).toContain("{test: typeof test\n}");
            expect(result.content).toContain(
              "} & { $emit: typeof ___VERTER___emits }"
            );
          });
        });

        describe.todo("slots", () => {
          it.todo("not have slots", () => {});

          it.todo("should have slots variable", () => {});

          it.todo("should add implicitly add a variable to slots", () => {});
        });

        describe.todo("expose", () => {
          it.todo("TODO");
        });

        describe.todo("model", () => {
          it.todo("TODO");
        });
      });

      describe("identifiers", () => {
        it("object properties", () => {
          const result = process(
            `<script setup lang='ts'>const test = { a: 1, b: 1}</script>`
          );

          expect(result.content).toContain("{test: typeof test\n");
        });
        it("function params", () => {
          const result = process(
            `<script setup lang='ts'>function test(a: 1, b: 1) {}</script>`
          );

          expect(result.content).toContain("{test: typeof test\n");
        });
        it("function typed params", () => {
          const result = process(
            `<script setup lang='ts'>function test(a: number, b: string): number | undefined {}</script>`
          );

          expect(result.content).toContain("{test: typeof test\n");
        });

        it("defineProps", () => {
          const result = process(
            `<script setup lang='ts'>defineProps({ a: String })</script>`
          );
          expect(result.content).toContain("{\n} & typeof ___VERTER___props");
        });

        it("defineProps with default", () => {
          const result = process(
            `<script setup lang='ts'>withDefaults(defineProps({ a: String }), {a: '1'})</script>`
          );
          expect(result.content).toContain("{\n} & typeof ___VERTER___props");
        });

        it("defineProps with default + variable", () => {
          const result = process(
            `<script setup lang='ts'>const props = withDefaults(defineProps({ a: String }), {a: '1'})</script>`
          );
          expect(result.content).toContain("{props: typeof props\n}");

          expect(result.content).toContain("} & typeof props");
        });

        it("props with variable assigned", () => {
          const result = process(
            `<script setup lang='ts'>const props = defineProps({ a: String });</script>`
          );
          expect(result.content).toContain("{props: typeof props\n}");
          expect(result.content).toContain("} & typeof props");
        });

        test("generic", () => {
          const result = process(
            `<script setup lang="ts" generic="T">const a = ref() as Ref<T>;\nfunction test(i: number): T {}</script>`
          );

          expect(result.content).toContain("{a: typeof a, test: typeof test\n");
        });

        test("function body", () => {
          const result = process(
            `<script setup lang="ts">const a = ref();\nfunction test(index) { if(index) {};  return a.value; }</script>`
          );

          expect(result.content).toContain("{a: typeof a, test: typeof test\n");
        });

        test("if statement", () => {
          const result = process(
            `<script setup lang="ts">if(true) { let a = ref() }</script>`
          );

          expect(result.content).toContain("as {\n} ");
        });
        test("while", () => {
          const result = process(
            `<script setup lang="ts">while(true) { let a = ref() }</script>`
          );
          expect(result.content).toContain("as {\n} ");
        });
        test("for", () => {
          const result = process(
            `<script setup lang="ts">for(let i = 0; i < 10; i++) { let a = ref() }</script>`
          );
          expect(result.content).toContain("as {\n} ");
        });

        test("only expose used bindings on the template", () => {
          const result = process(
            `<script setup lang="ts">const a = ref(); const b = ref();</script><template>{{a}}</template>`
          );
          expect(result.content).toContain("{a: typeof a\n}");
          // this should be FULL_CONTEXT
          expect(result.content).toContain("{a: typeof a, b: typeof b\n}");
        });
      });

      describe("invalid syntax", () => {
        it("should handle invalid syntax", () => {
          const result = process(
            `<script setup lang="ts">import {ref} from 'vue';\nconst a = </script>`
          );

          expect(result.content).toContain(`const a = `);
          expect(result.content).toContain("a: typeof a");
          expectFindStringWithMap("const a = ", result);
        });
      });
    });
    describe("js", () => {
      it("simple", () => {
        const result = process(
          `<script setup>import { ref } from 'vue';\nconst a = ref();</script><template><div>test</div></template>`
        );

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
          "export const ___VERTER___default = ___VERTER___defineComponent({ specialOptions: true })"
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
        expect(result.content).toContain("a: typeof a");
        expectFindStringWithMap("const a = ", result);
      });
    });
  });

  describe("options", () => {
    describe("ts", () => {
      it("simple", () => {
        const result = process(`<script lang="ts">export default {}</script>`);

        expect(result.content).toContain(
          "export const ___VERTER___default = ___VERTER___defineComponent({})"
        );
        expect(result.content).toContain(
          "export function ___VERTER___BindingContext() { return /*##___VERTER_BINDING_RETURN___##*/{}/*##/___VERTER_BINDING_RETURN___##*/ }"
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
          "return ___VERTER___defineComponent({ data(){ return { a: {} as T }}})"
        );
      });
    });

    describe("js", () => {
      it("simple", () => {
        const result = process(`<script>export default {}</script>`);

        expect(result.content).toContain(
          'import { defineComponent as ___VERTER___defineComponent } from "vue";'
        );
        expect(result.content).toContain(
          "const ___VERTER___default = ___VERTER___defineComponent({})"
        );
        expect(result.content).toContain(
          "export function ___VERTER___BindingContext() { return /*##___VERTER_BINDING_RETURN___##*/{}/*##/___VERTER_BINDING_RETURN___##*/ }"
        );
      });

      it("empty", () => {
        const result = process(`<template><div></div></template>`);

        expect(result.content).toContain(
          'import { defineComponent as ___VERTER___defineComponent } from "vue";'
        );
        expect(result.content).toContain(
          "const ___VERTER___default = ___VERTER___defineComponent({})"
        );
        expect(result.content).toContain(
          "export function ___VERTER___BindingContext() { return /*##___VERTER_BINDING_RETURN___##*/{}/*##/___VERTER_BINDING_RETURN___##*/ }"
        );
      });

      it("with defineComponent", () => {
        const result = process(
          `<script>export default defineComponent({name: 'test'})</script>`
        );
        expect(result.content).toContain(
          "export const ___VERTER___default = defineComponent({name: 'test'})"
        );
      });

      it("not change if is a variable", () => {
        const result = process(
          `<script>
const Comp = defineComponent({name: 'test'})
export default Comp;</script>`
        );
        expect(result.content).toContain(
          "const Comp = defineComponent({name: 'test'})"
        );

        expect(result.content).toContain(
          "export const ___VERTER___default = Comp"
        );
      });
    });
  });
});
