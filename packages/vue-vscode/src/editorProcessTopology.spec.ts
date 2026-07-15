import { describe, expect, it } from "vitest";

import {
  descendantsOf,
  localSemanticEnginesUnderVerterLsp,
  parsePosixProcessInventory,
  parseWindowsProcessInventory,
} from "../e2e/processInventory";

describe("editor process topology", () => {
  it("parses Windows CIM output without confusing Name with CommandLine", () => {
    const rows = parseWindowsProcessInventory(
      JSON.stringify([
        {
          ProcessId: 41,
          ParentProcessId: 7,
          Name: "verter-lsp.exe",
          CommandLine: '"C:\\tmp\\verter-lsp.exe" --type-provider=tsserver',
        },
        {
          ProcessId: 42,
          ParentProcessId: 41,
          Name: "node.exe",
          CommandLine: 'node "C:\\sdk\\typescript\\lib\\tsserver.js"',
        },
      ]),
    );

    expect(rows).toEqual([
      {
        pid: 41,
        parentPid: 7,
        name: "verter-lsp.exe",
        commandLine: '"C:\\tmp\\verter-lsp.exe" --type-provider=tsserver',
      },
      {
        pid: 42,
        parentPid: 41,
        name: "node.exe",
        commandLine: 'node "C:\\sdk\\typescript\\lib\\tsserver.js"',
      },
    ]);
  });

  it("walks the full descendant tree and detects managed semantic grandchildren", () => {
    const rows = parsePosixProcessInventory(`
10 1 /opt/code --extensionTestsPath=/tests
20 10 /tmp/verter-lsp --type-provider=auto
30 20 /bin/sh -c /opt/tsgo --lsp --stdio
31 30 /opt/tsgo --lsp --stdio
40 10 node /opt/typescript/lib/tsserver.js
`);

    expect(descendantsOf(rows, 20).map((row) => row.pid)).toEqual([30, 31]);
    expect(localSemanticEnginesUnderVerterLsp(rows).map((row) => row.pid)).toEqual([31]);
  });

  it("does not misreport an editor-owned tsserver sibling as an LSP child", () => {
    const rows = parsePosixProcessInventory(`
10 1 /opt/code --extensionTestsPath=/tests
20 10 /tmp/verter-lsp --type-provider=tsserver
40 10 /opt/code --ms-enable-electron-run-as-node /opt/typescript/lib/tsserver.js
`);

    expect(localSemanticEnginesUnderVerterLsp(rows)).toEqual([]);
  });

  it("detects the Code executable used as a Node host when it is below the LSP", () => {
    const rows = parsePosixProcessInventory(`
10 1 /opt/code --extensionTestsPath=/tests
20 10 /tmp/verter-lsp --type-provider=tsserver
40 20 /opt/code --ms-enable-electron-run-as-node /opt/typescript/lib/tsserver.js
`);

    expect(localSemanticEnginesUnderVerterLsp(rows).map((row) => row.pid)).toEqual([40]);
  });
});
