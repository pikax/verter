// Hermetic fake `verter-dx-baseline materialize` one-shot used by
// @verter/dx-harness materialize-client tests. It speaks the same stdin-JSON /
// stdout-JSON contract as the real binary but never requires the Rust build.
//
//   argv[2] must be "materialize" (mirrors the real CLI dispatch).
//   FAKE_MAT_FAIL=1   write a diagnostic to stderr and exit 1 (provider failure).
//   FAKE_MAT_BADARG=1 skip the "materialize" arg check assertion off (debug).
//
// On success it echoes the received `expectedVueVersion` into `verterTypesDts`
// (so a test can prove the request body crossed stdin) and returns one IDE
// artifact carrying an authoritative `sourceMap` string the client must surface
// verbatim.
import process from "node:process";

const env = process.env;

if (process.argv[2] !== "materialize") {
  process.stderr.write(`expected "materialize" subcommand, got ${process.argv[2]}\n`);
  process.exit(2);
}

let input = "";
process.stdin.setEncoding("utf-8");
process.stdin.on("data", (c) => (input += c));
process.stdin.on("end", () => {
  let req;
  try {
    req = JSON.parse(input);
  } catch {
    process.stderr.write("bad request json\n");
    process.exit(3);
    return;
  }
  if (typeof req.workspaceRoot !== "string") {
    process.stderr.write("missing workspaceRoot\n");
    process.exit(4);
    return;
  }
  if (env.FAKE_MAT_FAIL === "1") {
    process.stderr.write("materialize boom\n");
    process.exit(1);
    return;
  }
  // Log to stderr — stdout is reserved for the protocol result (as the real
  // binary does), so a test can also assert stderr capture.
  process.stderr.write(`fake materialize for ${req.workspaceRoot}\n`);

  const result = {
    ideArtifacts: [
      {
        sourceVue: `${req.workspaceRoot}/A.vue`,
        generatedPath: `${req.workspaceRoot}/A.vue.tsx`,
        sourceMapPresent: true,
        sourceMap: "SHIFTED-MAP-A",
      },
    ],
    publicApiTwins: [
      {
        sourceVue: `${req.workspaceRoot}/A.vue`,
        generatedPath: `${req.workspaceRoot}/A.vue.ts`,
        sourceMapPresent: false,
      },
    ],
    verterTypesDts: req.expectedVueVersion ?? null,
    mapAbsent: [],
    sourceMapIdentities: { [`${req.workspaceRoot}/A.vue`]: "identity-A" },
    compileErrors: [],
    tsconfigPath: `${req.workspaceRoot}/tsconfig.json`,
    synthesizedTsconfig: true,
    supportRewrites: [],
    vueVersionWarnings: [],
  };
  process.stdout.write(JSON.stringify(result) + "\n");
  process.exit(0);
});
