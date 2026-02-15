import * as esbuild from 'esbuild'
import path from 'path'
import { fileURLToPath } from 'url'
import vue from '@verter/unplugin/esbuild'
import fs from 'fs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const appDir = path.resolve(__dirname, '../../app')
const outDir = path.resolve(__dirname, 'dist')

// Copy index.html and adjust script src
const html = fs.readFileSync(path.resolve(appDir, 'index.html'), 'utf-8')
  .replace('./src/main.ts', './bundle.js')

fs.mkdirSync(outDir, { recursive: true })
fs.writeFileSync(path.resolve(outDir, 'index.html'), html)

await esbuild.build({
  entryPoints: [path.resolve(appDir, 'src/main.ts')],
  bundle: true,
  outfile: path.resolve(outDir, 'bundle.js'),
  format: 'esm',
  platform: 'browser',
  target: 'es2020',
  plugins: [vue()],
  define: {
    __VUE_OPTIONS_API__: 'true',
    __VUE_PROD_DEVTOOLS__: 'false',
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: 'false',
    'process.env.NODE_ENV': '"production"',
  },
  loader: {
    '.ts': 'ts',
  },
})

console.log('esbuild: Build complete')
