import { createContext, parseGeneric } from "./parser";

describe("parser", () => {
  describe("createContext", () => {
    it("should create context", () => {
      const context = createContext(
        `<template><div></div></template><script></script>`
      );
      expect(context).toMatchObject({
        filename: "temp.vue",
        isSetup: false,
        isAsync: false,
        generic: undefined,
      });
    });

    it("should add empty script", () => {
      const context = createContext(`<template><div></div></template>`);
      expect(context.sfc.script).toMatchObject({
        content: "",
      });
      expect(context.s.toString()).toContain("<script></script>");
    });

    it("should resolve the generic", () => {
      const context = createContext(`<script generic="T"></script>`);
      expect(context.generic).toMatchObject({
        source: "T",
        names: ["T"],
        sanitisedNames: ["___VERTER___TS_T"],
        declaration: "___VERTER___TS_T = any",
      });
    });

    it("should resolve the script setup", () => {
      const context = createContext(`<script setup></script>`);
      expect(context.isSetup).toBe(true);
    });

    it("should handle multiple scripts", () => {
      const context = createContext(`<script></script><script setup></script>`);

      expect(context.isSetup).toBe(true);
    });

    it.todo("should pass options to the parser", () => {});

    it("should be async", () => {
      const context = createContext(
        `<script setup>await Promise.resolve()</script>`
      );
      expect(context.isAsync).toBe(true);
    });
  });

  describe("parseGeneric", () => {
    it("should resolve generic", () => {
      expect(parseGeneric('T extends "foo" | "bar"', "__TS__")).toMatchObject({
        source: 'T extends "foo" | "bar"',
        names: ["T"],

        sanitisedNames: ["__TS__T"],

        declaration: '__TS__T extends "foo" | "bar" = any',
      });
    });

    it("should be undefined on empty string", () => {
      expect(parseGeneric("", "__TS__")).toBeUndefined();
    });

    it("undefinde if attribute is not a string", () => {
      expect(parseGeneric(true, "__TS__")).toBeUndefined();
    });
  });
});
