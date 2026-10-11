// Both session runners flush a header and ordered completed steps. A kill
// may tear only the last line; no subsequent step starts before its flush.
import { readFileSync } from "node:fs";

export function readSessionRecord(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (err) {
    if (err.code !== "ENOENT") throw err;
  }
  const lines = readFileSync(`${path}.steps.jsonl`, "utf8").split("\n");
  lines.pop(); // An unterminated last entry is never evidence of completion.
  const header = JSON.parse(lines.shift());
  if (header.schema !== 1 || header.kind !== "session" || header.stage !== "partial")
    throw new Error("invalid session journal header");
  const steps = lines.map((line, index) => {
    const entry = JSON.parse(line);
    if (entry.index !== index || !entry.step)
      throw new Error(`invalid session journal step ${index}`);
    return entry.step;
  });
  return { ...header, steps };
}
