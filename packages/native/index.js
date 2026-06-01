/* eslint-disable */
/* prettier-ignore */

// Committed thin wrapper over the NAPI-RS-generated loader.
//
// `./dist/index.js` is the napi-generated binding loader. It owns ALL
// platform detection and binary resolution: it first tries the
// co-located `dist/<binaryName>.<triple>.node`, then falls back to the
// per-platform optional-dependency package (`@verter/native-<triple>`).
// That fallback is what lets a plain `npm install` of `@verter/native`
// work when only the optional platform package ships a binary — the
// main package itself publishes no `.node`.
//
// This wrapper adds the JS-only conveniences the generated loader does
// not: string -> Buffer coercion for the payload arguments that cross
// the FFI boundary as raw bytes, and the `ComponentMetaHost` /
// `ComponentMetaSession` aliases for `MetaProject` / `MetaSession`.

const nativeBinding = require("./dist/index.js");

// ---------------------------------------------------------------------------
// JS-side string -> Buffer coercion
//
// The native binding always receives bytes (Buffer): the Rust FFI payload
// params are `Buffer` deliberately, to avoid a V8 UTF-16 -> UTF-8 re-encode.
// The JS wrapper accepts both string and Buffer for convenience — strings
// are converted to UTF-8 Buffers before crossing the FFI boundary.
// ---------------------------------------------------------------------------

function toBuffer(v) {
  return typeof v === "string" ? Buffer.from(v) : v;
}

const {
  processStyle: _processStyle,
  VerterHost,
  Workspace,
  MetaProject,
  MetaSession,
} = nativeBinding;

const ComponentMetaHost = MetaProject;
const ComponentMetaSession = MetaSession;

function processStyle(css, options) {
  return _processStyle(toBuffer(css), options);
}

const _upsert = VerterHost.prototype.upsert;
VerterHost.prototype.upsert = function (request) {
  if (typeof request.source === "string") {
    request = { ...request, source: Buffer.from(request.source) };
  }
  return _upsert.call(this, request);
};

const _compileMany = VerterHost.prototype.compileMany;
VerterHost.prototype.compileMany = function (files, options) {
  // The native binding expects each input.source as Buffer. Mirror
  // the existing upsert wrapper convention: coerce string -> Buffer
  // before crossing the FFI boundary; canonicalId is always a string.
  const coerced = files.map((f) =>
    typeof f.source === "string" ? { ...f, source: Buffer.from(f.source) } : f,
  );
  return _compileMany.call(this, coerced, options);
};

const _applyBlockOverrides = VerterHost.prototype.applyBlockOverrides;
VerterHost.prototype.applyBlockOverrides = function (request) {
  if (request.overrides && request.overrides.some((o) => typeof o.code === "string")) {
    request = {
      ...request,
      overrides: request.overrides.map((o) =>
        typeof o.code === "string" ? { ...o, code: Buffer.from(o.code) } : o,
      ),
    };
  }
  return _applyBlockOverrides.call(this, request);
};

if (MetaProject) {
  const _upsertBase = MetaProject.prototype.upsertBase;
  MetaProject.prototype.upsertBase = function (canonicalId, source) {
    return _upsertBase.call(this, canonicalId, toBuffer(source));
  };
}

if (MetaSession) {
  const _sessionUpsert = MetaSession.prototype.upsert;
  MetaSession.prototype.upsert = function (canonicalId, source) {
    return _sessionUpsert.call(this, canonicalId, toBuffer(source));
  };
}

module.exports.processStyle = processStyle;
module.exports.VerterHost = VerterHost;
module.exports.Workspace = Workspace;
module.exports.ComponentMetaHost = ComponentMetaHost;
module.exports.ComponentMetaSession = ComponentMetaSession;
module.exports.MetaProject = MetaProject;
module.exports.MetaSession = MetaSession;
