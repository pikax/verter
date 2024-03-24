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

              const ____VERTER_COMP_OPTION__ = {
                  data(){
                      return { test: 'hello' }
                  }
              }
              const ___VERTER_COMP___ = ___VERTER_defineComponent(____VERTER_COMP_OPTION__)
      const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
      const ___VERTER__comp = { 
                  ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                }
      function ___VERTER__TEMPLATE_RENDER() {
      <>
                  <div key={___VERTER__ctx.test}>{ "test" }</div>
              
      </>}
      }

              "
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
      "import { defineComponent as ___VERTER_defineComponent } from 'vue';

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
              const ___VERTER_COMP___ = ____VERTER_COMP_OPTION__
      const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
      const ___VERTER__comp = { 
                  ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                }
      function ___VERTER__TEMPLATE_RENDER() {
      <>
                  <div key={___VERTER__ctx.test}>{ "test" }</div>
              
      </>}
      }

              "
    `);

    testSourceMaps(p);
  });

  describe("test", () => {
    describe("script", () => {
      it.only("should generate default option", () => {
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
                    }

          }
          "
        `);
        testSourceMaps(p);
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


                          const ____VERTER_COMP_OPTION__ = {}
                          const ___VERTER_COMP___ = ___VERTER_defineComponent(____VERTER_COMP_OPTION__)
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                    }

          }
          "
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


                          const ____VERTER_COMP_OPTION__ = {
                              name: 'test',
              
                              data(){ 
                                  return {
                                      foo: 'foo'
                                  }
                              }
                          }
                          const ___VERTER_COMP___ = ___VERTER_defineComponent(____VERTER_COMP_OPTION__)
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                    }

          }
          "
        `);
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
                    }

          }
          "
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
                    }

          }
          "
        `);
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
                  

                  const ____VERTER_COMP_OPTION__ = {
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo'
                          }
                      }
                  }
                  const ___VERTER_COMP___ = ___VERTER_defineComponent(____VERTER_COMP_OPTION__)
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                    }

          }
          "
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


                  

                  const ____VERTER_COMP_OPTION__ = {
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo' as T
                          }
                      }
                  }
                  const ___VERTER_COMP___ = ___VERTER_defineComponent(____VERTER_COMP_OPTION__)
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                    }

          }
          "
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


                  

                  const ____VERTER_COMP_OPTION__ = {
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo' as T
                          }
                      }
                  }
                  const ___VERTER_COMP___ = ___VERTER_defineComponent(____VERTER_COMP_OPTION__)
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                    }
          function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div>
                          { ___VERTER__ctx.foo + 'aa' as T}
                      </div>
                  
          </>}
          }

                  
                  "
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


                  

                  const ____VERTER_COMP_OPTION__ = {
                      name: 'test',

                      data(){ 
                          return {
                              foo: 'foo' as T
                          }
                      }
                  }
                  const ___VERTER_COMP___ = ___VERTER_defineComponent(____VERTER_COMP_OPTION__)
          const ___VERTER__ctx = { ...(new ___VERTER_COMP___()) }
          const ___VERTER__comp = { 
                      ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
                    }
          function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div>
                          { (___VERTER__ctx.foo + 'aa' as typeof ___VERTER__ctx.foo).supa?.()}
                      </div>
                  
          </>}
          }

                  
                  "
        `);
      });
    });
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
    expect(p.content).toMatchInlineSnapshot();
  });

  it("should work setup + script", () => {
    const source = `<script setup lang="ts" generic="T extends string">
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
    expect(p.content).toMatchInlineSnapshot();
    // testSourceMaps(p.context.script)
  });
});
