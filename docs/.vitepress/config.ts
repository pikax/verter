import { defineConfig } from "vitepress";
import { withMermaid } from "vitepress-plugin-mermaid";

export default withMermaid(
  defineConfig({
    title: "Verter",
    description: "Fast Next Gen Unofficial Vue Tools",

    head: [
      ["meta", { property: "og:type", content: "website" }],
      ["meta", { property: "og:title", content: "Verter" }],
      ["meta", { property: "og:description", content: "Fast Next Gen Unofficial Vue Tools" }],
      ["meta", { property: "og:url", content: "https://verterjs.dev" }],
      ["meta", { name: "twitter:card", content: "summary_large_image" }],
    ],

    themeConfig: {
      nav: [
        { text: "Guide", link: "/guide/" },
        { text: "API", link: "/api/unplugin" },
        { text: "Editor", link: "/editor/vscode" },
        {
          text: "Links",
          items: [
            { text: "Playground", link: "https://play.verterjs.dev" },
            { text: "GitHub", link: "https://github.com/pikax/verter" },
            { text: "Changelog", link: "https://github.com/pikax/verter/blob/main/CHANGELOG.md" },
          ],
        },
      ],

      sidebar: {
        "/guide/": [
          {
            text: "Introduction",
            items: [
              { text: "What is Verter?", link: "/guide/" },
              { text: "Getting Started", link: "/guide/getting-started" },
            ],
          },
          {
            text: "Bundler Integration",
            items: [
              { text: "Vite", link: "/guide/vite" },
              { text: "webpack", link: "/guide/webpack" },
              { text: "Other Bundlers", link: "/guide/other-bundlers" },
            ],
          },
          {
            text: "Features",
            items: [
              { text: "Overview", link: "/guide/features" },
              { text: "Performance", link: "/guide/performance" },
              { text: "Cross-File Optimization", link: "/guide/cross-file-optimization" },
              { text: "Linting", link: "/guide/linting" },
              { text: "Architecture", link: "/guide/architecture" },
            ],
          },
          {
            text: "Advanced",
            items: [
              {
                text: "Reactivity Classification",
                link: "/guide/advanced/reactivity-classification",
              },
              { text: "Template Compilation", link: "/guide/advanced/template-compilation" },
            ],
          },
        ],
        "/api/": [
          {
            text: "API Reference",
            items: [
              { text: "@verter/unplugin", link: "/api/unplugin" },
              { text: "@verter/native", link: "/api/native" },
              { text: "@verter/wasm", link: "/api/wasm" },
              { text: "@verter/core", link: "/api/core" },
              { text: "@verter/types", link: "/api/types" },
              { text: "@verter/component-meta", link: "/api/component-meta" },
            ],
          },
        ],
        "/editor/": [
          {
            text: "Editor Support",
            items: [
              { text: "VS Code Extension", link: "/editor/vscode" },
              { text: "LSP Features", link: "/editor/lsp-features" },
              { text: "MCP Server", link: "/editor/mcp-server" },
              { text: "Settings Reference", link: "/editor/settings" },
            ],
          },
        ],
        "/contributing/": [
          {
            text: "Contributing",
            items: [
              { text: "How to Contribute", link: "/contributing/" },
              { text: "Rust Setup", link: "/contributing/rust-setup" },
              { text: "Testing", link: "/contributing/testing" },
              { text: "CI/CD", link: "/contributing/ci-cd" },
            ],
          },
        ],
      },

      socialLinks: [{ icon: "github", link: "https://github.com/pikax/verter" }],

      editLink: {
        pattern: "https://github.com/pikax/verter/edit/main/docs/:path",
      },

      search: {
        provider: "local",
      },

      footer: {
        message: "Released under the MIT License.",
        copyright: "Copyright 2024-present Carlos Rodrigues",
      },
    },
  }),
);
