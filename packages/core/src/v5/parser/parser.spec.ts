import { parser } from "./index.js";

describe("parser", () => {
  describe("filename", () => {
    it("should return filename", () => {
      const parsed = parser("", "super.vue");

      expect(parsed).toMatchObject({
        filename: "super.vue",
      });
    });

    it("should default filename", () => {
      const parsed = parser("");

      expect(parsed).toMatchObject({
        filename: "temp.vue",
      });
    });
  });

  describe("magicstring", () => {
    it("should have magicstring", () => {
      const source = "<script></script>";
      const parsed = parser(source);

      expect(parsed.s.original).toBe(source);
    });

    it("magicstring should not have changes", () => {
      const source = "<script></script>";
      const parsed = parser(source);

      expect(parsed.s.original).toBe(source);
      expect(parsed.s.toString()).toBe(source);
    });

    it("should add empty script", () => {
      const source = "<template></template>";
      const parsed = parser(source);

      expect(parsed.s.toString()).toContain(source);
      expect(parsed.s.toString()).toContain("<script></script>");
    });

    it("should add empty if script is in comment", () => {
      const source = `<template></template><!-- <script TEST></script> -->`;
      const parsed = parser(source);

      expect(parsed.s.toString()).toContain(source);
      expect(parsed.s.toString()).toContain("<script></script>");
    });

    // TODO - NOTE this is a quite an edge case, but it should be handled
    it.todo(
      "should not interpret a script inside template as a valid script",
      () => {
        const source = `<template> <script test></script></template>`;
        const parsed = parser(source);

        expect(parsed.s.toString()).toContain(source);
        expect(parsed.s.toString()).toContain("<script></script>");
      }
    );
  });

  describe("generic", () => {
    it("not generic if empty", () => {
      const source = ``;
      const parsed = parser(source);

      expect(parsed.generic).toBe(undefined);
    });

    it('generic="T"', () => {
      const source = `<script lang="ts" generic="T"></script>`;
      const parsed = parser(source);
      expect(parsed.generic).toBe("T");
    });

    it('generic="T extends string"', () => {
      const source = `<script lang="ts" generic="T extends string"></script>`;
      const parsed = parser(source);
      expect(parsed.generic).toBe("T extends string");
    });

    it('generic="T extends string, U"', () => {
      const source = `<script lang="ts" generic="T extends string, U"></script>`;
      const parsed = parser(source);
      expect(parsed.generic).toBe("T extends string, U");
    });

    it('generic="T extends string, U extends number"', () => {
      const source = `<script lang="ts" generic="T extends string, U extends number"></script>`;
      const parsed = parser(source);
      expect(parsed.generic).toBe("T extends string, U extends number");
    });

    it('generic="T extends Deep<U>, U extends Record<string, { sup: boolean }>"', () => {
      const source = `<script lang="ts" generic="T extends Deep<U>, U extends Record<string, { sup: boolean }>"></script>`;
      const parsed = parser(source);
      expect(parsed.generic).toBe(
        "T extends Deep<U>, U extends Record<string, { sup: boolean }>"
      );
    });

    it('not generic if not lang="ts"', () => {
      const source = `<script generic="T"></script>`;
      const parsed = parser(source);
      expect(parsed.generic).toBe(undefined);
    });
  });

  describe("isAsync", () => {
    it("not async", () => {
      const source = ``;
      const parsed = parser(source);

      expect(parsed.isAsync).toBe(false);
    });

    it("async", () => {
      const source = `<script>await Promise.resolve()</script>`;
      const parsed = parser(source);

      expect(parsed.isAsync).toBe(true);
    });
  });

  describe("template", () => {
    it("should parse template", () => {
      const source = `<template></template>`;
      const parsed = parser(source);

      expect(parsed.s.toString()).toContain(source);
    });
  });

  describe("script", () => {
    describe("isMain", () => {
      it("single script", () => {
        const source = `<script></script>`;
        const parsed = parser(source);

        expect(parsed.s.toString()).toContain(source);

        expect(parsed.blocks).toMatchObject([
          {
            type: "script",
            lang: "js",
            isMain: true,
          },
        ]);
      });

      it("single script setup", () => {
        const source = `<script setup></script>`;
        const parsed = parser(source);

        expect(parsed.s.toString()).toContain(source);

        expect(parsed.blocks).toMatchObject([
          {
            type: "script",
            lang: "js",
            isMain: true,
          },
        ]);
      });

      it("multiple scripts", () => {
        const source = `<script></script><script setup>const a = {}</script>`;
        const parsed = parser(source);

        expect(parsed.s.toString()).toContain(source);

        expect(parsed.blocks).toMatchObject([
          {
            type: "script",
            lang: "js",
            isMain: false,
          },
          {
            type: "script",
            lang: "js",
            isMain: true,
          },
        ]);
      });

      it.only("more than 2 scripts should have the last", () => {
        const source = `<script></script><script setup>const a = {}</script><script>const b = {}</script>`;
        const parsed = parser(source);

        expect(parsed.s.toString()).toContain(source);

        expect(parsed.blocks).toMatchObject([
          {
            type: "script",
            lang: "js",
            isMain: false,
          },
          {
            type: "script",
            lang: "js",
            isMain: true,
          },
        ]);
      });
    });
  });
});
