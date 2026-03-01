#!/usr/bin/env node
// verter-tsc launcher — finds and spawns the platform-specific Rust binary.

'use strict'

const { spawnSync } = require('child_process')
const path = require('path')
const fs = require('fs')

/**
 * Find the verter-tsc binary:
 * 1. Platform-specific npm package (e.g. @verter/tsc-win32-x64-msvc)
 * 2. Local debug build (for development)
 * 3. PATH fallback
 */
function findBinary() {
  const platform = process.platform
  const arch = process.arch

  // Map Node.js platform/arch to the npm package name suffix.
  const platformMap = {
    darwin: { arm64: 'darwin-arm64', x64: 'darwin-x64' },
    linux: { x64: 'linux-x64-gnu', arm64: 'linux-arm64-gnu' },
    win32: { x64: 'win32-x64-msvc' },
  }

  const suffix = platformMap[platform]?.[arch]
  if (suffix) {
    try {
      const pkg = `@verter/tsc-${suffix}`
      // The binary lives at the root of the platform package.
      const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`))
      const binName = platform === 'win32' ? 'verter-tsc.exe' : 'verter-tsc'
      const candidate = path.join(pkgDir, binName)
      if (fs.existsSync(candidate)) return candidate
    } catch {
      // Platform package not installed — fall through to dev build.
    }
  }

  // Development: use local debug build from Cargo workspace.
  const devBinName = platform === 'win32' ? 'verter-tsc.exe' : 'verter-tsc'
  const devBuild = path.join(
    __dirname,
    '..',
    '..',
    '..',
    'target',
    'debug',
    devBinName,
  )
  if (fs.existsSync(devBuild)) return devBuild

  // Release build fallback.
  const releaseBuild = path.join(
    __dirname,
    '..',
    '..',
    '..',
    'target',
    'release',
    devBinName,
  )
  if (fs.existsSync(releaseBuild)) return releaseBuild

  // Last resort: look on PATH.
  return platform === 'win32' ? 'verter-tsc.exe' : 'verter-tsc'
}

const binary = findBinary()
const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' })

if (result.error) {
  process.stderr.write(
    `verter-tsc: failed to start binary '${binary}': ${result.error.message}\n`,
  )
  process.exit(2)
}

process.exit(result.status ?? 1)
