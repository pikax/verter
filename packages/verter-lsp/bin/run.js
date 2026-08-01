#!/usr/bin/env node
// verter-lsp launcher — resolves the platform-specific Rust server binary and
// hands it the process stdio.
//
// The child INHERITS the real stdin/stdout descriptors, so the LSP stream runs
// directly between the editor and the native server: this wrapper is not on the
// per-message path. Programmatic consumers should skip it entirely and spawn
// `require("verter-lsp").serverBinaryPath()` themselves.

"use strict";

const { runLauncherCli } = require("@verter/binary-launcher/cli");

const { launcher } = require("../index.js");

function main(argv) {
  return runLauncherCli({ launcher, argv });
}

if (require.main === module) {
  process.exit(main(process.argv.slice(2)));
}

module.exports = { main };
