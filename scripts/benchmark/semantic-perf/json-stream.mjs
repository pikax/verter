// Write a JSON document without building it as one string.
//
// A whole run's record (every invocation of every section) can exceed the
// longest string a JavaScript engine allows, so `JSON.stringify(run)` throws
// before anything is written. This writer walks the value and flushes the
// text in bounded chunks; its output is byte for byte what
// `JSON.stringify(value, null, indent)` produces for plain JSON data.

import { closeSync, openSync, renameSync, writeSync } from "node:fs";

const FLUSH_AT = 1 << 20;

/** Write `value` as JSON to `path` (aside, then renamed: a reader never sees a torn file). */
export function writeJsonFile(path, value, indent = 1) {
  const aside = `${path}.tmp`;
  const fd = openSync(aside, "w");
  try {
    let pending = [];
    let size = 0;
    const emit = (text) => {
      pending.push(text);
      size += text.length;
      if (size >= FLUSH_AT) flush();
    };
    const flush = () => {
      if (pending.length) writeSync(fd, pending.join(""));
      pending = [];
      size = 0;
    };
    writeValue(value, "", emit, " ".repeat(indent));
    flush();
  } finally {
    closeSync(fd);
  }
  renameSync(aside, path);
}

/** JSON.stringify's view of a value: its toJSON, if any. */
const jsonView = (value, key) =>
  value !== null && typeof value === "object" && typeof value.toJSON === "function"
    ? value.toJSON(key)
    : value;

/** Whether JSON.stringify omits this value as an object member. */
const omitted = (value) =>
  value === undefined || typeof value === "function" || typeof value === "symbol";

function writeValue(value, pad, emit, step) {
  if (value === null || typeof value !== "object") {
    emit(JSON.stringify(value) ?? "null");
    return;
  }
  const inner = pad + step;
  if (Array.isArray(value)) {
    if (!value.length) return emit("[]");
    emit("[");
    value.forEach((item, i) => {
      emit(i ? `,\n${inner}` : `\n${inner}`);
      const v = jsonView(item, String(i));
      writeValue(omitted(v) ? null : v, inner, emit, step);
    });
    emit(`\n${pad}]`);
    return;
  }
  let first = true;
  for (const key of Object.keys(value)) {
    const v = jsonView(value[key], key);
    if (omitted(v)) continue;
    emit(first ? `{\n${inner}` : `,\n${inner}`);
    first = false;
    emit(`${JSON.stringify(key)}: `);
    writeValue(v, inner, emit, step);
  }
  emit(first ? "{}" : `\n${pad}}`);
}
