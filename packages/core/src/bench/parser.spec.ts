import { parseOld, parse, ValidFiles } from "./test.utils.js";

describe("bench parser sanity tests", () => {
  describe("simple", () => {
    const content = `<template>
<div> {{ foo }} </div>
</template>
<script lang="ts">
</script>`;
    it("old", () => {
      const { render } = parseOld(content);

      expect(render.content).toMatchSnapshot();
    });
    it("new", () => {
      const { render } = parse(content);
      expect(render.result).toMatchSnapshot();
    });
  });

  describe("test file", () => {
    // console.log(Object.keys(ValidFiles));
    const content = ValidFiles["ContactInformation.vue"];

    it("old", () => {
      const { render } = parseOld(content);

      expect(render.content).toMatchSnapshot();
    });
    it("new", () => {
      const { render } = parse(content);
      expect(render.result).toMatchSnapshot();
    });
  });

  describe.each(Object.keys(ValidFiles))("single %s", (x) => {
    const file = ValidFiles[x];
    it("parseOld", () => {
      parseOld(file).render.content;
    });
    it("parse", () => {
      parse(file).render.result;
    });
  });
});
