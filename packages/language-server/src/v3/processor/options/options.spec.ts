import { createContext } from '@verter/core'
import { processOptions } from './options.js'
import { type Position, SourceMapConsumer } from "source-map-js";
import { MagicString } from 'vue/compiler-sfc';

import fs from 'fs'


describe('processor options', () => {
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


  function process(source: string, filename = 'test.vue') {
    const context = createContext(source, filename, true)
    return processOptions(context)
  }

  function getSourceMap({ s }: { s: MagicString }) {
    return new SourceMapConsumer(s.generateMap({ hires: true, includeContent: true }) as any)
  }


  function expectFindStringWithMap(toFind: string, { content, s, loc: { source } }: { content: string, s: MagicString, loc: { source: string } }) {
    function getLineOffsets(text: string) {
      const lineOffsets: number[] = [];
      let isLineStart = true;

      for (let i = 0; i < text.length; i++) {
        if (isLineStart) {
          lineOffsets.push(i);
          isLineStart = false;
        }
        const ch = text.charAt(i);
        isLineStart = ch === "\r" || ch === "\n";
        if (ch === "\r" && i + 1 < text.length && text.charAt(i + 1) === "\n") {
          i++;
        }
      }

      if (isLineStart && text.length > 0) {
        lineOffsets.push(text.length);
      }

      return lineOffsets;
    }
    function clamp(num: number, min: number, max: number) {
      return Math.max(min, Math.min(max, num));
    }
    /**
     * Get the line and character based on the offset
     * @param offset The index of the position
     * @param text The text for which the position should be retrived
     * @param lineOffsets number Array with offsets for each line. Computed if not given
     */
    function positionAt(
      offset: number,
      text: string,
      lineOffsets = getLineOffsets(text)
    ): Position {
      offset = clamp(offset, 0, text.length);

      let low = 0;
      let high = lineOffsets.length;
      if (high === 0) {
        return {
          line: 1,
          column: offset,
        };
      }

      while (low <= high) {
        const mid = Math.floor((low + high) / 2);
        const lineOffset = lineOffsets[mid];

        if (lineOffset === offset) {
          return {
            line: mid + 1,
            column: 0
          };
        } else if (offset > lineOffset) {
          low = mid + 1;
        } else {
          high = mid - 1;
        }
      }

      // low is the least x for which the line offset is larger than the current offset
      // or array.length if no line offset is larger than the current offset
      const line = low;
      return { line, column: offset - lineOffsets[line] };
    }

    const map = getSourceMap({ s })

    const index = content.indexOf(toFind)

    const pos = positionAt(index, content)
    const loc = map.originalPositionFor(pos)

    const sourceLines = getLineOffsets(source)

    const lineOffset = sourceLines[loc.line - 1]
    expect(source.slice(lineOffset, lineOffset + toFind.length)).toBe(toFind)
  }



  describe('filename', () => {
    test.each([
      'ts',
      'js',
      'jsx',
      'tsx',
      ''
    ])('correct filename %s', (lang) => {
      const result = process(`<script${lang ? ` lang="${lang}"` : ''}>export default {};</script>`)
      expect(result).toMatchObject({
        filename: 'test.vue.options.' + (lang || 'js'),
      })
    })
  })

  it('should have the correct loc', () => {
    const result = process(`<script>export default {}</script><template><div></div></template>`)
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


  describe('compile export', () => {
    test('script', () => {
      const result = process(`<script>export default {}</script>`)

      expect(result.content).toContain('const ____VERTER_COMP_OPTION__COMPILED = {}')
    })
    test('defineComponent', () => {
      const result = process(`<script>export default defineComponent({})</script>`)
      expect(result.content).toContain('const ____VERTER_COMP_OPTION__COMPILED = defineComponent({})')
    })

    test('script setup', () => {
      const result = process(`<script setup>defineProps({test: String})</script>`)
      expect(result.content).toContain('const ____VERTER_COMP_OPTION__COMPILED = ')
    })

    test('script setup ts', () => {
      const result = process(`<script setup lang="ts" generic="T">defineProps<{test: T}>()</script>`)
      expect(result.content).toContain('const ____VERTER_COMP_OPTION__COMPILED = /*#__PURE__*/_defineComponent');
    })


    test('export options', () => {
      const result = process(`<script>export default {}</script>`)

      expect(result.content).toContain('export const ____VERTER_COMP_OPTION__ = __VERTER__defineComponent(____VERTER_COMP_OPTION__COMPILED)')
    })
    test('export options ts', () => {
      const result = process(`<script lang="ts">export default {}</script>`)
      expect(result.content).toContain('export const ____VERTER_COMP_OPTION__ = {}')
    })
  })

  describe('imports', () => {
    it('keep', () => {
      const result = process(`<script lang="ts">import { defineComponent } from 'vue'; import 'test/test'; import type {Test} from 'xx';</script>`)
      expect(result.content).toContain(`import { defineComponent } from 'vue';import 'test/test';import type {Test} from 'xx';`)
    })

    it('add compiled', () => {
      const result = process(`<script setup lang="ts"></script>`)
      expect(result.content).toContain(`import { defineComponent as _defineComponent } from 'vue'`)
    })

    it('add generated', () => {
      const result = process(`<script setup lang="ts"></script>`)
      expect(result.content).toContain(`import { defineComponent as _defineComponent } from 'vue'`)
      expect(result.content).toContain(`import { defineComponent as __VERTER__defineComponent, type DefineComponent as __VERTER__DefineComponent } from 'vue';`)

    })

    it('move to top', () => {
      const result = process('<script setup>import {ref} from "vue"; const a = ref(); import "test";</script>')
      expect(result.content).toContain(`import {ref} from "vue";import "test";`)
    })
  })


  describe('setup', () => {
    describe('ts', () => {
      it('simple', () => {
        const result = process(`<script setup lang="ts">import { ref, Ref } from 'vue';\nconst a = ref();</script><template><div>test</div></template>`)
        expect(result.content).toContain(`const a = ref();`)
        expect(result.content).toContain('function BuildBindingContext() {')

        testSourceMaps({
          content: result.content,
          map: result.s.generateMap({ hires: true, includeContent: true })
        })

        expectFindStringWithMap('const a = ref();', result)
        expectFindStringWithMap(`import { ref, Ref } from 'vue';`, result)
      })

      it('async', () => {
        const result = process(`<script setup lang="ts">import { ref, Ref } from 'vue';\nconst a = ref();\nawait Promise.resolve()</script><template><div>test</div></template>`)

        expect(result.content).toContain(`const a = ref();`)
        expect(result.content).toContain(`await Promise.resolve()`)
        expect(result.content).toContain('async function BuildBindingContext() {')
        expect(result.content).toContain('export type BindingContext = ReturnType<typeof BuildBindingContext> extends Promise<infer R> ? R : never;')
      })

      it.only('generic', () => {
        const result = process(`<script setup lang="ts" generic="T">import { ref, Ref } from 'vue';\nconst a = ref() as Ref<T>;</script><template><div>test</div></template>`)

        expect(result.content).toMatchInlineSnapshot(`
          "import { renderList as ___VERTER___renderList, normalizeClass as ___VERTER___normalizeClass, normalizeStyle as ___VERTER___normalizeStyle, defineComponent as __VERTER__defineComponent, type DefineComponent as __VERTER__DefineComponent } from 'vue';
          import { ref, Ref } from 'vue';
          function BuildBindingContext<T>() {

          const a = ref() as Ref<T>;
          return {} as {ref: typeof ref, Ref: typeof Ref, a: typeof a
          }

          }

          export type BindingContext<T> = ReturnType<typeof BuildBindingContext<T>>;

          import { defineComponent as _defineComponent } from 'vue'


          const ____VERTER_COMP_OPTION__COMPILED = /*#__PURE__*/_defineComponent({
            __name: 'test',
            setup(__props) {
          const a = ref() as Ref<T>;
          return {}
          }

          })
          declare function __VERTER__isDefineComponent<T>(o: T): o is (T extends __VERTER__DefineComponent<any, any, any, any, any> ? T : T & never);
          const ____VERTER_COMP_OPTION__RESULT = __VERTER__isDefineComponent(____VERTER_COMP_OPTION__COMPILED) ? ____VERTER_COMP_OPTION__COMPILED : __VERTER__defineComponent(____VERTER_COMP_OPTION__COMPILED)
          export const ____VERTER_COMP_OPTION__ = {} as typeof ____VERTER_COMP_OPTION__COMPILED & typeof ____VERTER_COMP_OPTION__RESULT;"
        `)

        expect(result.content).toContain(`const a = ref();`)
        expect(result.content).toContain(`await Promise.resolve()`)
        expect(result.content).toContain('async function BuildBindingContext() {')
        expect(result.content).toContain('export type BindingContext = ReturnType<typeof BuildBindingContext> extends Promise<infer R> ? R : never;')
      })

    })
  })

  describe('options', () => {

  })
})
