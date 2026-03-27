const path = require("path");
const HtmlWebpackPlugin = require("html-webpack-plugin");
const vue = require("@verter/unplugin/webpack").default;

const appDir = path.resolve(__dirname, "../../app");
const isProd = process.env.NODE_ENV === "production";
const isDevServer = process.env.WEBPACK_SERVE === "true";

module.exports = {
  mode: isProd ? "production" : "development",
  entry: path.resolve(appDir, "src/main.ts"),
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bundle.js",
    clean: true,
  },
  resolve: {
    extensions: [".ts", ".js", ".vue", ".json"],
  },
  module: {
    rules: [
      {
        test: /\.ts$/,
        exclude: /node_modules/,
        use: {
          loader: "esbuild-loader",
          options: { target: "es2020" },
        },
      },
      {
        test: /\.css$/,
        use: ["style-loader", "css-loader"],
      },
      {
        test: /\.scss$/,
        use: ["style-loader", "css-loader", "sass-loader"],
      },
      {
        test: /\.less$/,
        use: ["style-loader", "css-loader", "less-loader"],
      },
    ],
  },
  plugins: [
    vue(),
    new HtmlWebpackPlugin({
      template: path.resolve(appDir, "template.html"),
    }),
  ],
  devServer: {
    port: 3102,
    hot: true,
    static: false,
  },
  stats: "errors-warnings",
};
