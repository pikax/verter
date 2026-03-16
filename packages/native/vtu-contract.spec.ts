import { describe, expect, it } from "vitest";
import { dirname, join, resolve } from "node:path";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const fixtureNodeModules = resolve(
  here,
  "..",
  "..",
  ".integration-tests",
  "repos",
  "ant-design-vue",
  "node_modules",
);
const fixtureAvailable = existsSync(join(fixtureNodeModules, "vue"));

class FakeNode {
  nodeType: number;
  tagName: string;
  childNodes: FakeNode[] = [];
  parentNode: FakeNode | null = null;
  attributes: Record<string, string> = {};
  textContent = "";
  ownerDocument: FakeDocument;

  constructor(nodeType: number, tagName: string, ownerDocument: FakeDocument) {
    this.nodeType = nodeType;
    this.tagName = tagName.toUpperCase();
    this.ownerDocument = ownerDocument;
  }

  appendChild(node: FakeNode) {
    node.parentNode = this;
    this.childNodes.push(node);
    return node;
  }

  insertBefore(node: FakeNode, anchor: FakeNode | null) {
    node.parentNode = this;
    if (!anchor) {
      this.childNodes.push(node);
      return node;
    }
    const anchorIndex = this.childNodes.indexOf(anchor);
    if (anchorIndex === -1) {
      this.childNodes.push(node);
    } else {
      this.childNodes.splice(anchorIndex, 0, node);
    }
    return node;
  }

  removeChild(node: FakeNode) {
    const index = this.childNodes.indexOf(node);
    if (index >= 0) {
      this.childNodes.splice(index, 1);
      node.parentNode = null;
    }
    return node;
  }

  setAttribute(name: string, value: string) {
    this.attributes[name] = String(value);
  }

  removeAttribute(name: string) {
    delete this.attributes[name];
  }

  addEventListener() {}

  removeEventListener() {}

  querySelector() {
    return null;
  }

  querySelectorAll() {
    return [];
  }

  get firstChild() {
    return this.childNodes[0] ?? null;
  }

  get nextSibling() {
    if (!this.parentNode) {
      return null;
    }
    const siblings = this.parentNode.childNodes;
    const index = siblings.indexOf(this);
    return siblings[index + 1] ?? null;
  }
}

interface FakeDocument {
  createElement(tagName: string): FakeNode;
  createElementNS(namespace: string, tagName: string): FakeNode;
  createTextNode(text: string): FakeNode;
  createComment(text: string): FakeNode;
  body: FakeNode;
}

const fakeDocument: FakeDocument = {
  createElement(tagName) {
    return new FakeNode(1, tagName, fakeDocument);
  },
  createElementNS(_namespace, tagName) {
    return new FakeNode(1, tagName, fakeDocument);
  },
  createTextNode(text) {
    const node = new FakeNode(3, "", fakeDocument);
    node.textContent = String(text);
    return node;
  },
  createComment(text) {
    const node = new FakeNode(8, "", fakeDocument);
    node.textContent = String(text);
    return node;
  },
  body: null as unknown as FakeNode,
};
fakeDocument.body = new FakeNode(1, "body", fakeDocument);

(globalThis as any).document = fakeDocument;
(globalThis as any).window = { document: fakeDocument };
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { userAgent: "node" },
});
(globalThis as any).Node = FakeNode;
(globalThis as any).Element = FakeNode;
(globalThis as any).HTMLElement = FakeNode;
(globalThis as any).SVGElement = FakeNode;

describe.skipIf(!fixtureAvailable)("VTU wrapper.vm contract", () => {
  // Lazy-load from integration test fixture — only runs when fixtureAvailable is true
  const vue: typeof import("vue") = fixtureAvailable
    ? require(join(fixtureNodeModules, "vue"))
    : (undefined as any);
  const { mount } = fixtureAvailable
    ? (require(join(fixtureNodeModules, "@vue/test-utils")) as typeof import("@vue/test-utils"))
    : ({ mount: undefined as any } as any);
  const { parse, compileScript } = fixtureAvailable
    ? (require(join(fixtureNodeModules, "@vue/compiler-sfc")) as typeof import("@vue/compiler-sfc"))
    : ({ parse: undefined as any, compileScript: undefined as any } as any);

  function compileScriptSetupComponent(scriptSetup: string) {
    const source = `<script setup>${scriptSetup}</script>`;
    const { descriptor } = parse(source, { filename: "VmContract.vue" });
    const compiled = compileScript(descriptor, { id: "vtu-contract" }).content;
    const commonJs = compiled.replace("export default", "module.exports.default =");
    const module = { exports: {} as { default?: any } };
    new Function("module", "exports", commonJs)(module, module.exports);
    const component = module.exports.default;
    component.render = () => vue.h("div");
    return component;
  }

  it("exposes internal script setup bindings with and without defineExpose", () => {
    const closedComponent = compileScriptSetupComponent(`
const count = 1
const hidden = 2
`);
    const closedWrapper = mount(closedComponent);
    expect((closedWrapper.vm as any).count).toBe(1);
    expect((closedWrapper.vm as any).hidden).toBe(2);

    const exposedComponent = compileScriptSetupComponent(`
const count = 1
const hidden = 2
defineExpose({ count })
`);
    const exposedWrapper = mount(exposedComponent);
    expect((exposedWrapper.vm as any).count).toBe(1);
    expect((exposedWrapper.vm as any).hidden).toBe(2);
  });
});
