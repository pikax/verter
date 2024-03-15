import { parse, MagicString, compileScript } from "@vue/compiler-sfc";
import { createBuilder } from "./builder.js";
import { parseExpression, parse as babelParse } from "@babel/parser";
import { SourceMapConsumer, SourceMapGenerator, SourceNode } from "source-map-js";
import fs from 'fs'
import remapping from '@ampproject/remapping'


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

    expect(res).toMatchInlineSnapshot(`
      "
      import { useModel as _useModel, mergeModels as _mergeModels, defineComponent as _defineComponent } from 'vue'
      import { defineComponent } from 'vue';
      import { DeclareComponent } from 'vue';
      import { DeclareEmits, EmitsToProps } from 'vue';



      const __options = defineComponent(({
        __name: 'test',
        props: /*#__PURE__*/_mergeModels({
          type: { type: null, required: true }
        }, {
          "modelValue": { type: Object },
          "modelModifiers": {},
        }),
        emits: ["update:modelValue"],
        setup<T extends string>(__props: any, { expose: __expose }) {
        __expose();

      const model = _useModel<{ foo: T }>(__props, "modelValue");
      const test = __props;

      const __returned__ = { model, test }
      Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
      return __returned__
      }

      }));
      type Type__options = typeof __options;


      type __DATA__ = {};
      type __EMITS__ = {};
      type __SLOTS__ = {};

      type __COMP__ = DeclareComponent<{ new<T extends string>(): { $props: { modelValue: { foo: T } } & { type: T} & EmitsToProps<DeclareEmits<{ 'update:modelValue': [{ foo: T }] }>>, $emit: DeclareEmits<{ 'update:modelValue': [{ foo: T }] }> , $children: {}  } }, __DATA__, __EMITS__, __SLOTS__, Type__options>



      export default __options as unknown as __COMP__;
              "
    `);
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
    expect(res).toMatchInlineSnapshot(`
      "
      import { defineComponent } from 'vue';
      import { computed } from 'vue';
      import { defineComponent as _defineComponent } from 'vue'
      import { DeclareComponent } from 'vue';



      const __options = defineComponent(({
        __name: 'test',
        props: {
          items: { type: Array, required: true },
          getKey: { type: Function, required: true },
          getLabel: { type: Function, required: true }
        },
        setup<T>(__props: any, { expose: __expose }) {

      const props = __props;

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

      __expose({ getItemAtIndex });

      const __returned__ = { props, orderedItems, getItemAtIndex }
      Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
      return __returned__
      }

      }));
      type Type__options = typeof __options;


      // expose

              function __exposeResolver<T>() {
                
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

                return { getItemAtIndex };
              }
            

      type __DATA__ = {};
      type __EMITS__ = {};
      type __SLOTS__ = {};

      type __COMP__ = DeclareComponent<{ new<T>(): { $props: {
        items: T[];
        getKey: (item: T) => string | number;
        getLabel: (item: T) => string;
      }, $emit: {} , $children: {}, $data: ReturnType<typeof __exposeResolver<T>>  } }, __DATA__, __EMITS__, __SLOTS__, Type__options>



      export default __options as unknown as __COMP__;
              "
    `);
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
    expect(res).toMatchInlineSnapshot(`
      "
      import { defineComponent } from 'vue';
      import { defineComponent as _defineComponent } from 'vue'
      import { DeclareComponent } from 'vue';
      import { DeclareEmits, EmitsToProps } from 'vue';



      const __options = defineComponent(({
        ...__default__,
        __name: 'test',
        props: {
          id: { type: String, required: false },
          label: { type: String, required: false },
          icon: { type: String, required: false },
          required: { type: [String, Boolean], required: false },
          noBorder: { type: [String, Boolean], required: false }
        },
        setup(__props: any, { expose: __expose }) {
        __expose();

          const props = __props;
          
          
      const __returned__ = { xx, props }
      Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
      return __returned__
      }

      }));
      type Type__options = typeof __options;
      const __props = defineProps<{
            id?: string;
            label?: string;
          
            icon?: string;
            required?: string | boolean;
          
            noBorder?: string | boolean;
          }>();
      type Type__props = typeof __props;;


      // expose


      type __PROPS__ = Type__props & EmitsToProps<{}>;
      type __DATA__ = {};
      type __EMITS__ = {};
      type __SLOTS__ = {};

      type __COMP__ = DeclareComponent<__PROPS__, __DATA__, __EMITS__, __SLOTS__,  "setup" extends keyof Type__options ? Type__options & { setup: () => {} } : Type__options >



      export default __options as unknown as __COMP__;
              "
    `);
  });

  it('simple test', () => {

  })

  it("babel parser", () => {
    // Wrap your type in an expression (TypeScript type assertion)
    const code = `type MyType<T extends { item: string }, X> = {}`;

    // Parse the expression
    const ast = babelParse(code, {
      plugins: ["typescript"],
    });

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


  test.only('parse with sourcemaps', () => {
    const codeStr = `<script setup lang="ts">
      // defineProps({ foo: String })
      defineProps<{ foo: string}>()
      const test = 'hello'
</script>`
    const { descriptor } = parse(codeStr, {
      sourceMap: true
    })

    const compiled = compileScript(descriptor, {
      id: descriptor.filename,
      sourceMap: true,
    })



    const s = new MagicString(compiled.content, {
      filename: descriptor.filename
    })
    s.replace('hello', 'Welcome pikax');


    const decoded = s.generateDecodedMap({ hires: true })
    const finalStr = s.toString()
    const r = remapping([decoded, compiled.map], () => null)

    const finalMap = r.toString()

    fs.writeFileSync("D:/Downloads/sourcemap-test/sourcemap-example.js", finalStr + '\n//# sourceMappingURL=sourcemap-example.js.map', 'utf-8')
    fs.writeFileSync("D:/Downloads/sourcemap-test/sourcemap-example.js.map", finalMap.toString(), 'utf-8')

    // console.log('--')
    // console.log(generated)
  })
});


