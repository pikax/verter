import { createContext } from '@verter/core'
import { processScript } from './script.js'

describe('processScript', () => {
    function process(source: string, filename = 'test.vue') {
        const context = createContext(source, filename)
        return processScript(context)
    }

    it('should return new file name', () => {
        const result = process(`<template><div></div></template><script><script>`)
        expect(result).toMatchObject({
            filename: 'test.vue.script.ts',
        })
    })

    it('should have the correct loc', () => {
        const result = process(`<script>export default {}</script><template></template>`)
        expect(result.loc).toMatchInlineSnapshot(`
          {
            "end": {
              "column": 26,
              "line": 1,
              "offset": 25,
            },
            "source": "export default {}",
            "start": {
              "column": 9,
              "line": 1,
              "offset": 8,
            },
          }
        `)
    })

    it('should return RenderContext', () => {
        const result = process(`<script></script>`)
        expect(result.content).toMatchInlineSnapshot(`
          "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';

          export function resolveRenderContext() {

          return {} as {}
          }
          type RenderContextType = ReturnType<typeof resolveRenderContext>;
          export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
        `)

    })


    it('should be generic', () => {
        const result = process(`<script generic="T"></script>`)
        expect(result.content).toMatchInlineSnapshot(`
          "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';

          export function resolveRenderContext<T>() {

          return {} as {}
          }
          type RenderContextType<T> = ReturnType<typeof resolveRenderContext<T>>;
          export declare function RenderContext<T>(): __VERTER_ShallowUnwrapRef<RenderContextType<T>>;"
        `)
    })

    it('should handle empty script', () => {
        const result = process(``)
        expect(result.content).toMatchInlineSnapshot(`
          "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';

          export function resolveRenderContext() {

          return {} as {}
          }
          type RenderContextType = ReturnType<typeof resolveRenderContext>;
          export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
        `)
    })
    it('should handle no script', () => {
        const result = process(``)
        expect(result.content).toMatchInlineSnapshot(`
          "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';

          export function resolveRenderContext() {

          return {} as {}
          }
          type RenderContextType = ReturnType<typeof resolveRenderContext>;
          export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
        `)
    })

    // TODO handle multiple scripts
    it.todo('should handle multiple scripts', () => {
        const result = process(`<script setup></script><script></script>`)
        expect(result.content).toMatchInlineSnapshot(`
          "
          export function resolveRenderContext() {

          return {

          }
          }
          type RenderContextType = ReturnType<typeof resolveRenderContext>;
          export declare const RenderContext: RenderContextType;"
        `)
    })


    describe('setup', () => {
        it('should handle setup', () => {
            const result = process(`<script setup></script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';

              export function resolveRenderContext() {

              return {} as {}
              }
              type RenderContextType = ReturnType<typeof resolveRenderContext>;
              export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
            `)
        })

        test('not move import', () => {
            const result = process(`<script setup>import {ref} from 'vue'; const a = {}</script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';
              import {ref} from 'vue';
              export function resolveRenderContext() {
               const a = {}
              return {} as {a: typeof a}
              }
              type RenderContextType = ReturnType<typeof resolveRenderContext>;
              export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
            `)
        })
        test('move import', () => {
            const result = process(`<script setup>const a = {};import {ref} from 'vue'; </script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';
              import {ref} from 'vue';
              export function resolveRenderContext() {
              const a = {}; 
              return {} as {a: typeof a}
              }
              type RenderContextType = ReturnType<typeof resolveRenderContext>;
              export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
            `)
        })

        test('keep import order', () => {
            const result = process(`<script setup>import 'a'; import 'b'; const a = {};import {ref} from 'vue';</script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';
              import 'a';import 'b';import {ref} from 'vue';
              export function resolveRenderContext() {
                const a = {};
              return {} as {a: typeof a}
              }
              type RenderContextType = ReturnType<typeof resolveRenderContext>;
              export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
            `)
        })

        test('async', () => {
            const result = process(`<script setup>import {ref} from 'vue'; const a = {}; await Promise.resolve()</script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';
              import {ref} from 'vue';
              export async function resolveRenderContext() {
               const a = {}; await Promise.resolve()
              return {} as {a: typeof a}
              }
              type RenderContextType = ReturnType<typeof resolveRenderContext> extends Promise<infer R> ? R : never;
              export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
            `)
        })

        test('generic async', () => {
            const result = process(`<script setup generic="T" lang="ts">import {ref, Ref} from 'vue'; const a = ref() as Ref<T>; await Promise.resolve()</script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';
              import {ref, Ref} from 'vue';
              export async function resolveRenderContext<T>() {
               const a = ref() as Ref<T>; await Promise.resolve()
              return {} as {a: typeof a}
              }
              type RenderContextType<T> = ReturnType<typeof resolveRenderContext<T>> extends Promise<infer R> ? R : never;
              export declare function RenderContext<T>(): __VERTER_ShallowUnwrapRef<RenderContextType<T>>;"
            `)
        })

        test('not expose import type', () => {
            const result = process(`<script setup lang="ts">import { type Ref } from 'vue'; import type { Computed } from 'vue'; import { ref, type Test} from 'vue' </script>`);
            expect(result.content).toMatchInlineSnapshot(`
              "import { ShallowUnwrapRef as __VERTER_ShallowUnwrapRef } from 'vue';
              import { type Ref } from 'vue';import type { Computed } from 'vue';import { ref, type Test} from 'vue'
              export function resolveRenderContext() {
                 
              return {} as {}
              }
              type RenderContextType = ReturnType<typeof resolveRenderContext>;
              export declare function RenderContext(): __VERTER_ShallowUnwrapRef<RenderContextType>;"
            `)
        })
    })
})