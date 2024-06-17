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
          "
          export function RenderContext() {

          return {

          }
          }
          "
        `)

    })


    it('should be generic', () => {
        const result = process(`<script generic="T"></script>`)
        expect(result.content).toMatchInlineSnapshot(`
          "
          export function RenderContext<T>() {

          return {

          }
          }
          "
        `)
    })

    it('should handle empty script', () => {
        const result = process(``)
        expect(result.content).toMatchInlineSnapshot(`
          "
          export function RenderContext() {

          return {

          }
          }
          "
        `)
    })
    it('should handle no script', () => {
        const result = process(``)
        expect(result.content).toMatchInlineSnapshot(`
          "
          export function RenderContext() {

          return {

          }
          }
          "
        `)
    })

    // TODO handle multiple scripts
    it.todo('should handle multiple scripts', () => {
        const result = process(`<script setup></script><script></script>`)
        expect(result.content).toMatchInlineSnapshot(`
          "
          export function RenderContext() {

          return {
          }
          }
          "
        `)
    })


    describe('setup', () => {
        it('should handle setup', () => {
            const result = process(`<script setup></script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "
              export function RenderContext() {

              return {

              }
              }
              "
            `)
        })

        test('not move import', () => {
            const result = process(`<script setup>import {ref} from 'vue'; const a = {}</script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import {ref} from 'vue';
              export function RenderContext() {
               const a = {}
              return {
              ref,
              a
              }
              }
              "
            `)
        })
        test('move import', () => {
            const result = process(`<script setup>const a = {};import {ref} from 'vue'; </script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import {ref} from 'vue';
              export function RenderContext() {
              const a = {}; 
              return {
              ref,
              a
              }
              }
              "
            `)
        })

        test('keep import order', () => {
            const result = process(`<script setup>import 'a'; import 'b'; const a = {};import {ref} from 'vue';</script>`)
            expect(result.content).toMatchInlineSnapshot(`
              "import 'a';import 'b';import {ref} from 'vue';
              export function RenderContext() {
                const a = {};
              return {
              ref,
              a
              }
              }
              "
            `)
        })
    })
})