// Both session runners flush a header and ordered completed steps. A kill
// may tear only the last line; no subsequent step starts before its flush.
import { readFileSync } from "node:fs";

// `read` reads one file's text (the run's own reader, for a packed or copied run).
export function readSessionRecord(path, read = (p) => readFileSync(p, "utf8")) {
  try {
    return JSON.parse(read(path));
  } catch (err) {
    if (err.code !== "ENOENT") throw err;
  }
  const lines = read(`${path}.steps.jsonl`).split("\n");
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
