import {
  expectFindStringWithMap,
  testSourceMaps,
} from "../../../utils.test-utils";
import { processRender, FunctionExportName } from "./render";
import { createContext } from "@verter/core";

describe("processRender", () => {
  function process(source: string, filename = "test.vue") {
    const context = createContext(source, filename);
    return processRender(context);
  }

  it("should return new file name", () => {
    const result = process(`<template><div></div></template>`);
    expect(result).toMatchObject({
      filename: "test.vue.render.tsx",
    });
  });

  it("should have the correct loc", () => {
    const result = process(`<script></script><template><div></div></template>`);
    expect(result.loc).toMatchInlineSnapshot(`
      {
        "source": "<script></script><template><div></div></template>",
      }
    `);
  });
  it("should return the render function", () => {
    const result = process(`<template><div></div></template>`);

    expect(result.content).toContain("export function ___VERTER___Render()");
  });

  it("should be generic", () => {
    const result = process(
      `<template><div></div></template><script lang="ts" generic="T"></script>`
    );
    expect(result.content).toContain("export function ___VERTER___Render<T>()");
  });

  it("should be generic complex", () => {
    const result = process(
      `<template><div></div></template><script lang="ts" generic="T extends 'foo' | 'bar'"></script>`
    );
    expect(result.content).toContain(
      "export function ___VERTER___Render<T extends 'foo' | 'bar'>()"
    );
  });

  it("should map the generic correctly", () => {
    const result = process(
      `<template><div></div></template><script lang="ts" generic="T extends 'foo' | 'bar'"></script>`
    );
    expect(result.content).toContain(
      "export function ___VERTER___Render<T extends 'foo' | 'bar'>()"
    );

    // might not need a map
    expectFindStringWithMap("T extends 'foo' | 'bar'", result);
  });

  it("should handle empty template", () => {
    const result = process(`<template></template>`);
    expect(result.content).toContain(`return <>

</>`);
  });

  it("should handle no template", () => {
    const result = process(`<script></script>`);
    expect(result.content).toContain(`return <></>`);
  });

  it("should parse the template", () => {
    const result = process(
      `<template><div v-if="true">test</div><span v-else>foo</span></template>`
    );

    expect(result.content).toContain('{ "test" }');
  });

  describe("comments", () => {
    it("should ignore the template if is commmented", () => {
      const result = process(`<template><!--<div></div>--></template>`);
      expect(result.content).toContain(`return <>
{/*<div></div>*/}
</>`);
    });

    it("empty if template in comment", () => {
      const result = process(`<!--<template></template>-->`);
      expect(result.content).toContain(`return <></>`);
    });
  });

  describe("setup", () => {
    it.skip("should work with setup", () => {
      const result = process(
        `<template><div></div></template><script setup></script>`
      );
      expect(result.content).toMatchInlineSnapshot(`
        "

        export function ___VERTER___Render() {

        const ___VERTER___Component = new (___VERTER___defineComponent(___VERTER___default))
        const ___VERTER___BindingContextCTX = ___VERTER___BindingContext()
        const ___VERTER___ctx = {
            ...___VERTER___Component,
            ...({} as ___VERTER___ShallowUnwrapRef<typeof ___VERTER___BindingContextCTX>),
        }

        return <>
        <div></div>
        </>
        }"
      `);
    });
  });
});
