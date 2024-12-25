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
  });

  describe("isAsync", () => {
    it("not async", () => {
      const source = ``;
      const parsed = parser(source);

      expect(parsed.isAsync).toBe(false);
    });
  });
});
