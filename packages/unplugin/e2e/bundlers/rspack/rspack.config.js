const path = require('path')
const { HtmlRspackPlugin } = require('@rspack/core')
const vue = require('@verter/unplugin/rspack').default

const appDir = path.resolve(__dirname, '../../app')
const isProd = process.env.NODE_ENV === 'production'

module.exports = {
  mode: isProd ? 'production' : 'development',
  entry: path.resolve(appDir, 'src/main.ts'),
  output: {
    path: path.resolve(__dirname, 'dist'),
    filename: 'bundle.js',
    clean: true,
  },
  resolve: {
    extensions: ['.ts', '.js', '.vue', '.json'],
  },
  module: {
    rules: [
      {
        test: /\.ts$/,
        exclude: /node_modules/,
        use: {
          loader: 'builtin:swc-loader',
          options: {
            jsc: { parser: { syntax: 'typescript' } },
          },
        },
      },
      {
        test: /\.css$/,
        use: ['style-loader', 'css-loader'],
      },
      {
        test: /\.scss$/,
        use: ['style-loader', 'css-loader', 'sass-loader'],
      },
      {
        test: /\.less$/,
        use: ['style-loader', 'css-loader', 'less-loader'],
      },
    ],
  },
  plugins: [
    vue(),
    new HtmlRspackPlugin({
      template: path.resolve(appDir, 'template.html'),
    }),
  ],
  devServer: {
    port: 3103,
    hot: true,
  },
  stats: 'errors-warnings',
}
