#!/usr/bin/env node
/**
 * STP5 fixture content mapper.
 *
 * A minimal JSON-RPC 2.0 server over stdio (LSP base-protocol framing:
 * `Content-Length` headers) speaking the TypeScript content-mapper protocol of
 * microsoft/typescript-go#4712 as amended by microsoft/TypeScript#63936:
 * `initialize`, `openProject`, `transform`, `closeProject`. TypeScript sends
 * every request; this server never sends a request or a notification.
 *
 * It maps a tiny line language (`.stp5`) to TypeScript:
 *
 *   # comment            dropped (no virtual text)
 *   @expect / @ignore    a diagnostic directive covering the next statement
 *   call +               `add_op();`, an Alias span onto `+`
 *   !anything            a mapper (parse) diagnostic over the line
 *   anything else        copied verbatim, followed by a synthesized `;\n`
 *
 * A synthesized header precedes the output, so virtual and original offsets
 * never coincide. Every position is emitted in the negotiated encoding.
 *
 * Behaviour switches arrive as argv (initialize precedes openProject, so the
 * encoding choice cannot come from the tsconfig options):
 *
 *   --encoding=utf-8|utf-16    the encoding to choose from the offered set
 *   --twin=<name>              a deliberately non-conforming reply (see TWINS)
 *   --log=<file>               append every request as one JSON line
 *
 * Dynamic configuration: the mapper reads `stp5.config.json` beside the
 * project's tsconfig (outside the options TypeScript passes), so it returns a
 * `configIdentity` fingerprint of that file and lists it in `watchedFiles`.
 */

import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const DIAGNOSTIC_SOURCE = "stp5-fixture";
export const UNUSED_EXPECT_CODE = 9002;
export const MAPPER_PARSE_CODE = 9001;
export const OPTION_DIAGNOSTIC_CODE = 9100;
export const HEADER = "// stp5 fixture mapper output\n";
/** A header with a type error in fully synthesized (unmapped) text. */
export const SYNTHESIZED_ERROR_HEADER =
  '// stp5 fixture mapper output\nconst __stp5Synthesized: number = "x";\n';

export const TWINS = Object.freeze([
  // A conforming static-config mapper: no configIdentity, no watchedFiles.
  "static-config",
  // openProject omits configIdentity but still lists watchedFiles. Under a
  // dynamicConfig manifest the identity is missing; under a static one the
  // watched files are not allowed.
  "omit-config-identity",
  // diagnosticSource names TypeScript itself.
  "reserved-diagnostic-source",
  // initialize chooses an encoding TypeScript did not offer.
  "unoffered-encoding",
  // the transform result has no `extension`.
  "omit-extension",
  // Expect directives omit the index while two unused diagnostics are declared.
  "ambiguous-unused-index",
  // `call +` is mapped as an Atom instead of an Alias.
  "atom-call",
  // the synthesized header carries a type error with no original location.
  "synthesized-error",
  // Control: the process exits on initialize. Its refusal reason must differ
  // from the reserved-source and unoffered-encoding twins', so a crash can
  // never stand in for a protocol refusal.
  "exit-on-initialize",
]);

/** Length of `text` in the negotiated encoding. */
export function encodedLength(text, encoding) {
  return encoding === "utf-8" ? Buffer.byteLength(text, "utf8") : text.length;
}

/**
 * Transform one `.stp5` document. Pure: exported so the in-process spec can
 * check the fixture's own output against the STP5 wire validators without a
 * TypeScript host.
 */
