// Optional native-checker oracle for the provider-free Rust publication tests.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { version } from "typescript";
import { API } from "typescript/unstable/sync";

assert.equal(version, "7.0.2");
const dir = mkdtempSync(join(tmpdir(), "verter-interface-identity-")).replaceAll("\\", "/");
const fixture = readFileSync(new URL("./generic_interface_identity.ts", import.meta.url), "utf8");
const source =
  fixture +
  `
export type Direct = State<string>;
export type NestedClosed = Nested<string>;
export type NonemptyClosed = Nonempty<string>;
export type OrdinaryClosed = Ordinary;
export type OperatorsClosed = Operators<string>;
export type StructurallyEqual = Equivalent<string>;
export type StructurallyNestedEqual = NestedEquivalent<string>;
export type KeysClosed = keyof Child<string>;
export type ChangedArgument = State<number>;
`;
try {
  writeFileSync(`${dir}/probe.ts`, source);
  for (const strictNullChecks of [false, true]) {
    for (const noImplicitAny of [false, true]) {
      const config = `${dir}/tsconfig.json`;
      writeFileSync(
        config,
        JSON.stringify({
          compilerOptions: {
            strict: true,
            strictNullChecks,
            noImplicitAny,
            noEmit: true,
            types: [],
          },
          files: ["probe.ts"],
        }),
      );
      const api = new API({ cwd: dir });
      try {
        const snapshot = api.updateSnapshot({ openProjects: [config] });
        try {
          const project = snapshot.getProject(config);
          assert.deepEqual(project.program.getSemanticDiagnostics(), []);
          const typeOf = (name) => {
            const at = source.indexOf(`type ${name} =`) + "type ".length;
            const type = project.checker.getTypeAtPosition(`${dir}/probe.ts`, at);
            assert.ok(type, name);
            return type;
          };
          const print = (name) =>
            project.checker.typeToString(typeOf(name), undefined, 1 | (1 << 23));
          const answers = Object.fromEntries(
            [
              "Direct",
              "NestedClosed",
              "NonemptyClosed",
              "OrdinaryClosed",
              "OperatorsClosed",
              "StructurallyEqual",
              "StructurallyNestedEqual",
              "KeysClosed",
              "ChangedArgument",
            ].map((name) => [name, print(name)]),
          );
          assert.match(answers.Direct, /Empty<string>/);
          assert.equal(answers.NestedClosed.match(/Empty<string>/g)?.length, 2);
          assert.match(answers.NonemptyClosed, /Full<string>/);
          assert.match(answers.OrdinaryClosed, /Plain/);
          assert.match(answers.OperatorsClosed, /Empty<string>/);
          assert.equal(answers.StructurallyEqual, "true");
          assert.equal(answers.StructurallyNestedEqual, "true");
          // keyof retains its authored print origin; its semantic constituents
          // independently prove the inherited and own keys.
          const keys = typeOf("KeysClosed");
          assert.ok(keys.isUnionType());
          assert.deepEqual(
            keys
              .getTypes()
              .map((key) => {
                assert.ok(key.isStringLiteralType());
                return key.value;
              })
              .sort(),
            ["own", "value"],
          );
          assert.match(answers.ChangedArgument, /Empty<number>/);
          console.log(
            JSON.stringify({ strictNullChecks, noImplicitAny, diagnostics: [], answers }),
          );
        } finally {
          snapshot.dispose();
        }
      } finally {
        api.close();
      }
    }
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}
