// Regenerate native Vapor goldens with an explicitly supplied oracle install.
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { createRequire } = require("node:module");

if (process.argv.length !== 3) {
  throw new Error("usage: node generate.cjs <oracle-install-directory>");
}
const oracle = createRequire(path.resolve(process.argv[2], "package.json"));
const version = "3.6.0-rc.15";
for (const name of ["compiler-sfc", "compiler-vapor"]) {
  if (oracle(`@vue/${name}/package.json`).version !== version) {
    throw new Error(`@vue/${name} must be ${version}`);
  }
}
const sfc = oracle("@vue/compiler-sfc");
const vapor = oracle("@vue/compiler-vapor");
const output = path.join(__dirname, "goldens", version);
fs.mkdirSync(output, { recursive: true });
const digest = (text) => crypto.createHash("sha256").update(text).digest("hex");
const options = {
  mode: "module",
  prefixIdentifiers: true,
  isTS: false,
  comments: true,
};
const rows = fs
  .readdirSync(path.join(__dirname, "cases"))
  .filter((file) => file.endsWith(".vue"))
  .sort()
  .map((file) => {
    const filename = path.join(__dirname, "cases", file);
    const source = fs.readFileSync(filename, "utf8").replace(/\r\n/g, "\n");
    const parsed = sfc.parse(source, { filename });
    const bindings =
      parsed.descriptor.script || parsed.descriptor.scriptSetup
        ? sfc.compileScript(parsed.descriptor, { id: "loop-source" }).bindings
        : {};
    const diagnostics = [...parsed.errors];
    const result = vapor.compile(parsed.descriptor.template.content, {
      ...options,
      filename,
      bindingMetadata: bindings,
      onError: (error) => diagnostics.push(String(error)),
    });
    if (diagnostics.length) throw new Error(JSON.stringify(diagnostics));
    const code = result.code + "\n";
    fs.writeFileSync(path.join(output, file.replace(/\.vue$/, ".js")), code);
    return {
      file,
      version,
      bindings,
      diagnostics,
      sourceSha256: digest(source),
      outputSha256: digest(code),
    };
  });
fs.writeFileSync(
  path.join(__dirname, "oracle.json"),
  JSON.stringify(
    { version, options: { ...options, bindingMetadata: "compileScript" }, rows },
    null,
    2,
  ) + "\n",
);
