import { defineConfig } from "vitepress";
import { withMermaid } from "vitepress-plugin-mermaid";
import { readdirSync, readFileSync } from "node:fs";

// Sidebar sections load from per-section fragments (sidebar/<section>.json):
// a docs node registers its pages by adding its own fragment instead of
// editing this file.
const sidebarDir = new URL("./sidebar/", import.meta.url);
const sidebar = Object.fromEntries(
  readdirSync(sidebarDir)
    .sort()
    .filter((name) => name.endsWith(".json"))
    .map((name) => [
      `/${name.slice(0, -5)}/`,
      JSON.parse(readFileSync(new URL(name, sidebarDir), "utf8")),
    ]),
);

export default withMermaid(
  defineConfig({
    title: "Verter",
    description: "Fast Next Gen Unofficial Vue Tools",

    // `docs/arch/**` is internal design material — design docs, plans, deferral
    // ledgers and memos written for contributors reading the repository, not
    // pages of the public site. They live under `docs/` so they sit beside the
    // code they describe, but they are not built or published. Link to them by
    // their GitHub URL from a published page, never by site path.
    srcExclude: ["arch/**"],

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

      sidebar,

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
