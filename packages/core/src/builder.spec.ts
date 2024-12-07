import { parse, MagicString, compileScript } from "@vue/compiler-sfc";
import { createBuilder } from "./builder.js";
import { parse as babelParse } from "@babel/parser";
import fs from "fs";
import remapping from "@ampproject/remapping";

describe("builder", () => {
  it("test", () => {
    const builder = createBuilder({});

    const res = builder.process(
      "test.vue",
      `<script setup lang="ts" generic="T extends string">
const model = defineModel<{ foo: T }>();
const test = defineProps<{ type: T}>();
</script>
<template>
  <span>1</span>
</template>
    `
    );

    expect(res).toMatchInlineSnapshot(`{}`);
  });

  it("another", () => {
    const builder = createBuilder({});

    const res = builder.process(
      "test.vue",
      `
      <script setup lang="ts" generic="T">
import { computed } from "vue";

const props = defineProps<{
  items: T[];
  getKey: (item: T) => string | number;
  getLabel: (item: T) => string;
}>();

const orderedItems = computed<T[]>(() =>
  props.items.sort((item1, item2) =>
    props.getLabel(item1).localeCompare(props.getLabel(item2))
  )
);

function getItemAtIndex(index: number): T | undefined {
  if (index < 0 || index >= orderedItems.value.length) {
    return undefined;
  }
  return orderedItems.value[index];
}

defineExpose({ getItemAtIndex });
</script>

<template>
  <ol>
    <li v-for="item in orderedItems" :key="getKey(item)">
      {{ getLabel(item) }}
    </li>
  </ol>
</template>
`
    );
    expect(res).toMatchInlineSnapshot(`{}`);
  });
  it("multiple scripts", () => {
    const code = `<script setup lang="ts">
    const props = defineProps<{
      id?: string;
      label?: string;
    
      icon?: string;
      required?: string | boolean;
    
      noBorder?: string | boolean;
    }>();
    
    </script>
    <script lang="ts">
    const xx = 1
    export default {
      inheritAttrs: false,
    };
    </script>
    <template>
      <span>1</span>
      <input v-model="msg" />
      <span v-cloak>{{ msg }}</span>
      <my-comp test="1"></my-comp>
      <my-comp v-for="i in 5" :key="i" :test="i"></my-comp>
    </template>
    `;
    const builder = createBuilder({});
    const res = builder.process("test.vue", code);
    expect(res).toMatchInlineSnapshot(`{}`);
  });

  it("simple test", () => {});

  it("babel parser", () => {
    // Wrap your type in an expression (TypeScript type assertion)
    const code = `type MyType<T extends { item: string }, X> = {}`;

    // Parse the expression
    const ast = babelParse(code, {
      plugins: ["typescript"],
    });

    // @ts-expect-error
    const type = ast.program.body[0].typeParameters;

    expect(code.slice(type.start, type.end)).toMatchInlineSnapshot(
      `"<T extends { item: string }, X>"`
    );

    expect(1).toBe(1);

    // const parsed = parseExpression(
    //   "type MyType<T extends { item: string }, X> = {}",
    //   {
    //     sourceType: "module",
    //     plugins: ["typescript"],
    //   }
    // );

    // console.log(parsed);
    // expect(parsed).toMatchInlineSnapshot();
  });

  test("parse with sourcemaps", () => {
    const codeStr = `<script setup lang="ts">
      // defineProps({ foo: String })
      defineProps<{ foo: string}>()
      const test = 'hello'
</script>`;
    const { descriptor } = parse(codeStr, {
      sourceMap: true,
    });

    const compiled = compileScript(descriptor, {
      id: descriptor.filename,
      sourceMap: true,
    });

    const s = new MagicString(compiled.content, {
      filename: descriptor.filename,
    });
    s.replace("hello", "Welcome pikax");

    const decoded = s.generateDecodedMap({ hires: true });
    const finalStr = s.toString();
    const r = remapping([decoded as any, compiled.map], () => null);

    const finalMap = r.toString();

    // fs.writeFileSync("D:/Downloads/sourcemap-test/sourcemap-example.js", finalStr + '\n//# sourceMappingURL=sourcemap-example.js.map', 'utf-8')
    fs.writeFileSync(
      "D:/Downloads/sourcemap-test/sourcemap-example.js",
      finalStr,
      "utf-8"
    );
    fs.writeFileSync(
      "D:/Downloads/sourcemap-test/sourcemap-example.js.map",
      finalMap.toString(),
      "utf-8"
    );

    // console.log('--')
    // console.log(generated)
  });
});
