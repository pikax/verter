import { processRender, FunctionExportName } from './render';
import { createContext } from '@verter/core';

describe('processRender', () => {
  function process(source: string, filename = 'test.vue') {
    const context = createContext(source, filename)
    return processRender(context)
  }


  it('should return new file name', () => {
    const result = process(`<template><div></div></template>`)
    expect(result).toMatchObject({
      filename: 'test.vue.render.tsx',
    })
  })

  it('should have the correct loc', () => {
    const result = process(`<script></script><template><div></div></template>`)
    expect(result.loc).toMatchInlineSnapshot(`
          {
            "end": {
              "column": 39,
              "line": 1,
              "offset": 38,
            },
            "source": "<div></div>",
            "start": {
              "column": 28,
              "line": 1,
              "offset": 27,
            },
          }
        `)
  })

  it('should return the render function', () => {
    const result = process(`<template><div></div></template>`)

    expect(result.content).toMatchInlineSnapshot(`
      "import { RenderContext } from './../test.vue.script.ts'
      export function Render() {
      const ___VERTER___ctx = RenderContext()
      return <>
      <div></div>
      </>
      }
      "
    `)
  })

  it('should be generic', () => {
    const result = process(`<template><div></div></template><script generic="T"></script>`)
    expect(result.content).toMatchInlineSnapshot(`
      "import { RenderContext } from './../test.vue.script.ts'
      export function Render<T>() {
      const ___VERTER___ctx = RenderContext<T>()
      return <>
      <div></div>
      </>
      }
      "
    `)

  })

  it('should handle empty template', () => {
    const result = process(`<template></template>`)
    expect(result.content).toMatchInlineSnapshot(`
      "import { RenderContext } from './../test.vue.script.ts'
      export function Render() {
      const ___VERTER___ctx = RenderContext()
      return <>

      </>
      }
      "
    `)
  })

  it('should handle no template', () => {
    const result = process(`<script></script>`)
    expect(result.content).toMatchInlineSnapshot(`
      "export function Render() {} {
      return <></>
      } "
    `)
  });

  it('should parse the template', () => {
    const result = process(`<template><div v-if="true">test</div><span v-else>foo</span></template>`)
    expect(result.content).toMatchInlineSnapshot(`
      "import { RenderContext } from './../test.vue.script.ts'
      export function Render() {
      const ___VERTER___ctx = RenderContext()
      return <>
      { ()=> {if(true){<div >{ "test" }</div>}else{
      <span >{ "foo" }</span>
      }}}
      </>
      }
      "
    `)
  })

  describe('comments', () => {
    it('should ignore the template if is commmented', () => {
      const result = process(`<template><!--<div></div>--></template>`)
      expect(result.content).toMatchInlineSnapshot(`
        "import { RenderContext } from './../test.vue.script.ts'
        export function Render() {
        const ___VERTER___ctx = RenderContext()
        return <>
        {/*<div></div>*/}
        </>
        }
        "
      `)
    })

    it('empty if template in comment', () => {
      const result = process(`<!--<template></template>-->`)
      expect(result.content).toMatchInlineSnapshot(`
        "export function Render() {} {
        return <></>
        } "
      `)
    })
  })
})