export function transformStp5(
  content,
  encoding,
  { omitExtension = false, ambiguous = false, atomCall = false, synthesizedError = false } = {},
) {
  const at = (text, index) => encodedLength(text.slice(0, index), encoding);
  let virtual = synthesizedError ? SYNTHESIZED_ERROR_HEADER : HEADER;
  const mappings = [];
  const directives = [];
  const diagnostics = [];
  let pending = null;
  const lineRe = /[^\r\n]*(\r\n|\n|$)/g;
  let match;
  while ((match = lineRe.exec(content)) && match[0].length > 0) {
    const start = match.index;
    const line = match[0].replace(/\r?\n$/, "");
    lineRe.lastIndex = start + match[0].length;
    const trimmed = line.trim();
    if (trimmed.length === 0 || trimmed.startsWith("#")) continue;
    if (trimmed === "@expect" || trimmed === "@ignore") {
      const offset = start + line.indexOf(trimmed);
      pending = {
        originalStart: at(content, offset),
        originalLength: encodedLength(trimmed, encoding),
        policy: trimmed === "@expect" ? 1 : 0,
      };
      continue;
    }
    if (trimmed.startsWith("!")) {
      const offset = start + line.indexOf(trimmed);
      diagnostics.push({
        messageText: `Unrecognized stp5 statement '${trimmed.slice(1)}'.`,
        start: at(content, offset),
        length: encodedLength(trimmed, encoding),
        code: MAPPER_PARSE_CODE,
      });
      continue;
    }
    const virtualStart = encodedLength(virtual, encoding);
    if (trimmed === "call +") {
      const plus = start + line.indexOf("+");
      virtual += "add_op();\n";
      mappings.push([
        virtualStart,
        encodedLength("add_op", encoding),
        at(content, plus),
        1,
        atomCall ? 1 : 2,
      ]);
    } else {
      virtual += `${line};\n`;
      mappings.push([
        virtualStart,
        encodedLength(line, encoding),
        at(content, start),
        encodedLength(line, encoding),
        0,
      ]);
    }
    if (pending) {
      const virtualEnd = encodedLength(virtual, encoding);
      const directive = [
        pending.originalStart,
        pending.originalLength,
        virtualStart,
        virtualEnd,
        pending.policy,
      ];
      // The index is optional while exactly one unused diagnostic is declared;
      // the first Expect carries it explicitly, later ones rely on the default.
      if (pending.policy === 1 && !ambiguous && !directives.some((row) => row[4] === 1)) {
        directive.push(0);
      }
      directives.push(directive);
      pending = null;
    }
  }
  const unusedExpectDirectiveDiagnostics = [
    { code: UNUSED_EXPECT_CODE, messageText: "Unused '@expect' directive." },
  ];
  if (ambiguous) {
    unusedExpectDirectiveDiagnostics.push({
      code: UNUSED_EXPECT_CODE + 1,
      messageText: "Second unused text.",
    });
  }
  const result = {
    text: virtual,
    extension: ".ts",
    mappings,
    diagnosticDirectives: { unusedExpectDirectiveDiagnostics, directives },
  };
  if (omitExtension) delete result.extension;
  if (diagnostics.length > 0) result.diagnostics = diagnostics;
  return result;
}

/** `optionDiagnostics` for the contentMappers entry's `options` object. */
export function optionDiagnosticsFor(options) {
  const diagnostics = [];
  if (options && typeof options === "object" && Array.isArray(options.plugins)) {
    options.plugins.forEach((plugin, index) => {
      if (plugin && typeof plugin.name !== "string") {
        diagnostics.push({
          path: ["plugins", index, "name"],
          messageText: "Plugin option 'name' must be a string.",
          code: OPTION_DIAGNOSTIC_CODE,
        });
      }
    });
  }
  return diagnostics;
}

/**
 * The request handler. TypeScript-shaped requests in, protocol replies out;
 * a thrown error becomes a JSON-RPC error response.
 */
