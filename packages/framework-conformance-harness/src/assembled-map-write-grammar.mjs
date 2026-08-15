// The assembler write-site manifest, as an exact byte grammar, and placement.
//
// Implements LAYER 1 §6.1 (classification vocabulary), §6.2 (the manifest
// W-01…W-18 and the transitions T1…T4), and §6.3 (placement derived from the
// write cursor) of `spec/assembled-map-composition-layer1.md`.
//
// Placement is DERIVED from the write grammar AS THE ASSEMBLER WRITES. It is
// never supplied as an input (§3.1), never recovered by scanning the generated
// output, and never reconstructed by concatenating code first and computing
// offsets afterwards (§6.3).

import { advance } from "./assembled-map-coordinates.mjs";

/**
 * §6.2 — `raw(s)`: the bytes of `s`, verbatim, with NO escaping of any kind
 * (Rust's `{}` for `&str`). A `"` or `\` in such a string therefore reaches the
 * generated JavaScript unescaped; that is existing behaviour and the code
 * baseline is byte-pinned.
 */
function raw(text) {
  return text;
}

/**
 * §6.2 — `dbg(s)`: Rust's `{:?}` for `&str`. Under precondition P1 (§3.5) this
 * is the COMPLETE definition; outside P1 Rust additionally applies `\u{…}`
 * escaping driven by its Unicode printability and grapheme-extended tables,
 * which layer 1 does not reproduce and defines no behaviour for — so P1 is
 * enforced at the DTO boundary rather than approximated here.
 */
function dbg(text) {
  let out = '"';
  for (const character of text) {
    switch (character) {
      case '"':
        out += '\\"';
        break;
      case "\\":
        out += "\\\\";
        break;
      case "\n":
        out += "\\n";
        break;
      case "\r":
        out += "\\r";
        break;
      case "\t":
        out += "\\t";
        break;
      default:
        out += character;
    }
  }
  return `${out}"`;
}

/** §6.2 — `dec(i)`: the decimal rendering of a `usize`, no separators or sign. */
function dec(value) {
  return String(value);
}

/**
 * §6.2 — `spec(n)` is `format_import_specifier`: for a name beginning with `_`
 * and longer than one character, `raw(n[1..]) ++ " as " ++ raw(n)`; otherwise
 * `raw(n)`.
 */
function importSpecifier(name) {
  if (name.startsWith("_") && name.length > 1) return `${raw(name.slice(1))} as ${raw(name)}`;
  return raw(name);
}

/** §6.2 — `styleId(i)`, from `render_ids`. */
function styleId(input, index) {
  const lang =
    index < input.styleLangs.length && input.styleLangs[index] !== null
      ? input.styleLangs[index]
      : "css";
  return `${raw(input.canonicalId)}?vue&type=style&index=${dec(index)}&lang.${raw(lang)}`;
}

/** §6.2 — `customId(i)`, from `render_ids`. */
function customId(input, index) {
  const type = index < input.customTypes.length ? input.customTypes[index] : "custom";
  return `${raw(input.canonicalId)}?vue&type=${raw(type)}&index=${dec(index)}`;
}

/** The write cursor of §6.3: the generated `(line, column)` of the next byte. */
class Writer {
  constructor() {
    this.code = "";
    this.cursor = { line: 0, column: 0 };
  }

  write(text) {
    if (text.length === 0) return;
    this.code += text;
    this.cursor = advance(this.cursor, text);
  }

  /** A fragment's placement is the cursor value when its first byte is written. */
  placement() {
    return { lineOffset: this.cursor.line, columnOffset: this.cursor.column };
  }
}

/**
 * §6.2 — every write `assemble_vue_main_module` performs, in execution order,
 * and §6.3 — the placement of each mapped fragment.
 *
 * @param {object} input the validated `AssembleInput`
 * @param {string|null} rewrittenScriptCode the script fragment's code AFTER both
 *   authorized rewrites (§5.1), or `null` when the script is absent
 * @returns {{ code: string, scriptPlacement: object|null, templatePlacement: object|null }}
 */
