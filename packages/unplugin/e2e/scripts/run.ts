/**
 * @ai-generated - This orchestration script was generated with AI assistance.
 * Runs E2E tests for @verter/unplugin across bundlers.
 *
 * Usage: tsx e2e/scripts/run.ts [--bundler <name>|all] [--mode dev|build|all]
 */

import { execaCommand, type ResultPromise } from 'execa'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const e2eDir = path.resolve(__dirname, '..')
const unpluginDir = path.resolve(e2eDir, '..')

interface BundlerDef {
  name: string
  hasDev: boolean
  devPort?: number
  buildPort: number
  buildCmd: string
  devCmd?: string
  serveCmd?: string // for build-only bundlers, use sirv
}

const bundlers: BundlerDef[] = [
  {
    name: 'vite',
    hasDev: true,
    devPort: 3101,
    buildPort: 4101,
    buildCmd: 'npx vite build --config e2e/bundlers/vite/vite.config.ts',
    devCmd: 'npx vite --config e2e/bundlers/vite/vite.config.ts',
    serveCmd: 'npx vite preview --config e2e/bundlers/vite/vite.config.ts',
  },
  {
    name: 'webpack',
    hasDev: true,
    devPort: 3102,
    buildPort: 4102,
    buildCmd: 'npx webpack --config e2e/bundlers/webpack/webpack.config.js --env production',
    devCmd: 'npx webpack serve --config e2e/bundlers/webpack/webpack.config.js',
    serveCmd: 'npx sirv e2e/bundlers/webpack/dist --port 4102 --single',
  },
  {
    name: 'rspack',
    hasDev: true,
    devPort: 3103,
    buildPort: 4103,
    buildCmd: 'npx rspack build --config e2e/bundlers/rspack/rspack.config.js',
    devCmd: 'npx rspack serve --config e2e/bundlers/rspack/rspack.config.js',
    serveCmd: 'npx sirv e2e/bundlers/rspack/dist --port 4103 --single',
  },
  {
    name: 'farm',
    hasDev: true,
    devPort: 3104,
    buildPort: 4104,
    buildCmd: 'npx farm build --config e2e/bundlers/farm/farm.config.mjs',
    devCmd: 'npx farm --config e2e/bundlers/farm/farm.config.mjs',
    serveCmd: 'npx sirv e2e/bundlers/farm/dist --port 4104 --single',
  },
  {
    name: 'rollup',
    hasDev: false,
    buildPort: 4105,
    buildCmd: 'npx rollup --config e2e/bundlers/rollup/rollup.config.mjs',
    serveCmd: 'npx sirv e2e/bundlers/rollup/dist --port 4105 --single',
  },
  {
    name: 'esbuild',
    hasDev: false,
    buildPort: 4106,
    buildCmd: 'node e2e/bundlers/esbuild/build.mjs',
    serveCmd: 'npx sirv e2e/bundlers/esbuild/dist --port 4106 --single',
  },
  {
    name: 'rolldown',
    hasDev: false,
    buildPort: 4107,
    buildCmd: 'node e2e/bundlers/rolldown/build.mjs',
    serveCmd: 'npx sirv e2e/bundlers/rolldown/dist --port 4107 --single',
  },
]

// Parse CLI args
const args = process.argv.slice(2)
let selectedBundler = 'all'
let selectedMode: 'dev' | 'build' | 'all' = 'all'

for (let i = 0; i < args.length; i++) {
  if (args[i] === '--bundler' && args[i + 1]) {
    selectedBundler = args[++i]
  } else if (args[i] === '--mode' && args[i + 1]) {
    selectedMode = args[++i] as typeof selectedMode
  }
}

const selectedBundlers =
  selectedBundler === 'all'
    ? bundlers
    : bundlers.filter((b) => b.name === selectedBundler)

if (selectedBundlers.length === 0) {
  console.error(`Unknown bundler: ${selectedBundler}. Valid: ${bundlers.map((b) => b.name).join(', ')}`)
  process.exit(1)
}

// Track child processes for cleanup
const childProcesses: ResultPromise[] = []

function cleanup() {
  for (const proc of childProcesses) {
    try { proc.kill('SIGTERM') } catch {}
  }
}

process.on('SIGINT', () => { cleanup(); process.exit(1) })
process.on('SIGTERM', () => { cleanup(); process.exit(1) })

async function waitForServer(port: number, timeout = 30000): Promise<boolean> {
  const start = Date.now()
  while (Date.now() - start < timeout) {
    try {
      const response = await fetch(`http://localhost:${port}`)
      if (response.ok || response.status === 304) return true
    } catch {}
    await new Promise((r) => setTimeout(r, 500))
  }
  return false
}

