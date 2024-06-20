import { createContext, parseGenerics } from "./parser";

describe("parser", () => {
  describe("createContext", () => {
    it("should create context", () => {
      const context = createContext(
        `<template><div></div></template><script></script>`
      );
      expect(context).toMatchObject({
        filename: "temp.vue",
        id: "temp.vue",
        isSetup: false,
        isAsync: false,
        generic: undefined,
      });
    });

    it("should add empty script", () => {
      const context = createContext(`<template><div></div></template>`);
      expect(context.script).toMatchObject({
        content: "",
      });
      expect(context.s.toString()).toContain("<script></script>");
    });

    it("should resolve the generic", () => {
      const context = createContext(`<script generic="T"></script>`);
      expect(context.generic).toBe("T");
    });

    it("should resolve the script setup", () => {
      const context = createContext(`<script setup></script>`);
      expect(context.isSetup).toBe(true);
    });

    it("should handle multiple scripts", () => {
      const context = createContext(`<script></script><script setup></script>`);
      expect(context.script).toMatchObject({
        content: "",
      });
      expect(context.isSetup).toBe(true);
      expect(context.script).toBe(context.sfc.descriptor.scriptSetup);
    });

    it.todo("should pass options to the parser", () => {});
  });
});
