import path from 'path'
import fs from 'fs'
import { fileURLToPath } from 'url'
import vue from '@verter/unplugin/rollup'
import resolve from '@rollup/plugin-node-resolve'
import commonjs from '@rollup/plugin-commonjs'
import postcss from 'rollup-plugin-postcss'
import replace from '@rollup/plugin-replace'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const appDir = path.resolve(__dirname, '../../app')

export default {
  input: path.resolve(appDir, 'src/main.ts'),
  output: {
    dir: path.resolve(__dirname, 'dist'),
    format: 'es',
    entryFileNames: 'bundle.js',
  },
  plugins: [
    replace({
      preventAssignment: true,
      values: {
        __VUE_OPTIONS_API__: 'true',
        __VUE_PROD_DEVTOOLS__: 'false',
        __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: 'false',
        'process.env.NODE_ENV': JSON.stringify('production'),
      },
    }),
    vue(),
    resolve({
      extensions: ['.ts', '.js', '.vue', '.json'],
      browser: true,
    }),
    commonjs(),
    postcss({
      inject: true,
      extensions: ['.css', '.scss', '.less'],
    }),
    // Copy index.html with corrected script src
    {
      name: 'copy-html',
      closeBundle() {
        const outDir = path.resolve(__dirname, 'dist')
        const html = fs.readFileSync(path.resolve(appDir, 'index.html'), 'utf-8')
          .replace('./src/main.ts', './bundle.js')
        fs.mkdirSync(outDir, { recursive: true })
        fs.writeFileSync(path.resolve(outDir, 'index.html'), html)
      },
    },
  ],
  external: [],
  onwarn(warning, warn) {
    // Suppress circular dependency warnings from Vue
    if (warning.code === 'CIRCULAR_DEPENDENCY') return
    warn(warning)
  },
}
