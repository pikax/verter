import { MagicString } from "@vue/compiler-sfc";
import { createBuilder } from "../builder";
import { mergeFull } from "./full";
import fs from "node:fs";

describe("Mergers Full", () => {
  function testSourceMaps({
    content,
    map,
  }: {
    content: string;
    map: ReturnType<MagicString["generateMap"]>;
  }) {
    fs.writeFileSync(
      "D:/Downloads/sourcemap-test/sourcemap-example.js",
      content,
      "utf-8"
    );
    fs.writeFileSync(
      "D:/Downloads/sourcemap-test/sourcemap-example.js.map",
      JSON.stringify(map),
      "utf-8"
    );
  }

  const builder = createBuilder();
  function process(content: string, fileName = "test.vue") {
    const { locations, context } = builder.preProcess(fileName, content);
    return mergeFull(locations, context);
  }

  it("should work options API", () => {
    const source = `<script>
        export default {
            data(){
                return { test: 'hello' }
            }
        }
        </script>
        <template>
            <div :key="test">test</div>
        </template>`;

    const p = process(source);

    expect(p.source).toBe(source);

    expect(p.content).toMatchInlineSnapshot(`
      "import { defineComponent as ___VERTER_defineComponent } from 'vue';

      function ___VERTER_COMPONENT__() {


              const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({
                  data(){
                      return { test: 'hello' }
                  }
              })
              function ___VERTER__TEMPLATE_RENDER() {
      <>
                  <div key={___VERTER__ctx.test}>{ "test" }</div>
              
      </>}
      ___VERTER__TEMPLATE_RENDER();
      const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
      const ___VERTER__comp = { 
                  ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                }
      return ___VERTER_COMP___;
      }

              const __VERTER__RESULT = ___VERTER_COMPONENT__();
      export default __VERTER__RESULT;"
    `);

    p.map;
    testSourceMaps(p);
  });

  it("should work setup", () => {
    const source = `<script setup>
        const test = ref('hello')
        </script>
        <template>
            <div :key="test">test</div>
        </template>`;

    const p = process(source);

    expect(p.source).toBe(source);

    expect(p.content).toMatchInlineSnapshot(`
      "
      import { defineComponent as ___VERTER_defineComponent } from 'vue';

      function ___VERTER_COMPONENT__() {
      const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({
        __name: 'test',
        setup(__props, { expose: __expose }) {
        __expose();

              const test = ref('hello')
              
      const __returned__ = { test }
      Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
      return __returned__
      }

      })

              const test = ref('hello')
              function ___VERTER__TEMPLATE_RENDER() {
      <>
                  <div key={___VERTER__ctx.test}>{ "test" }</div>
              
      </>}
      ___VERTER__TEMPLATE_RENDER();
      const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
      const ___VERTER__comp = { 
                  ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                }
      return ___VERTER_COMP___;
      }

              const __VERTER__RESULT = ___VERTER_COMPONENT__();
      export default __VERTER__RESULT;"
    `);

    testSourceMaps(p);
  });

  describe("test", () => {
    describe("script", () => {
      it("should generate default option", () => {
        const source = `<script></script>`;

        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent as ___VERTER_defineComponent } from 'vue';

          function ___VERTER_COMPONENT__() {
          const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({})
          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);
      });

      it("should append defineComponent", () => {
        const source = `<script>
                export default {}
                </script>`;

        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent as ___VERTER_defineComponent } from 'vue';

          function ___VERTER_COMPONENT__() {


                          const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({})
                          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);
      });

      it("should append defineComponent with options", () => {
        const source = `<script>
                export default {
                    name: 'test',
    
                    data(){ 
                        return {
                            foo: 'foo'
                        }
                    }
                }
                </script>`;

        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent as ___VERTER_defineComponent } from 'vue';

          function ___VERTER_COMPONENT__() {


                          const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({
                              name: 'test',
              
                              data(){ 
                                  return {
                                      foo: 'foo'
                                  }
                              }
                          })
                          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);
        testSourceMaps(p);
      });

      it("should keep defineComponent", () => {
        const source = `<script>
                export default defineComponent({
                    name: 'test',
    
                    data(){ 
                        return {
                            foo: 'foo'
                        }
                    }
                })
                </script>`;

        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "
          function ___VERTER_COMPONENT__() {


                          const ____VERTER_COMP_OPTION__ = defineComponent({
                              name: 'test',
              
                              data(){ 
                                  return {
                                      foo: 'foo'
                                  }
                              }
                          })
                          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);
      });

      it("should keep customeDefineComponent", () => {
        const source = `<script>
                export default customDefineComponent({
                    name: 'test',
    
                    data(){ 
                        return {
                            foo: 'foo'
                        }
                    }
                })
                </script>`;

        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "
          function ___VERTER_COMPONENT__() {


                          const ____VERTER_COMP_OPTION__ = customDefineComponent({
                              name: 'test',
              
                              data(){ 
                                  return {
                                      foo: 'foo'
                                  }
                              }
                          })
                          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);
      });

      it("should keep imports unaltered", () => {
        const source = `<script>
        // import stuff
        import { defineComponent } from 'vue'

        export default defineComponent({
            name: 'test',

            data(){ 
                return {
                    foo: 'foo'
                }
            }
        })
        </script>`;

        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent } from 'vue'
          function ___VERTER_COMPONENT__() {


                  // import stuff
                  

                  const ____VERTER_COMP_OPTION__ = defineComponent({
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo'
                          }
                      }
                  })
                  const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);

        testSourceMaps(p);
      });

      it("should handle generated imports + user imports", () => {
        const source = `<script>
        // import stuff
        import { ref } from 'vue'

        export default {
            name: 'test',

            data(){ 
                return {
                    foo: 'foo'
                }
            }
        }
        </script>`;

        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent as ___VERTER_defineComponent } from 'vue';
          import { ref } from 'vue'
          function ___VERTER_COMPONENT__() {


                  // import stuff
                  

                  const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo'
                          }
                      }
                  })
                  const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);
        testSourceMaps(p);
      });

      it("should handle generic", () => {
        const source = `<script lang="ts" generic="T extends string = 'foo'">
        import { ref } from 'vue'

        export default {
            name: 'test',

            data(){ 
                return {
                    foo: 'foo' as T
                }
            }
        }
        </script>`;
        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent as ___VERTER_defineComponent } from 'vue';
          import { ref } from 'vue'
          function ___VERTER_COMPONENT__<T extends string = 'foo'>() {


                  

                  const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo' as T
                          }
                      }
                  })
                  const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }
          const __VERTER__RESULT = ___VERTER_COMPONENT__<any>();
          type __VERTER_RESULT_TYPE__<_VUE_TS__T extends string = 'foo'> = (ReturnType<typeof ___VERTER_COMPONENT__<_VUE_TS__T>> extends infer Comp ? Pick<Comp, keyof Comp> : never) & { new<T extends string = 'foo'>(): { $props: { 
                  /* props info here */

                 }} };
          export default __VERTER__RESULT as unknown as __VERTER_RESULT_TYPE__;"
        `);
        testSourceMaps(p);
      });

      it("should handle generic on the template", () => {
        const source = `<script lang="ts" generic="T extends string = 'foo'">
        import { ref } from 'vue'

        export default {
            name: 'test',

            data(){ 
                return {
                    foo: 'foo' as T
                }
            }
        }
        </script>
        <template>
            <div>
                {{ foo + 'aa' as T}}
            </div>
        </template>
        `;
        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent as ___VERTER_defineComponent } from 'vue';
          import { ref } from 'vue'
          function ___VERTER_COMPONENT__<T extends string = 'foo'>() {


                  

                  const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo' as T
                          }
                      }
                  })
                  function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div>
                          { ___VERTER__ctx.foo + 'aa' as T}
                      </div>
                  
          </>}
          ___VERTER__TEMPLATE_RENDER();
          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }

                  
                  const __VERTER__RESULT = ___VERTER_COMPONENT__<any>();
          type __VERTER_RESULT_TYPE__<_VUE_TS__T extends string = 'foo'> = (ReturnType<typeof ___VERTER_COMPONENT__<_VUE_TS__T>> extends infer Comp ? Pick<Comp, keyof Comp> : never) & { new<T extends string = 'foo'>(): { $props: { 
                  /* props info here */

                 }} };
          export default __VERTER__RESULT as unknown as __VERTER_RESULT_TYPE__;"
        `);
        testSourceMaps(p);
      });

      it("should handle more complex generic on the template", () => {
        const source = `<script lang="ts" generic="T extends string & { supa?: () => number } = 'foo'">
        import { ref } from 'vue'

        export default {
            name: 'test',

            data(){ 
                return {
                    foo: 'foo' as T
                }
            }
        }
        </script>
        <template>
            <div>
                {{ (foo + 'aa' as typeof foo).supa?.()}}
            </div>
        </template>
        `;
        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import { defineComponent as ___VERTER_defineComponent } from 'vue';
          import { ref } from 'vue'
          function ___VERTER_COMPONENT__<T extends string & { supa?: () => number } = 'foo'>() {


                  

                  const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo' as T
                          }
                      }
                  })
                  function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div>
                          { (___VERTER__ctx.foo + 'aa' as typeof ___VERTER__ctx.foo).supa?.()}
                      </div>
                  
          </>}
          ___VERTER__TEMPLATE_RENDER();
          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                    }
          return ___VERTER_COMP___;
          }

                  
                  const __VERTER__RESULT = ___VERTER_COMPONENT__<any>();
          type __VERTER_RESULT_TYPE__<_VUE_TS__T extends string & { supa?: () => number } = 'foo'> = (ReturnType<typeof ___VERTER_COMPONENT__<_VUE_TS__T>> extends infer Comp ? Pick<Comp, keyof Comp> : never) & { new<T extends string & { supa?: () => number } = 'foo'>(): { $props: { 
                  /* props info here */

                 }} };
          export default __VERTER__RESULT as unknown as __VERTER_RESULT_TYPE__;"
        `);
      });

      it("support withDefaults", () => {
        const source = `<script setup lang="ts">
        withDefaults(defineProps<{ foo?: string }>(), {
          foo: "test",
        });
        </script>
        <template>
          <span>1</span>
        </template>
        `;
        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import 'vue/jsx';

          // patching elements
          declare global {
            namespace JSX {
              export interface IntrinsicClassAttributes<T> {
                $children: T extends { $slots: infer S } ? { default?: never } & { [K in keyof S]: S[K] extends (...args: infer Args) => any ? (...args: Args) => JSX.Element : () => JSX.Element } : { default?: never }
              }
            }
          }
          declare module 'vue' {
            interface HTMLAttributes {
              $children?: {
                default: () => JSX.Element
              }
            }
          }
          import { defineComponent as _defineComponent } from 'vue'

          function ___VERTER_COMPONENT__() {
          const ____VERTER_COMP_OPTION__ = /*#__PURE__*/_defineComponent({
            __name: 'test',
            props: {
              foo: { default: "test" }
            },
            setup(__props: any, { expose: __expose }) {
            __expose();

                  
                  
          const __returned__ = {  }
          Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
          return __returned__
          }

          })

                  withDefaults(defineProps<{ foo?: string }>(), {
                    foo: "test",
                  });
                  function ___VERTER__TEMPLATE_RENDER() {
          <>
                    <span>{ "1" }</span>
                  
          </>}
          ___VERTER__TEMPLATE_RENDER();
          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER_PROPS___ = {
                        ......(withDefaults(defineProps<{ foo?: string }>(), {
                    foo: "test",
                  }))
                      }
          const ___VERTER__ctx = { ...({} as InstanceType<typeof ___VERTER_COMP___>),
          ...({} as ) }
          const ___VERTER__comp = { 
                      //...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
                      ...___VERTER__ctx
                    }
          ___VERTER__comp;
          ___VERTER__ctx;
          return ___VERTER_COMP___;
          }

                  
                  const __VERTER__RESULT = ___VERTER_COMPONENT__();
          export default __VERTER__RESULT;"
        `);
      });

      it("handle generic list", () => {
        const source = `<script setup lang="ts" generic="T">
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
        
        `;
        const p = process(source);
        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
          "import 'vue/jsx';

          // patching elements
          declare global {
            namespace JSX {
              export interface IntrinsicClassAttributes<T> {
                $children: T extends { $slots: infer S } ? { default?: never } & { [K in keyof S]: S[K] extends (...args: infer Args) => any ? (...args: Args) => JSX.Element : () => JSX.Element } : { default?: never }
              }
            }
          }
          declare module 'vue' {
            interface HTMLAttributes {
              $children?: {
                default: () => JSX.Element
              }
            }
          }
          import { defineComponent as _defineComponent } from 'vue'
          import { renderList as __VERTER__renderList, unref as __VERTER__unref } from 'vue';
          import { computed } from "vue";
          function ___VERTER_COMPONENT__<T>() {
          const ____VERTER_COMP_OPTION__ = /*#__PURE__*/_defineComponent({
            __name: 'test',
            props: {
              items: {},
              getKey: { type: Function },
              getLabel: { type: Function }
            },
            setup(__props: any, { expose: __expose }) {

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

          })

                  
                  
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
                  function ___VERTER__TEMPLATE_RENDER() {
          <>
                    <ol>
                      {__VERTER__renderList(___VERTER__ctx.orderedItems,(item)=>{<li  key={___VERTER__ctx.getKey(item)}>
                        { ___VERTER__ctx.getLabel(item) }
                      </li>})}
                    </ol>
                  
          </>}
          ___VERTER__TEMPLATE_RENDER();
          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER__ctx = { ...({} as InstanceType<typeof ___VERTER_COMP___>),
          ...(defineProps<{
                    items: T[];
                    getKey: (item: T) => string | number;
                    getLabel: (item: T) => string;
                  }>()),
          computed: __VERTER__unref(computed),
          props: __VERTER__unref(props),
          orderedItems: __VERTER__unref(orderedItems),
          getItemAtIndex: __VERTER__unref(getItemAtIndex) }
          const ___VERTER__comp = { 
                      //...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
                      ...___VERTER__ctx
                    }
          ___VERTER__comp;
          ___VERTER__ctx;
          return ___VERTER_COMP___;
          }

                  
                  
                  
                  const __VERTER__RESULT = ___VERTER_COMPONENT__<any>();
          type __VERTER_RESULT_TYPE__<_VUE_TS__T = any> = (ReturnType<typeof ___VERTER_COMPONENT__<_VUE_TS__T>> extends infer Comp ? Pick<Comp, keyof Comp> : never) & { new<T extends _VUE_TS__T = _VUE_TS__T>(): { $props: { 
                  /* props info here */

                 }} };
          export default __VERTER__RESULT as unknown as __VERTER_RESULT_TYPE__;"
        `);

        testSourceMaps(p);
      });
    });
  });

  it("teet", () => {
    const source = `<script lang="ts">
    import { ref } from 'vue'
    
    
    const hh = 'hh';
    
    const eee= ref();
    
    </script> 
    <template>
    <div>
        <div aria-autocomplete='' :about="false"></div>
        <span ></span>
        <image></image>
        </div>
    </template>`;
    const p = process(source);
    expect(p.source).toBe(source);
    expect(p.content).toMatchInlineSnapshot(`
          "import 'vue/jsx';

          // patching elements
          declare global {
            namespace JSX {
              export interface IntrinsicClassAttributes<T> {
                $children: T extends { $slots: infer S } ? { default?: never } & { [K in keyof S]: S[K] extends (...args: infer Args) => any ? (...args: Args) => JSX.Element : () => JSX.Element } : { default?: never }
              }
            }
          }
          declare module 'vue' {
            interface HTMLAttributes {
              $children?: {
                default: () => JSX.Element
              }
            }
          }
          import { defineComponent as _defineComponent } from 'vue'
          import { renderList as __VERTER__renderList, unref as __VERTER__unref } from 'vue';
          import { computed } from "vue";
          function ___VERTER_COMPONENT__<T>() {
          const ____VERTER_COMP_OPTION__ = /*#__PURE__*/_defineComponent({
            __name: 'test',
            props: {
              items: {},
              getKey: { type: Function },
              getLabel: { type: Function }
            },
            setup(__props: any, { expose: __expose }) {

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

          })

                  
                  
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
                  function ___VERTER__TEMPLATE_RENDER() {
          <>
                    <ol>
                      {__VERTER__renderList(___VERTER__ctx.orderedItems,(item)=>{<li  key={___VERTER__ctx.getKey(item)}>
                        { ___VERTER__ctx.getLabel(item) }
                      </li>})}
                    </ol>
                  
          </>}
          ___VERTER__TEMPLATE_RENDER();
          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
          const ___VERTER_PROPS___ = {
                        ......(defineProps<{
                    items: T[];
                    getKey: (item: T) => string | number;
                    getLabel: (item: T) => string;
                  }>())
                      }
          const ___VERTER__ctx = { ...({} as Omit<InstanceType<typeof ___VERTER_COMP___>, "computed"|"props"|"orderedItems"|"getItemAtIndex">),
          ...({} as Omit<typeof ___VERTER_PROPS___, "computed"|"props"|"orderedItems"|"getItemAtIndex">),
          computed: __VERTER__unref(computed),
          props: __VERTER__unref(props),
          orderedItems: __VERTER__unref(orderedItems),
          getItemAtIndex: __VERTER__unref(getItemAtIndex) }
          const ___VERTER__comp = { 
                      //...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                      ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
                      ...___VERTER__ctx
                    }
          ___VERTER__comp;
          ___VERTER__ctx;
          return ___VERTER_COMP___;
          }

                  
                  
                  
                  const __VERTER__RESULT = ___VERTER_COMPONENT__<any>();
          type __VERTER_RESULT_TYPE__<_VUE_TS__T = any> = (ReturnType<typeof ___VERTER_COMPONENT__<_VUE_TS__T>> extends infer Comp ? Pick<Comp, keyof Comp> : never) & { new<T extends _VUE_TS__T = _VUE_TS__T>(): { $props: { 
                  /* props info here */

                 }} };
          export default __VERTER__RESULT as unknown as __VERTER_RESULT_TYPE__;"
        `);
  });

  it.skip("sss", () => {
    const source = `
        <template>
            <div :key="test">test</div>
        </template>
        <!--<script setup>
        const test = ref('hello')
        </script>-->
        <script>
        export const Supa = ''
        </script>
        <style>
        .test {

        }
        </style>`;

    const p = process(source);

    expect(p.source).toBe(source);

    expect(p.content).toMatchInlineSnapshot(`
          "
                  
                  
                  <script>
                  export const Supa = ''

                  export default {}
                  </script>
                  function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div key={___VERTER__ctx.test}>{ "test" }</div>
                  
          </>}"
        `);
    testSourceMaps(p);
  });

  it("supa test", () => {
    const source = `<script lang="ts" setup>
    import { ref } from "vue";
    import { isIOS } from "@/utils/browser";
    import { useMotionProperties, useSpring } from "@vueuse/motion";
    import { useDrag } from "@vueuse/gesture";
    
    const scrollElementEl = ref<HTMLElement | null>(null);
    
    if (!isIOS) {
      const { motionProperties } = useMotionProperties(scrollElementEl, {
        translateY: 0,
        willChange: "transform",
      });
    
      // Bind the motion properties to a spring reactive object.
      const { set } = useSpring(motionProperties);
    
      let lastY = 0;
      useDrag(
        ({ movement: [x, y], dragging, velocity }) => {
          const diff = y - lastY;
          if (!dragging) {
            set({
              translateY: 0,
            });
            return;
          }
          if (diff < 0) {
            if (!isBottom()) {
              lastY = y;
              return;
            }
          } else {
            if (!isTop()) {
              lastY = y;
              return;
            }
          }
          set({
            translateY: diff,
          });
        },
        {
          domTarget: scrollElementEl,
        },
      );
    }
    
    function isBottom() {
      const s = Math.floor(
        Math.abs(
          scrollElementEl.value!.scrollHeight -
            (scrollElementEl.value!.clientHeight +
              scrollElementEl.value!.scrollTop),
        ),
      );
      return s === 0;
    }
    function isTop() {
      return scrollElementEl.value!.scrollTop === 0;
    }
    
    function onTouchStart(e: Event) {
      if (isIOS) return;
      if (isTop()) {
        return;
      }
      if (isBottom()) {
        return;
      }
      e.stopImmediatePropagation();
    }
    </script>
    <template>
      <div class="flex-1 overflow-y-auto pt-9x" ref="scrollElementEl">
        <div
          :class="[isIOS ? 'h-force-scroll' : 'min-h-full']"
          @touchstart="onTouchStart"
        >
          <slot />
        </div>
      </div>
    </template>
    `;
    const p = process(source);

    expect(p.source).toBe(source);

    // expect(p.content).toMatchInlineSnapshot();
    testSourceMaps(p);
  });

  it("should remove comment code", () => {
    const source = `<template>
        <div :key="test">test</div>
    </template>
    <!--<script setup>
    const test = ref('hello')
    </script>-->`;

    const p = process(source);

    expect(p.source).toBe(source);
    expect(p.content).not.contain("<!--<script");
  });

  describe.skip("generic", () => {
    describe("error", () => {
      it("should not work if not ts", () => {
        const source = `<script setup lang="ts" generic="T">
                const test = 'test';
                </script>
                <style>
                .hello {
                    
                }
                </style>`;

        const p = process(source);

        expect(p.source).toBe(source);
        expect(p.content).toMatchInlineSnapshot(`
                  "/* @jsxImportSource vue */

                  /* verter:template-context start */
                  const ___VERTER_ctx = {
                  ...(new ____VERTER_COMP_OPTION__())
                  }
                  /* verter:template-context end */
                  <script setup generic="T">
                                  </script>"
                `);
      });
    });
  });

  it("should get an error generics are only supported in ts", () => {
    const source = `<script setup generic="T extends string">
        const model = defineModel<{ foo: T }>();
        const test = defineProps<{ type: T}>();
        </script>
        <script lang="ts">
         const SUPA = 'hello'
        </script>
        <template>
          <span>1</span>
        </template>`;

    const p = process(source);

    expect(p.source).toBe(source);
    expect(p.content).toMatchInlineSnapshot(`
      "/* @jsxImportSource vue */
      import 'vue/jsx';

      import { defineComponent as ___VERTER_defineComponent, defineComponent as ___VERTER_defineComponent } from 'vue';

      function ___VERTER_COMPONENT__<T extends string>() {
       )
      const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({})

              const model = defineModel<{ foo: T }>();
              const test = defineProps<{ type: T}>();
              function ___VERTER__TEMPLATE_RENDER() {
      <>
                <span>{ "1" }</span>
              
      </>}
      ___VERTER__TEMPLATE_RENDER();
      const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER__ctx = { ...({} as InstanceType<typeof ___VERTER_COMP___>) }
      const ___VERTER__comp = { 
                  //...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
                  ...___VERTER__ctx
                }
      ___VERTER__comp;
      ___VERTER__ctx;
      return ___VERTER_COMP___;
      }

              
      function ___VERTER_COMPONENT__() {

               const SUPA = 'hello'
              
      }

              const __VERTER__RESULT = ___VERTER_COMPONENT__();
      export default __VERTER__RESULT;"
    `);
  });

  it("should work setup + script", () => {
    const source = `<script setup lang="ts" generic="T extends string">
    import {} from 'vue';
    
        const model = defineModel<{ foo: T }>();
        const test = defineProps<{ type: T}>();
        </script>
        <script lang="ts">
         const SUPA = 'hello'
        </script>
        <template>
          <span>1</span>
        </template>`;

    const p = process(source);

    expect(p.source).toBe(source);
    expect(p.content).toMatchInlineSnapshot(`
      "import { useModel as _useModel, mergeModels as _mergeModels, defineComponent as _defineComponent } from 'vue'
      import {} from 'vue';
               const SUPA = 'hello'
              
      function ___VERTER_COMPONENT__<T extends string>() {
      const ____VERTER_COMP_OPTION__ = /*#__PURE__*/_defineComponent({
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
              
      const __returned__ = { SUPA, model, test }
      Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
      return __returned__
      }

      })

          
          
              const model = defineModel<{ foo: T }>();
              const test = defineProps<{ type: T}>();
              function ___VERTER__TEMPLATE_RENDER() {
      <>
                <span>{ "1" }</span>
              
      </>}const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
      const ___VERTER__comp = { 
                  ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } })
                }
      return ___VERTER_COMP___;
      }

              
              const __VERTER__RESULT = ___VERTER_COMPONENT__<any>();
      type __VERTER_RESULT_TYPE__<_VUE_TS__T extends string = any> = (ReturnType<typeof ___VERTER_COMPONENT__<_VUE_TS__T>> extends infer Comp ? Pick<Comp, keyof Comp> : never) & { new<T extends string = _VUE_TS__T>(): { $props: { 
              /* props info here */

             }} };
      export default __VERTER__RESULT as unknown as __VERTER_RESULT_TYPE__;"
    `);
    // testSourceMaps(p.context.script)
  });

  it.skip("should place types outside of the script function", () => {
    const source = `<script lang="ts" setup>
    export type MyTest = 1
    </script>`;

    const p = process(source);

    expect(p.source).toBe(source);

    expect(p.content).toMatchInlineSnapshot(`
      "/* @jsxImportSource vue */
      import 'vue/jsx';
      import { defineComponent as _defineComponent } from 'vue'
      export type MyTest = 1
      function ___VERTER_COMPONENT__() {
      const ____VERTER_COMP_OPTION__ = /*#__PURE__*/_defineComponent({
        __name: 'test',
        setup(__props, { expose: __expose }) {
        __expose();

          
      const __returned__ = {  }
      Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
      return __returned__
      }

      })

          
          const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER__ctx = { ...({} as InstanceType<typeof ___VERTER_COMP___>) }
      const ___VERTER__comp = { 
                  //...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
                  ...___VERTER__ctx
                }
      ___VERTER__comp;
      ___VERTER__ctx;
      return ___VERTER_COMP___;
      }
      const __VERTER__RESULT = ___VERTER_COMPONENT__();
      export default __VERTER__RESULT;"
    `);
  });

  it("it should unref on object deconstructor", () => {
    const source = `<script setup lang="ts">
    const { testX } = { test: 1 }
    </script>
    `;

    const p = process(source);

    expect(p.source).toBe(source);

    expect(p.content).toContain("unref(testX)");
    testSourceMaps(p);
  });

  it("it should unref on object deconstructor + alias", () => {
    const source = `<script setup lang="ts">
    const { notTheTestYouLookingFor: test } = { test: 1 }
    </script>
    `;

    const p = process(source);

    expect(p.source).toBe(source);

    expect(p.content).toContain("unref(test)");
    testSourceMaps(p);
  });

  it.skip("no script", () => {
    const source = `<template><span>1</span></template>`;

    const p = process(source);

    expect(p.content).toMatchInlineSnapshot(`
      "import 'vue/jsx';

      // patching elements
      declare global {
        namespace JSX {
          export interface IntrinsicClassAttributes<T> {
            $children: T extends { $slots: infer S } ? { default?: never } & { [K in keyof S]: S[K] extends (...args: infer Args) => any ? (...args: Args) => JSX.Element : () => JSX.Element } : { default?: never }
          }
        }
      }
      declare module 'vue' {
        interface HTMLAttributes {
          $children?: {
            default: () => JSX.Element
          }
        }
      }
      import { defineComponent as ___VERTER_defineComponent } from 'vue';


      function ___VERTER_COMPONENT__() {
      const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({})
      function ___VERTER__TEMPLATE_RENDER() {
      <><span>{ "1" }</span>
      </>}
      ___VERTER__TEMPLATE_RENDER();
      const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER__ctx = { ...({} as InstanceType<typeof ___VERTER_COMP___>).$props,
      ...({} as InstanceType<typeof ___VERTER_COMP___>) }
      const ___VERTER__comp = { 
                  //...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
                  ...___VERTER__ctx
                }
      ___VERTER__comp;
      ___VERTER__ctx;
      return ___VERTER_COMP___;
      }

      const __VERTER__RESULT = ___VERTER_COMPONENT__();
      export default __VERTER__RESULT;"
    `);
  });

  it.only("slot def", () => {
    const source = `<template> 
  <div> 
    <slot />
  </div>
