// Regenerate from the pinned local Prettier width dependencies, never a network oracle.
const fs = require("node:fs");
const path = require("node:path");
const root = path.resolve(__dirname, "../../..");
const out = path.join(__dirname, "../src/width_data");
function write(file, data) {
  const expected = Buffer.from(data);
  if (process.argv.includes("--check")) {
    require("node:assert/strict").deepEqual(fs.readFileSync(file), expected, file);
  } else {
    fs.writeFileSync(file, expected);
  }
}
function shadow(code) {
  return code >= 0xd800 && code <= 0xdfff ? code + 0xf0000 - 0xd800 : code;
}
function translate(source) {
  source = source
    .replace(/\\u([0-9a-f]{4})/gi, (_, hex) => `\\x{${shadow(parseInt(hex, 16)).toString(16)}}`)
    .replace(/\\x([0-9a-f]{2})/gi, (_, hex) => `\\x{${hex}}`);
  let result = "",
    inClass = false;
  for (let i = 0; i < source.length; i++) {
    const c = source[i];
    if (c === "\\") {
      const next = source[++i];
      result += next === "d" ? (inClass ? "0-9" : "[0-9]") : next === "/" ? "/" : "\\" + next;
    } else if (c === "[") {
      result += inClass ? "\\[" : "[";
      inClass = true;
    } else {
      result += c;
      if (c === "]") inClass = false;
    }
  }
  return result;
}
for (const [pkg, version, file] of [
  ["emoji-regex", "8.0.0", "emoji"],
  ["ansi-regex", "5.0.1", "ansi"],
]) {
  const dir = path.join(root, "node_modules/.pnpm", `${pkg}@${version}`, "node_modules", pkg);
  write(path.join(out, `${file}.regex`), translate(require(dir)().source));
  write(
    path.join(out, `${file}.LICENSE`),
    fs.readFileSync(path.join(dir, pkg === "emoji-regex" ? "LICENSE-MIT.txt" : "license")),
  );
}
const stringWidth = require(
  path.join(root, "node_modules/.pnpm/string-width@4.2.3/node_modules/string-width"),
);
const cases = [
  "é🙂",
  "e\u0301",
  "👩‍💻",
  "👨‍👩‍👧‍👦",
  "🇵🇹",
  "1️⃣",
  "©",
  "©️",
  "\u1ab0",
  "\u200d",
  "\x1b[31ma\x1b[0m",
  "\x1b]0;title\x07X",
  "🫠",
  "\u{f0000}",
  "\u{20000}",
];
for (let c = 0; c < 0x110000; c += 997) {
  if (c < 0xd800 || c > 0xdfff) cases.push(String.fromCodePoint(c));
}
write(
  path.join(__dirname, "width-cases.tsv"),
  cases
    .map(
      (text) =>
        `${Array.from(text, (c) => c.codePointAt(0).toString(16)).join(",")}\t${stringWidth(text)}`,
    )
    .join("\n") + "\n",
);