export function assembleModule(input, rewrittenScriptCode) {
  const writer = new Writer();
  const runtimeModule = input.runtimeModuleName ?? "vue"; // `R`
  const ssrId = input.ssrModuleId ?? input.canonicalId; // `S`
  const templatePresent = input.template !== null;
  const scriptPresent = input.script !== null;

  let scriptPlacement = null;
  let templatePlacement = null;

  // W-01 — for each `i` in `0..styleCount`
  for (let i = 0; i < input.styleCount; i += 1) {
    writer.write(`import "${styleId(input, i)}"\n`);
  }

  // W-02 — for each `i` in `0..customBlockCount`
  for (let i = 0; i < input.customBlockCount; i += 1) {
    writer.write(`import block${dec(i)} from "${customId(input, i)}"\n`);
  }

  // W-03
  if (input.styleCount > 0 || input.customBlockCount > 0) writer.write("\n");

  // W-04
  if (templatePresent && input.template.imports.length > 0) {
    const specifiers = input.template.imports.map(importSpecifier).join(", ");
    writer.write(`import { ${specifiers} } from "${raw(runtimeModule)}"\n`);
  }

  // W-05
  if (templatePresent && input.template.ssrImports.length > 0) {
    const specifiers = input.template.ssrImports.map(importSpecifier).join(", ");
    writer.write(`import { ${specifiers} } from "vue/server-renderer"\n`);
  }

  if (scriptPresent) {
    // T1 — into the script. W-06: the rewritten script code, byte for byte (A).
    scriptPlacement = writer.placement();
    writer.write(rewrittenScriptCode);
    // T2 — out of the script, i.e. BEFORE W-07.
    // W-07 tests the FINAL fragment bytes, so a script whose trailing
    // `export default _sfc_main;\n` was removed by pass 2 and now ends without
    // LF receives this newline.
    if (!rewrittenScriptCode.endsWith("\n")) writer.write("\n");
  } else {
    // W-08
    writer.write("const _sfc_main = {}\n");
    // W-09
    if (input.scopeId !== "") writer.write(`_sfc_main.__scopeId = "${raw(input.scopeId)}"\n`);
  }

  if (templatePresent) {
    // W-10
    writer.write("\n");
    // T3 — into the template. W-11: the template code, byte for byte (A).
    templatePlacement = writer.placement();
    writer.write(input.template.code);
    // T4 — out of the template, i.e. BEFORE W-12.
    // W-12
    if (!input.template.code.endsWith("\n")) writer.write("\n");
    // W-13 / W-13′ — a text scan of the template code; a code-shape decision
    // only, contributing no segment either way.
    if (input.template.code.includes("function ssrRender(")) {
      writer.write("_sfc_main.ssrRender = ssrRender\n");
    } else if (input.template.code.includes("function render(")) {
      writer.write("_sfc_main.render = render\n");
    }
  }

  // W-14
  for (let i = 0; i < input.customBlockCount; i += 1) {
    writer.write(`if (typeof block${dec(i)} === 'function') block${dec(i)}(_sfc_main)\n`);
  }

  // W-15
  if (!input.isProduction) writer.write(`_sfc_main.__file = ${dbg(input.canonicalId)}\n`);

  // W-16 / W-16′
  if (!input.isProduction && !input.ssr) {
    if (input.hmrStrategy === "vite") {
      writer.write("/* HMR(vite) */\nif (import.meta.hot) { import.meta.hot.accept(() => {}) }\n");
    } else if (input.hmrStrategy === "webpack") {
      writer.write("/* HMR(webpack) */\nif (module.hot) { module.hot.accept(() => {}) }\n");
    }
  }

  // W-17
  if (input.ssr) {
    writer.write(
      `import { useSSRContext as __vite_useSSRContext } from "${raw(runtimeModule)}"\n` +
        "const _sfc_setup = _sfc_main.setup\n" +
        "_sfc_main.setup = (props, ctx) => {\n" +
        "  const ssrContext = __vite_useSSRContext()\n" +
        `  ;(ssrContext.modules || (ssrContext.modules = new Set())).add(${dbg(ssrId)})\n` +
        "  return _sfc_setup ? _sfc_setup(props, ctx) : undefined\n" +
        "}\n",
    );
  }

  // W-18 — always; no trailing newline; the module's last write.
  writer.write("export default _sfc_main");

  return { code: writer.code, scriptPlacement, templatePlacement };
}

/**
 * §6.3 — a fragment segment at `(l, c)` is placed at
 * `genLine = l + lineOffset`, `genCol = (l == 0) ? c + columnOffset : c`.
 *
 * Stated for all columns so it stays total if the write grammar changes; F-c's
 * derived invariant (`columnOffset` is 0 at both T1 and T3) may be OBSERVED but
 * must not be assumed.
 */
export function placeSegment(segment, placement) {
  return {
    ...segment,
    genLine: segment.genLine + placement.lineOffset,
    genCol: segment.genLine === 0 ? segment.genCol + placement.columnOffset : segment.genCol,
  };
}