async function runPlaywright(bundlerName: string): Promise<boolean> {
  try {
    await execaCommand(
      `npx playwright test --config e2e/playwright.config.ts`,
      {
        cwd: unpluginDir,
        env: { ...process.env, E2E_BUNDLER: bundlerName },
        stdio: 'inherit',
      },
    )
    return true
  } catch {
    return false
  }
}

interface Result {
  bundler: string
  mode: string
  passed: boolean
  error?: string
}

const results: Result[] = []

async function runBuildMode(bundlerDef: BundlerDef): Promise<void> {
  console.log(`\n${'='.repeat(60)}`)
  console.log(`Building: ${bundlerDef.name}`)
  console.log('='.repeat(60))

  try {
    // Build
    await execaCommand(bundlerDef.buildCmd, {
      cwd: unpluginDir,
      env: { ...process.env, NODE_ENV: 'production' },
      stdio: 'inherit',
    })
  } catch (err) {
    console.error(`Build failed for ${bundlerDef.name}`)
    results.push({ bundler: bundlerDef.name, mode: 'build', passed: false, error: 'Build failed' })
    return
  }

  // Start static server
  if (bundlerDef.serveCmd) {
    const proc = execaCommand(bundlerDef.serveCmd, {
      cwd: unpluginDir,
      stdio: 'pipe',
    })
    childProcesses.push(proc)

    const ready = await waitForServer(bundlerDef.buildPort)
    if (!ready) {
      console.error(`Server failed to start for ${bundlerDef.name}`)
      results.push({ bundler: bundlerDef.name, mode: 'build', passed: false, error: 'Server timeout' })
      try { proc.kill('SIGTERM') } catch {}
      return
    }

    const passed = await runPlaywright(bundlerDef.name)
    results.push({ bundler: bundlerDef.name, mode: 'build', passed })

    try { proc.kill('SIGTERM') } catch {}
  }
}

async function runDevMode(bundlerDef: BundlerDef): Promise<void> {
  if (!bundlerDef.hasDev || !bundlerDef.devCmd || !bundlerDef.devPort) return

  console.log(`\n${'='.repeat(60)}`)
  console.log(`Dev server: ${bundlerDef.name}`)
  console.log('='.repeat(60))

  const proc = execaCommand(bundlerDef.devCmd, {
    cwd: unpluginDir,
    stdio: 'pipe',
    env: { ...process.env, WEBPACK_SERVE: 'true' },
  })
  childProcesses.push(proc)

  const ready = await waitForServer(bundlerDef.devPort)
  if (!ready) {
    console.error(`Dev server failed for ${bundlerDef.name}`)
    results.push({ bundler: bundlerDef.name, mode: 'dev', passed: false, error: 'Server timeout' })
    try { proc.kill('SIGTERM') } catch {}
    return
  }

  const passed = await runPlaywright(bundlerDef.name)
  results.push({ bundler: bundlerDef.name, mode: 'dev', passed })

  try { proc.kill('SIGTERM') } catch {}
}

// Main execution
console.log(`\nVerter E2E Test Runner`)
console.log(`Bundlers: ${selectedBundlers.map((b) => b.name).join(', ')}`)
console.log(`Mode: ${selectedMode}\n`)

for (const bundlerDef of selectedBundlers) {
  if (selectedMode === 'all' || selectedMode === 'build') {
    await runBuildMode(bundlerDef)
  }
  if (selectedMode === 'all' || selectedMode === 'dev') {
    await runDevMode(bundlerDef)
  }
}

cleanup()

// Print summary table
console.log(`\n${'='.repeat(60)}`)
console.log('RESULTS SUMMARY')
console.log('='.repeat(60))
console.log(`${'Bundler'.padEnd(12)} ${'Mode'.padEnd(8)} ${'Status'.padEnd(10)} Error`)
console.log('-'.repeat(60))

let allPassed = true
for (const r of results) {
  const status = r.passed ? 'PASS' : 'FAIL'
  const statusColor = r.passed ? '\x1b[32m' : '\x1b[31m'
  console.log(
    `${r.bundler.padEnd(12)} ${r.mode.padEnd(8)} ${statusColor}${status}\x1b[0m${' '.repeat(6)} ${r.error || ''}`,
  )
  if (!r.passed) allPassed = false
}

console.log('-'.repeat(60))
console.log(allPassed ? '\x1b[32mAll tests passed!\x1b[0m' : '\x1b[31mSome tests failed.\x1b[0m')
process.exit(allPassed ? 0 : 1)
