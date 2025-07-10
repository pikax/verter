import { bench } from "vitest";

import { parse, parseOld, ValidFiles } from "./test.utils.js";

describe("benchmark parser", () => {
  describe.each(Object.keys(ValidFiles))("single %s", (x) => {
    const file = ValidFiles[x];
    bench("parseOld", () => {
      parseOld(file).render.content;
    });
    bench("parse", () => {
      parse(file).render.result;
    });
  });
});
