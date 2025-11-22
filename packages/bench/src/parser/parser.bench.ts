import { bench, describe } from "vitest";
import { ValidFiles } from "../utils.js";
import {
  parseStringVerter,
  parseVerterLoose,
  parseStringVolar,
  processVerterScript,
} from "./utils.js";

describe("benchmark parser & process", () => {
  describe("parser", () => {
    describe.each(Object.keys(ValidFiles))("single %s", (x) => {
      const file = ValidFiles[x];
      bench("Verter", () => {
        parseStringVerter(file).isTS;
      });

      bench("verter AcornLoose", () => {
        parseVerterLoose(file).isTS;
      });

      bench("Volar", () => {
        parseStringVolar(file);
      });
    });
  });

  describe("process", () => {
    describe.each(Object.keys(ValidFiles))("single %s", (x) => {
      const file = ValidFiles[x];

      bench("Verter", () => {
        processVerterScript(parseStringVerter(file));
      });

      bench("verter AcornLoose", () => {
        processVerterScript(parseVerterLoose(file));
      });

      bench("Volar", () => {
        parseStringVolar(file).embeddedCodes.flatMap((x) => x.embeddedCodes);
      });
    });
  });
});