</template>`;
    const p = process(source);

    expect(p.content).toMatchInlineSnapshot(`
      "import 'vue/jsx';

      // patching elements
      declare global {
        namespace JSX {
          export interface IntrinsicClassAttributes<T> {
            // $children: T extends { $slots: infer S } ? { default?: never } & { [K in keyof S]: S[K] extends (...args: infer Args) => any ? (...args: Args) => JSX.Element : () => JSX.Element } : { default?: never }
            $children?: T extends { $slots: infer S } ? S : undefined
          }
        }
      }
      declare module 'vue' {
        interface HTMLAttributes {
          $children?: {
            default: () => JSX.Element
          }
        }
      }
      import { defineComponent as ___VERTER_defineComponent } from 'vue';
      // Helper to retrieve slots definitions
      declare function ___VERTER_extract_Slots<CompSlots>(comp: { new(): { $slots: CompSlots } }, slots?: undefined): CompSlots
      declare function ___VERTER_extract_Slots<CompSlots, Slots extends Record<string, any> = {}>(comp: { new(): { $slots: CompSlots } }, slots?: Slots): Slots extends undefined ? CompSlots : Slots
      // Generate slot component based on the slots
      declare function ___VERTER_Create_Slot_Component<T>(slots: T): T extends { readonly [x: string]: (...args: any[]) => any } ? (props: {
        [K in keyof T]: (T[K] extends (props: infer Props) => any ? Props : {}) & (K extends 'default' ? { name?: 'default' } : { name: K })
      }[keyof T]) => JSX.Element : never


      function ___VERTER_COMPONENT__() {
      const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({})
      function ___VERTER__TEMPLATE_RENDER() {
      <> 
        <div> 
          <___VERTER_SLOT_COMP />
        </div>

      </>}
      ___VERTER__TEMPLATE_RENDER();
      const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER_SLOT_COMP = ___VERTER_Create_Slot_Component(___VERTER_extract_Slots(___VERTER_COMP___))
      const ___VERTER__ctx = { ...({} as InstanceType<typeof ___VERTER_COMP___>) }
      const ___VERTER__comp = { 
                  //...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
                  ...___VERTER__ctx
                }
      ___VERTER__comp;
      ___VERTER__ctx;
      return ___VERTER_COMP___;
      }

      const __VERTER__RESULT = ___VERTER_COMPONENT__();
      export default __VERTER__RESULT;"
    `);
  });

  it.todo("SFC error when doing template slots", () => {
    const source = `  
<Comp>
    <template v-if="isLoading" #default> </template>
    <template v-else-if="!ticketData.length">
      <div class="flex flex-col items-center justify-center gap-30.4x">
        <img class="mt-116.8x h-40 w-40" src="@/assets/emptytickets-icon.svg" />
        <span class="text-session text-neutral-450">亲，没有找到相关信息～</span>
      </div>
    </template>
    <template v-else>
      <template #processing>
        <div>XXX</div>
      </template>
      <template #completed>
        <div>YYY</div>
      </template>
    </template>
    
</Comp>`;
  });
});
