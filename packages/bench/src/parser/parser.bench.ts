import { bench, describe } from "vitest";
import { ValidFiles } from "../utils.js";
import {
  parseStringVerter,
  parseVerterLoose,
  parseStringVolar,
} from "./utils.js";

describe("benchmark parser", () => {
  describe.each(Object.keys(ValidFiles))("single %s", (x) => {
    const file = ValidFiles[x];
    bench("verter", () => {
      parseStringVerter(file).isTS;
    });
    bench("Volar", () => {
      parseStringVolar(file).embeddedCodes.flatMap((x) => x.embeddedCodes);
    });

    bench("verter AcornLoose", () => {
      parseVerterLoose(file).isTS;
    });
  });
});
