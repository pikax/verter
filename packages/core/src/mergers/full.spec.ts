import { MagicString } from "@vue/compiler-sfc";
import { createBuilder } from "../builder"
import { mergeFull } from "./full"
import fs from 'node:fs'


describe('Mergers Full', () => {

    function testSourceMaps({ content, map }: {
        content: string,
        map: ReturnType<MagicString["generateMap"]>
    }
    ) {
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


    const builder = createBuilder()
    function process(content: string, fileName = 'test.vue') {
        const { locations, context } = builder.preProcess(fileName, content)
        return mergeFull(locations, context)
    }


    it('should work options API', () => {
        const source = `<script>
        export default {
            data(){
                return { test: 'hello' }
            }
        }
        </script>
        <template>
            <div :key="test">test</div>
        </template>`

        const p = process(source)

        expect(p.source).toBe(source)

        expect(p.content).toMatchInlineSnapshot(`
          "/* @jsxImportSource vue */

                  const ____VERTER_COMP_OPTION__ = {
                      data(){
                          return { test: 'hello' }
                      }
                  }
                  const ___VERTER_COMP___ = ___VERTER_defineComponent(___VERTER_COMP_OPTION__) 
          /* verter:template-context start */
          const ___VERTER_ctx = {
          ...(new ____VERTER_COMP_OPTION__())
          }
          /* verter:template-context end */


          /* verter:template-render start */
          function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div key={___VERTER__ctx.test}>{ "test" }</div>
                  
          </>
          }
          /* verter:template-render end */
          "
        `)

        p.map
        testSourceMaps(p)
    })

    it('should work setup', () => {
        const source = `<script setup>
        const test = ref('hello')
        </script>
        <template>
            <div :key="test">test</div>
        </template>`

        const p = process(source)

        expect(p.source).toBe(source)

        expect(p.content).toMatchInlineSnapshot(`
          "/* @jsxImportSource vue */
          <script setup>
                  const test = ref('hello')
                  </script>
                  
          /* verter:template-context start */
          const ___VERTER_ctx = {
          ...(new ____VERTER_COMP_OPTION__())
          test: unref(test)
          }
          /* verter:template-context end */


          /* verter:template-render start */
          function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div key={___VERTER__ctx.test}>{ "test" }</div>
                  
          </>
          }
          /* verter:template-render end */
          "
        `)
    })

    it.only('sss', () => {
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
        </style>`

        const p = process(source)

        expect(p.source).toBe(source)

        expect(p.content).toMatchInlineSnapshot(`
          "/* @jsxImportSource vue */

                  
          /* verter:template-context start */
          const ___VERTER_ctx = {
          ...(new ____VERTER_COMP_OPTION__())
          }
          /* verter:template-context end */

          /* verter:template-render end */

                  
                  <script>
                  export const Supa = ''
                  const ___VERTER_COMP___ = ___VERTER_COMP_OPTION__ </script>
                  

          /* verter:template-render start */
          function ___VERTER__TEMPLATE_RENDER() {
          <>
                      <div key={___VERTER__ctx.test}>{ "test" }</div>
                  
          </>
          }"
        `)
        testSourceMaps(p)
    })


    it('should remove comment code', () => {
        const source = `<template>
        <div :key="test">test</div>
    </template>
    <!--<script setup>
    const test = ref('hello')
    </script>-->`

        const p = process(source)

        expect(p.source).toBe(source)
        expect(p.content).not.contain('<!--<script')
    })

    describe.skip('generic', () => {

        describe('error', () => {
            it('should not work if not ts', () => {
                const source = `<script setup lang="ts" generic="T">
                const test = 'test';
                </script>
                <style>
                .hello {
                    
                }
                </style>`

                const p = process(source)

                expect(p.source).toBe(source)
                expect(p.content).toMatchInlineSnapshot(`
                  "/* @jsxImportSource vue */

                  /* verter:template-context start */
                  const ___VERTER_ctx = {
                  ...(new ____VERTER_COMP_OPTION__())
                  }
                  /* verter:template-context end */
                  <script setup generic="T">
                                  </script>"
                `)

            })
        })

    })

    it('should get an error generics are only supported in ts', () => {
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

        const p = process(source)

        expect(p.source).toBe(source)
        expect(p.content).toMatchInlineSnapshot()
    })

    it('should work setup + script', () => {
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

        const p = process(source)

        expect(p.source).toBe(source)
        expect(p.content).toMatchInlineSnapshot()
        // testSourceMaps(p.context.script)
    })

})