export function createMapperHandler({ encoding = "utf-16", twin = null } = {}) {
  const projects = new Map();
  let negotiated = null;
  return {
    get projects() {
      return projects;
    },
    handle(method, params) {
      switch (method) {
        case "initialize": {
          if (twin === "exit-on-initialize") process.exit(3);
          const offered = params?.positionEncodings || [];
          if (twin === "unoffered-encoding") {
            negotiated = "utf-32";
          } else if (offered.includes(encoding)) {
            negotiated = encoding;
          } else {
            // Never fall back silently: a run that asked for an encoding must
            // either negotiate it or fail where the harness can see it.
            throw Object.assign(
              new Error(
                `requested encoding ${encoding} was not offered: ${JSON.stringify(offered)}`,
              ),
              { code: -32602 },
            );
          }
          return {
            positionEncoding: negotiated,
            diagnosticSource: twin === "reserved-diagnostic-source" ? "tsc" : DIAGNOSTIC_SOURCE,
          };
        }
        case "openProject": {
          const configDir = params.configFileName
            ? path.dirname(params.configFileName)
            : process.cwd();
          const dynamicFile = path.join(configDir, "stp5.config.json");
          const dynamicText = fs.existsSync(dynamicFile)
            ? fs.readFileSync(dynamicFile, "utf8")
            : "";
          projects.set(params.projectHandle, {
            options: params.options ?? null,
            compilerOptions: params.compilerOptions ?? null,
            dynamicText,
          });
          const result = { optionDiagnostics: optionDiagnosticsFor(params.options) };
          if (twin !== "static-config") result.watchedFiles = [dynamicFile];
          if (twin !== "static-config" && twin !== "omit-config-identity") {
            result.configIdentity = createHash("sha256").update(dynamicText).digest("hex");
          }
          return result;
        }
        case "transform": {
          if (!projects.has(params.projectHandle)) {
            throw Object.assign(new Error(`unknown projectHandle ${params.projectHandle}`), {
              code: -32602,
            });
          }
          return transformStp5(params.content, negotiated, {
            omitExtension: twin === "omit-extension",
            ambiguous: twin === "ambiguous-unused-index",
            atomCall: twin === "atom-call",
            synthesizedError: twin === "synthesized-error",
          });
        }
        case "closeProject": {
          projects.delete(params.projectHandle);
          return null;
        }
        default:
          throw Object.assign(new Error(`method not found: ${method}`), { code: -32601 });
      }
    },
  };
}

/**
 * An incremental reader of LSP base-protocol frames (`Content-Length`
 * headers): feed it chunks, it calls `onMessage` with each parsed body.
 */
export function createFrameReader(onMessage) {
  let buffer = Buffer.alloc(0);
  return (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const header = buffer.subarray(0, headerEnd).toString("ascii");
      const lengthMatch = /Content-Length:\s*(\d+)/i.exec(header);
      if (!lengthMatch) {
        buffer = buffer.subarray(headerEnd + 4);
        continue;
      }
      const length = Number(lengthMatch[1]);
      if (buffer.length < headerEnd + 4 + length) return;
      const body = buffer.subarray(headerEnd + 4, headerEnd + 4 + length).toString("utf8");
      buffer = buffer.subarray(headerEnd + 4 + length);
      onMessage(JSON.parse(body));
    }
  };
}

/** Write one LSP base-protocol frame. */
export function writeFrame(output, message) {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  output.write(`Content-Length: ${body.length}\r\n\r\n`);
  output.write(body);
}

/** Frame and serve JSON-RPC over the given streams until input ends. */
export function startServer({
  input = process.stdin,
  output = process.stdout,
  handler = createMapperHandler(),
  onRequest = () => {},
  onResponse = () => {},
  onEnd = () => process.exit(0),
} = {}) {
  const reply = (method, message) => {
    onResponse({ response: method, ...message });
    writeFrame(output, { jsonrpc: "2.0", ...message });
  };
  const dispatch = (message) => {
    if (message.method === undefined) return;
    onRequest({ method: message.method, params: message.params ?? null, id: message.id ?? null });
    if (message.id === undefined || message.id === null) return;
    try {
      const result = handler.handle(message.method, message.params);
      reply(message.method, { id: message.id, result: result ?? null });
    } catch (error) {
      reply(message.method, {
        id: message.id,
        error: { code: error.code ?? -32603, message: String(error.message || error) },
      });
    }
  };
  input.on("data", createFrameReader(dispatch));
  // TypeScript closing the pipe ends the mapper; a lingering process would
  // hold its working directory open.
  input.on("end", onEnd);
}

function arg(name) {
  const prefix = `--${name}=`;
  const hit = process.argv.find((value) => value.startsWith(prefix));
  return hit ? hit.slice(prefix.length) : null;
}

function isMainModule() {
  try {
    return (
      process.argv[1] !== undefined &&
      fs.realpathSync(path.resolve(process.argv[1])) ===
        fs.realpathSync(fileURLToPath(import.meta.url))
    );
  } catch {
    return false;
  }
}

if (isMainModule()) {
  const logFile = arg("log");
  const log = (entry) => {
    if (logFile) fs.appendFileSync(logFile, `${JSON.stringify(entry)}\n`);
  };
  startServer({
    handler: createMapperHandler({ encoding: arg("encoding") || "utf-16", twin: arg("twin") }),
    onRequest: log,
    onResponse: log,
  });
}
