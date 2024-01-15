import { createBuilder } from "./builder.js";
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
      import { defineComponent } from 'vue';



      const __options = defineComponent(({
        __name: 'test',
        props: /*#__PURE__*/_mergeModels({
          type: { type: null, required: true }
        }, {
          "modelValue": { type: Object },
          "modelModifiers": {},
        }),
        emits: ["update:modelValue"],
        setup(__props: any, { expose: __expose }) {
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


      export default __options as __COMP__;
              "
    `);
  });
});
