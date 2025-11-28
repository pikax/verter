'use strict';

var path = require('node:path');
var fs = require('node:fs');
var require$$1 = require('@vue/compiler-sfc');
var require$$1$1 = require('@vue/shared');
var require$$0$1 = require('@vue/compiler-core');
var require$$0 = require('oxc-parser');
var require$$1$2 = require('vue');

const DEFAULT_REGEXP = /\.vue$/;
const RELATIVE_REGEXP = /^\.\.?($|[\\/])/;
const isRelative = (fileName) => RELATIVE_REGEXP.test(fileName);
const isVue = (fileName) => DEFAULT_REGEXP.test(fileName);
const isRelativeVue = (fileName) => isVue(fileName) && isRelative(fileName);

var dist = {};

var v5 = {};

var parser$1 = {};

var parser = {};

var utils$3 = {};

var sfc$1 = {};

var sfc = {};

var hasRequiredSfc$1;
function requireSfc$1() {
  if (hasRequiredSfc$1) return sfc;
  hasRequiredSfc$1 = 1;
  Object.defineProperty(sfc, "__esModule", { value: true });
  sfc.retrieveHTMLComments = retrieveHTMLComments;
  sfc.cleanHTMLComments = cleanHTMLComments;
  sfc.catchEmptyBlocks = catchEmptyBlocks;
  sfc.extractBlocksFromDescriptor = extractBlocksFromDescriptor;
  sfc.findBlockLanguage = findBlockLanguage;
  sfc.keepBlocks = keepBlocks;
  sfc.removeBlockTag = removeBlockTag;
  sfc.retrieveAttributes = retrieveAttributes;
  const BLOCK_TAG_REGEX = /(?:<)(?<tag>\w+)(?<content>[^>]*)(?:>)/gm;
  const BLOCK_END_TAG_REGEX = /(?:<\/)(?<tag>\w+)(?<content>[^>]*)(?:>)/gm;
  const EMPTY_BLOCK_REGEX = /<(\w+)([^>]*>)(\s*)<\/\1>/gm;
  const COMMENTED_BLOCKS_REGEX = /(<!--[\s\S]*?-->)/gm;
  function retrieveHTMLComments(source) {
    return Array.from(source.matchAll(COMMENTED_BLOCKS_REGEX)).map((x) => ({
      start: x.index,
      end: x.index + x[0].length
    }));
  }
  function cleanHTMLComments(source) {
    return source.replaceAll(COMMENTED_BLOCKS_REGEX, "").trimStart();
  }
  function catchEmptyBlocks(descriptor) {
    const source = descriptor.source;
    const matches = source.matchAll(EMPTY_BLOCK_REGEX);
    const commentedBlocks = retrieveHTMLComments(source);
    const knownBlocks = [
      ...descriptor.customBlocks,
      descriptor.template,
      descriptor.script,
      descriptor.scriptSetup
    ].filter((x) => !!x).map((x) => ({
      start: x.loc.start.offset,
      end: x.loc.end.offset
    })).concat(commentedBlocks);
    const blocks = [];
    for (const it of matches) {
      const contentStartIndex = it.index + `<${it[1] + it[2]}`.length;
      const contentEndIndex = contentStartIndex + it[3].length;
      const isInComment = knownBlocks.some((block) => {
        return contentStartIndex >= block.start && contentStartIndex < block.end || contentEndIndex > block.start && contentEndIndex <= block.end;
      });
      if (isInComment)
        continue;
      blocks.push({
        type: "empty",
        attrs: {},
        content: it[3],
        // loc does not contain <{tag}> is only after
        loc: {
          source,
          start: {
            offset: contentStartIndex,
            // todo
            line: 0,
            column: 0
          },
          end: {
            offset: contentEndIndex,
            // todo
            line: 0,
            column: 0
          }
        }
      });
    }
    return blocks;
  }
  function extractBlocksFromDescriptor(descriptor) {
    var _a;
    const sfcBlocks = [
      descriptor.script,
      descriptor.scriptSetup,
      descriptor.template,
      ...descriptor.styles,
      ...descriptor.customBlocks
      // ...catchEmptyBlocks(descriptor),
    ].filter((x) => !!x).sort((a, b) => {
      return a.loc.start.offset - b.loc.start.offset;
    });
    const source = descriptor.source;
    const blocks = [];
    let lastEnding = 0;
    for (let i = 0; i < sfcBlocks.length; i++) {
      const block = sfcBlocks[i];
      const initTagOffset = lastEnding;
      const initTagOffsetEnd = block.loc.start.offset;
      const endTagOffset = block.loc.end.offset;
      const nextOffsetEnd = ((_a = sfcBlocks[i + 1]) == null ? void 0 : _a.loc.start.offset) ?? source.length;
      const tagInit = source.slice(initTagOffset, initTagOffsetEnd);
      const cleanTag = cleanHTMLComments(tagInit);
      const tagMatch = cleanTag.matchAll(BLOCK_TAG_REGEX).next().value;
      if (!tagMatch.groups || tagMatch.index === void 0)
        continue;
      const hasClosingTagOnContent = cleanTag.indexOf(">") < cleanTag.length - 1;
      const tag = tagMatch.groups.tag;
      const content = (
        //   tagMatch.groups.content.length >= tagInit.length - "<script>".length
        // if there's many `>`
        hasClosingTagOnContent ? cleanTag.slice(tag.length + 1, -1) : tagMatch.groups.content
      );
      const tagStartIndex = tagInit !== cleanTag ? tagInit.indexOf(cleanTag) + initTagOffset : tagMatch.index + initTagOffset;
      const startTagPos = {
        start: tagStartIndex,
        end: tagStartIndex + (hasClosingTagOnContent ? tagInit.length : tagMatch[0].length)
      };
      const tagEnd = source.slice(endTagOffset, nextOffsetEnd);
      const tagEndMatch = tagEnd.matchAll(BLOCK_END_TAG_REGEX).next().value;
      const tagCloseIndex = endTagOffset + ((tagEndMatch == null ? void 0 : tagEndMatch.index) ?? 0);
      const endTagPos = {
        start: tagCloseIndex,
        end: tagCloseIndex + ((tagEndMatch == null ? void 0 : tagEndMatch[0].length) ?? 0)
      };
      const contentStartIndex = tagStartIndex + `<${tag}`.length;
      blocks.push({
        block,
        tag: {
          type: tag,
          content,
          attributes: retrieveAttributes(content, block.attrs, contentStartIndex),
          pos: {
            open: startTagPos,
            close: endTagPos,
            content: {
              start: contentStartIndex,
              end: contentStartIndex + content.length
            }
          }
        }
      });
      const nextBlock = sfcBlocks.at(i + 1);
      lastEnding = endTagPos.end;
      if (nextBlock) {
        const strToNextTag = source.slice(lastEnding, nextBlock.loc.start.offset);
        const commentedBlocks = Array.from(strToNextTag.matchAll(COMMENTED_BLOCKS_REGEX));
        const lastComment = commentedBlocks.at(-1);
        if (lastComment) {
          lastEnding += lastComment.index + lastComment[0].length;
        }
      }
    }
    return blocks;
  }
  function findBlockLanguage(block) {
    const langAttr = block.block.attrs.lang;
    const lang = langAttr === true ? void 0 : langAttr == null ? void 0 : langAttr.toString();
    if (block.tag.type === "script") {
      return lang || "js";
    }
    if (block.tag.type === "template") {
      return lang || "vue";
    }
    if (block.tag.type === "style") {
      return lang || "css";
    }
    return lang || block.tag.type;
  }
  function keepBlocks(allBlocks, keepBlocks2, s) {
    const toKeep = new Set(keepBlocks2);
    const blocks = [];
    for (const block of allBlocks) {
      if (toKeep.has(block.tag.type)) {
        blocks.push(block);
      } else {
        s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
      }
    }
    return blocks;
  }
  function removeBlockTag(block, s) {
    s.remove(block.tag.pos.open.start, block.tag.pos.open.end);
    s.remove(block.tag.pos.close.start, block.tag.pos.close.end);
  }
  function retrieveAttributes(source, parsedAttributes, offset = 0) {
    const attributes = {};
    const booleanAttributes = /* @__PURE__ */ new Set();
    function removeContent(source2, start, end) {
      return source2.slice(0, start) + " ".repeat(end - start) + source2.slice(end);
    }
    for (const [key, value] of Object.entries(parsedAttributes)) {
      if (value === true) {
        booleanAttributes.add(key);
        continue;
      }
      const match = source.match(new RegExp(`\\s${key}[^=]{0,}=\\s{0,}["'].{${value.length}}["']`));
      if ((match == null ? void 0 : match.index) !== void 0) {
        const len = match[0].length - 1;
        const keyIndex = match.index + 1;
        const valueIndex = keyIndex + match[0].indexOf(value) - 1;
        const end = keyIndex + len;
        attributes[key] = {
          key: {
            content: key,
            start: keyIndex + offset,
            end: keyIndex + key.length + offset
          },
          value: {
            content: value,
            start: valueIndex + offset,
            end: valueIndex + value.length + offset
          },
          content: match[0].slice(1),
          start: keyIndex + offset,
          end: end + offset
        };
        source = removeContent(source, keyIndex, end);
      }
    }
    booleanAttributes.forEach((key) => {
      const keyIndex = source.indexOf(key);
      const end = keyIndex + key.length;
      attributes[key] = {
        key: {
          content: key,
          start: keyIndex + offset,
          end: keyIndex + key.length + offset
        },
        content: key,
        start: keyIndex + offset,
        end: end + offset,
        value: void 0
      };
      source = removeContent(source, keyIndex, end);
    });
    return attributes;
  }
  return sfc;
}

var hasRequiredSfc;
function requireSfc() {
  if (hasRequiredSfc) return sfc$1;
  hasRequiredSfc = 1;
  (function(exports) {
    var __createBinding = sfc$1 && sfc$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = sfc$1 && sfc$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireSfc$1(), exports);
  })(sfc$1);
  return sfc$1;
}

var hasRequiredUtils$3;
function requireUtils$3() {
  if (hasRequiredUtils$3) return utils$3;
  hasRequiredUtils$3 = 1;
  (function(exports) {
    var __createBinding = utils$3 && utils$3.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = utils$3 && utils$3.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireSfc(), exports);
  })(utils$3);
  return utils$3;
}

var script$3 = {};

var walk$1 = {};

var walk = {};

var hasRequiredWalk$1;
function requireWalk$1() {
  if (hasRequiredWalk$1) return walk;
  hasRequiredWalk$1 = 1;
  Object.defineProperty(walk, "__esModule", { value: true });
  walk.shallowWalk = shallowWalk;
  walk.deepWalk = deepWalk;
  walk.templateWalk = templateWalk;
  const compiler_sfc_1 = require$$1;
  const shared_1 = require$$1$1;
  function shallowWalk(root, cb) {
    if (root.type === "Program") {
      for (let i = 0; i < root.body.length; i++) {
        cb(root.body[i]);
      }
    } else if (root.type === "FunctionExpression") {
      if (root.body) {
        for (let i = 0; i < root.body.body.length; i++) {
          cb(root.body.body[i]);
        }
      }
    } else if (root.type === "BlockStatement") {
      for (let i = 0; i < root.body.length; i++) {
        cb(root.body[i]);
      }
    } else if (root.body) {
      if (root.body.type === "BlockStatement") {
        for (let i = 0; i < root.body.body.length; i++) {
          cb(root.body.body[i]);
        }
      }
    }
  }
  function deepWalk(root, enter, leave) {
    (0, compiler_sfc_1.walk)(root, {
      enter,
      leave
    });
  }
  function templateWalk(root, options, context) {
    function visit(node, parent, context2, parentContext) {
      var _a, _b, _c;
      if (!node) {
        return;
      }
      const returnedContext = (_a = options.enter) == null ? void 0 : _a.call(options, node, parent, context2, parentContext);
      const overrideContext = returnedContext || context2;
      const childContext = {
        conditions: ((_b = parentContext.conditions) == null ? void 0 : _b.length) > 0 ? [...parentContext.conditions] : [],
        inFor: !!parentContext.for || !!parentContext.inFor
      };
      if ("children" in node) {
        for (let i = 0; i < node.children.length; i++) {
          const element = node.children[i];
          if ((0, shared_1.isObject)(element)) {
            visit(element, node, overrideContext, childContext);
          }
        }
      }
      (_c = options.leave) == null ? void 0 : _c.call(options, node, parent, context2, parentContext);
    }
    return visit(root, null, context, {});
  }
  return walk;
}

var hasRequiredWalk;
function requireWalk() {
  if (hasRequiredWalk) return walk$1;
  hasRequiredWalk = 1;
  (function(exports) {
    var __createBinding = walk$1 && walk$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = walk$1 && walk$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireWalk$1(), exports);
  })(walk$1);
  return walk$1;
}

var shared$1 = {};

var shared = {};

var hasRequiredShared$1;
function requireShared$1() {
  if (hasRequiredShared$1) return shared;
  hasRequiredShared$1 = 1;
  Object.defineProperty(shared, "__esModule", { value: true });
  shared.handleShared = handleShared;
  shared.createSharedContext = createSharedContext;
  function handleShared(node) {
    var _a;
    switch (node.type) {
      // case "ExpressionStatement": {
      //   const expression = node.expression;
      //   if (expression.type === "CallExpression") {
      //     return [
      //       {
      //         type: ScriptTypes.FunctionCall,
      //         node: expression,
      //         parent: node,
      //         name:
      //           expression.callee.type === "Identifier"
      //             ? expression.callee.name
      //             : "",
      //       },
      //     ];
      //   }
      //   return false;
      // }
      case "ExportAllDeclaration": {
        return [
          {
            type: "Export",
            node
          }
        ];
      }
      case "ImportDeclaration": {
        return [
          {
            type: "Import",
            node,
            bindings: ((_a = node.specifiers) == null ? void 0 : _a.map((x) => {
              switch (x.type) {
                case "ImportSpecifier":
                case "ImportDefaultSpecifier":
                case "ImportNamespaceSpecifier": {
                  const name = x.local.name;
                  return {
                    type: "Binding",
                    name,
                    node: x
                  };
                }
              }
              return void 0;
            }).filter((x) => !!x)) ?? []
          }
        ];
      }
      default:
        return false;
    }
  }
  function createSharedContext(opts) {
    opts.lang === "ts" || opts.lang === "tsx";
    function visit(node, parent, key) {
      var _a;
      switch (node.type) {
        case "ExportNamedDeclaration":
        case "ExportAllDeclaration": {
          return {
            type: "Export",
            node
          };
        }
        case "ImportDeclaration": {
          const importItem = {
            type: "Import",
            node,
            bindings: ((_a = node.specifiers) == null ? void 0 : _a.map((x) => {
              switch (x.type) {
                case "ImportSpecifier":
                case "ImportDefaultSpecifier":
                case "ImportNamespaceSpecifier": {
                  const name = x.local.name;
                  return {
                    type: "Binding",
                    name,
                    node: x
                  };
                }
              }
            }).filter((x) => !!x)) ?? []
          };
          return [importItem, ...importItem.bindings];
        }
        case "TSTypeAssertion": {
          return {
            type: "TypeAssertion",
            node
          };
        }
      }
    }
    function leave(node, parent, key) {
    }
    return {
      visit,
      leave
    };
  }
  return shared;
}

var hasRequiredShared;
function requireShared() {
  if (hasRequiredShared) return shared$1;
  hasRequiredShared = 1;
  (function(exports) {
    var __createBinding = shared$1 && shared$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = shared$1 && shared$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireShared$1(), exports);
  })(shared$1);
  return shared$1;
}

var options$1 = {};

var options = {};

var setup$1 = {};

var setup = {};

var utils$2 = {};

var node = {};

var hasRequiredNode;
function requireNode() {
  if (hasRequiredNode) return node;
  hasRequiredNode = 1;
  Object.defineProperty(node, "__esModule", { value: true });
  node.patchBabelNodeLoc = patchBabelNodeLoc;
  node.patchBabelPosition = patchBabelPosition;
  node.patchAcornPosition = patchAcornPosition;
  node.patchAcornNodeLoc = patchAcornNodeLoc;
  function patchBabelNodeLoc(node2, templateNode) {
    if (node2.loc) {
      patchBabelPosition(node2.loc.start, templateNode.loc.start);
      patchBabelPosition(node2.loc.end, templateNode.loc.start);
      node2.loc.source = node2.loc.identifierName ?? // @ts-expect-error not part of loc
      templateNode.content.slice(node2.loc.start.index - 1, node2.loc.end.index - 1);
    }
    return node2;
  }
  function patchBabelPosition(pos, offsetPos) {
    pos.line = offsetPos.line + pos.line - 1;
    pos.column = offsetPos.column + pos.column - 1;
    pos.offset = offsetPos.offset + pos.index - 1;
    return pos;
  }
  function patchAcornPosition(pos, offsetPos) {
    pos.line = offsetPos.line + pos.line - 1;
    pos.column = offsetPos.column + pos.column - 1;
    pos.offset = offsetPos.offset + pos.index - 1;
    return pos;
  }
  function patchAcornNodeLoc(node2, templateNode) {
    if (node2.loc) {
      patchAcornPosition(node2.loc.start, templateNode.loc.start);
      patchAcornPosition(node2.loc.end, templateNode.loc.start);
    } else {
      node2.loc = {
        start: { line: 0, column: 0, offset: node2.start },
        end: { line: 0, column: 0, offset: node2.end },
        // @ts-expect-error not part of loc
        source: templateNode.content.slice(node2.start, node2.end)
      };
      patchAcornPosition(node2.loc.start, templateNode.loc.start);
      patchAcornPosition(node2.loc.end, templateNode.loc.start);
    }
    node2.loc.source = // @ts-expect-error not part of loc
    templateNode.content.slice(
      // @ts-expect-error not part of loc
      node2.loc.start.index - 1,
      // @ts-expect-error not part of loc
      node2.loc.end.index - 1
    );
    return node2;
  }
  return node;
}

var ast = {};

var acornLoose$1 = {exports: {}};

var acorn$1 = {exports: {}};

var acorn = acorn$1.exports;

var hasRequiredAcorn;

function requireAcorn () {
    if (hasRequiredAcorn) return acorn$1.exports;
    hasRequiredAcorn = 1;
    (function (module, exports) {
        (function (global, factory) {
          factory(exports) ;
        })(acorn, (function (exports) {
          // This file was generated. Do not modify manually!
          var astralIdentifierCodes = [509, 0, 227, 0, 150, 4, 294, 9, 1368, 2, 2, 1, 6, 3, 41, 2, 5, 0, 166, 1, 574, 3, 9, 9, 7, 9, 32, 4, 318, 1, 80, 3, 71, 10, 50, 3, 123, 2, 54, 14, 32, 10, 3, 1, 11, 3, 46, 10, 8, 0, 46, 9, 7, 2, 37, 13, 2, 9, 6, 1, 45, 0, 13, 2, 49, 13, 9, 3, 2, 11, 83, 11, 7, 0, 3, 0, 158, 11, 6, 9, 7, 3, 56, 1, 2, 6, 3, 1, 3, 2, 10, 0, 11, 1, 3, 6, 4, 4, 68, 8, 2, 0, 3, 0, 2, 3, 2, 4, 2, 0, 15, 1, 83, 17, 10, 9, 5, 0, 82, 19, 13, 9, 214, 6, 3, 8, 28, 1, 83, 16, 16, 9, 82, 12, 9, 9, 7, 19, 58, 14, 5, 9, 243, 14, 166, 9, 71, 5, 2, 1, 3, 3, 2, 0, 2, 1, 13, 9, 120, 6, 3, 6, 4, 0, 29, 9, 41, 6, 2, 3, 9, 0, 10, 10, 47, 15, 343, 9, 54, 7, 2, 7, 17, 9, 57, 21, 2, 13, 123, 5, 4, 0, 2, 1, 2, 6, 2, 0, 9, 9, 49, 4, 2, 1, 2, 4, 9, 9, 330, 3, 10, 1, 2, 0, 49, 6, 4, 4, 14, 10, 5350, 0, 7, 14, 11465, 27, 2343, 9, 87, 9, 39, 4, 60, 6, 26, 9, 535, 9, 470, 0, 2, 54, 8, 3, 82, 0, 12, 1, 19628, 1, 4178, 9, 519, 45, 3, 22, 543, 4, 4, 5, 9, 7, 3, 6, 31, 3, 149, 2, 1418, 49, 513, 54, 5, 49, 9, 0, 15, 0, 23, 4, 2, 14, 1361, 6, 2, 16, 3, 6, 2, 1, 2, 4, 101, 0, 161, 6, 10, 9, 357, 0, 62, 13, 499, 13, 245, 1, 2, 9, 726, 6, 110, 6, 6, 9, 4759, 9, 787719, 239];

          // This file was generated. Do not modify manually!
          var astralIdentifierStartCodes = [0, 11, 2, 25, 2, 18, 2, 1, 2, 14, 3, 13, 35, 122, 70, 52, 268, 28, 4, 48, 48, 31, 14, 29, 6, 37, 11, 29, 3, 35, 5, 7, 2, 4, 43, 157, 19, 35, 5, 35, 5, 39, 9, 51, 13, 10, 2, 14, 2, 6, 2, 1, 2, 10, 2, 14, 2, 6, 2, 1, 4, 51, 13, 310, 10, 21, 11, 7, 25, 5, 2, 41, 2, 8, 70, 5, 3, 0, 2, 43, 2, 1, 4, 0, 3, 22, 11, 22, 10, 30, 66, 18, 2, 1, 11, 21, 11, 25, 71, 55, 7, 1, 65, 0, 16, 3, 2, 2, 2, 28, 43, 28, 4, 28, 36, 7, 2, 27, 28, 53, 11, 21, 11, 18, 14, 17, 111, 72, 56, 50, 14, 50, 14, 35, 39, 27, 10, 22, 251, 41, 7, 1, 17, 2, 60, 28, 11, 0, 9, 21, 43, 17, 47, 20, 28, 22, 13, 52, 58, 1, 3, 0, 14, 44, 33, 24, 27, 35, 30, 0, 3, 0, 9, 34, 4, 0, 13, 47, 15, 3, 22, 0, 2, 0, 36, 17, 2, 24, 20, 1, 64, 6, 2, 0, 2, 3, 2, 14, 2, 9, 8, 46, 39, 7, 3, 1, 3, 21, 2, 6, 2, 1, 2, 4, 4, 0, 19, 0, 13, 4, 31, 9, 2, 0, 3, 0, 2, 37, 2, 0, 26, 0, 2, 0, 45, 52, 19, 3, 21, 2, 31, 47, 21, 1, 2, 0, 185, 46, 42, 3, 37, 47, 21, 0, 60, 42, 14, 0, 72, 26, 38, 6, 186, 43, 117, 63, 32, 7, 3, 0, 3, 7, 2, 1, 2, 23, 16, 0, 2, 0, 95, 7, 3, 38, 17, 0, 2, 0, 29, 0, 11, 39, 8, 0, 22, 0, 12, 45, 20, 0, 19, 72, 200, 32, 32, 8, 2, 36, 18, 0, 50, 29, 113, 6, 2, 1, 2, 37, 22, 0, 26, 5, 2, 1, 2, 31, 15, 0, 328, 18, 16, 0, 2, 12, 2, 33, 125, 0, 80, 921, 103, 110, 18, 195, 2637, 96, 16, 1071, 18, 5, 26, 3994, 6, 582, 6842, 29, 1763, 568, 8, 30, 18, 78, 18, 29, 19, 47, 17, 3, 32, 20, 6, 18, 433, 44, 212, 63, 129, 74, 6, 0, 67, 12, 65, 1, 2, 0, 29, 6135, 9, 1237, 42, 9, 8936, 3, 2, 6, 2, 1, 2, 290, 16, 0, 30, 2, 3, 0, 15, 3, 9, 395, 2309, 106, 6, 12, 4, 8, 8, 9, 5991, 84, 2, 70, 2, 1, 3, 0, 3, 1, 3, 3, 2, 11, 2, 0, 2, 6, 2, 64, 2, 3, 3, 7, 2, 6, 2, 27, 2, 3, 2, 4, 2, 0, 4, 6, 2, 339, 3, 24, 2, 24, 2, 30, 2, 24, 2, 30, 2, 24, 2, 30, 2, 24, 2, 30, 2, 24, 2, 7, 1845, 30, 7, 5, 262, 61, 147, 44, 11, 6, 17, 0, 322, 29, 19, 43, 485, 27, 229, 29, 3, 0, 496, 6, 2, 3, 2, 1, 2, 14, 2, 196, 60, 67, 8, 0, 1205, 3, 2, 26, 2, 1, 2, 0, 3, 0, 2, 9, 2, 3, 2, 0, 2, 0, 7, 0, 5, 0, 2, 0, 2, 0, 2, 2, 2, 1, 2, 0, 3, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 1, 2, 0, 3, 3, 2, 6, 2, 3, 2, 3, 2, 0, 2, 9, 2, 16, 6, 2, 2, 4, 2, 16, 4421, 42719, 33, 4153, 7, 221, 3, 5761, 15, 7472, 16, 621, 2467, 541, 1507, 4938, 6, 4191];

          // This file was generated. Do not modify manually!
          var nonASCIIidentifierChars = "\u200c\u200d\xb7\u0300-\u036f\u0387\u0483-\u0487\u0591-\u05bd\u05bf\u05c1\u05c2\u05c4\u05c5\u05c7\u0610-\u061a\u064b-\u0669\u0670\u06d6-\u06dc\u06df-\u06e4\u06e7\u06e8\u06ea-\u06ed\u06f0-\u06f9\u0711\u0730-\u074a\u07a6-\u07b0\u07c0-\u07c9\u07eb-\u07f3\u07fd\u0816-\u0819\u081b-\u0823\u0825-\u0827\u0829-\u082d\u0859-\u085b\u0897-\u089f\u08ca-\u08e1\u08e3-\u0903\u093a-\u093c\u093e-\u094f\u0951-\u0957\u0962\u0963\u0966-\u096f\u0981-\u0983\u09bc\u09be-\u09c4\u09c7\u09c8\u09cb-\u09cd\u09d7\u09e2\u09e3\u09e6-\u09ef\u09fe\u0a01-\u0a03\u0a3c\u0a3e-\u0a42\u0a47\u0a48\u0a4b-\u0a4d\u0a51\u0a66-\u0a71\u0a75\u0a81-\u0a83\u0abc\u0abe-\u0ac5\u0ac7-\u0ac9\u0acb-\u0acd\u0ae2\u0ae3\u0ae6-\u0aef\u0afa-\u0aff\u0b01-\u0b03\u0b3c\u0b3e-\u0b44\u0b47\u0b48\u0b4b-\u0b4d\u0b55-\u0b57\u0b62\u0b63\u0b66-\u0b6f\u0b82\u0bbe-\u0bc2\u0bc6-\u0bc8\u0bca-\u0bcd\u0bd7\u0be6-\u0bef\u0c00-\u0c04\u0c3c\u0c3e-\u0c44\u0c46-\u0c48\u0c4a-\u0c4d\u0c55\u0c56\u0c62\u0c63\u0c66-\u0c6f\u0c81-\u0c83\u0cbc\u0cbe-\u0cc4\u0cc6-\u0cc8\u0cca-\u0ccd\u0cd5\u0cd6\u0ce2\u0ce3\u0ce6-\u0cef\u0cf3\u0d00-\u0d03\u0d3b\u0d3c\u0d3e-\u0d44\u0d46-\u0d48\u0d4a-\u0d4d\u0d57\u0d62\u0d63\u0d66-\u0d6f\u0d81-\u0d83\u0dca\u0dcf-\u0dd4\u0dd6\u0dd8-\u0ddf\u0de6-\u0def\u0df2\u0df3\u0e31\u0e34-\u0e3a\u0e47-\u0e4e\u0e50-\u0e59\u0eb1\u0eb4-\u0ebc\u0ec8-\u0ece\u0ed0-\u0ed9\u0f18\u0f19\u0f20-\u0f29\u0f35\u0f37\u0f39\u0f3e\u0f3f\u0f71-\u0f84\u0f86\u0f87\u0f8d-\u0f97\u0f99-\u0fbc\u0fc6\u102b-\u103e\u1040-\u1049\u1056-\u1059\u105e-\u1060\u1062-\u1064\u1067-\u106d\u1071-\u1074\u1082-\u108d\u108f-\u109d\u135d-\u135f\u1369-\u1371\u1712-\u1715\u1732-\u1734\u1752\u1753\u1772\u1773\u17b4-\u17d3\u17dd\u17e0-\u17e9\u180b-\u180d\u180f-\u1819\u18a9\u1920-\u192b\u1930-\u193b\u1946-\u194f\u19d0-\u19da\u1a17-\u1a1b\u1a55-\u1a5e\u1a60-\u1a7c\u1a7f-\u1a89\u1a90-\u1a99\u1ab0-\u1abd\u1abf-\u1ace\u1b00-\u1b04\u1b34-\u1b44\u1b50-\u1b59\u1b6b-\u1b73\u1b80-\u1b82\u1ba1-\u1bad\u1bb0-\u1bb9\u1be6-\u1bf3\u1c24-\u1c37\u1c40-\u1c49\u1c50-\u1c59\u1cd0-\u1cd2\u1cd4-\u1ce8\u1ced\u1cf4\u1cf7-\u1cf9\u1dc0-\u1dff\u200c\u200d\u203f\u2040\u2054\u20d0-\u20dc\u20e1\u20e5-\u20f0\u2cef-\u2cf1\u2d7f\u2de0-\u2dff\u302a-\u302f\u3099\u309a\u30fb\ua620-\ua629\ua66f\ua674-\ua67d\ua69e\ua69f\ua6f0\ua6f1\ua802\ua806\ua80b\ua823-\ua827\ua82c\ua880\ua881\ua8b4-\ua8c5\ua8d0-\ua8d9\ua8e0-\ua8f1\ua8ff-\ua909\ua926-\ua92d\ua947-\ua953\ua980-\ua983\ua9b3-\ua9c0\ua9d0-\ua9d9\ua9e5\ua9f0-\ua9f9\uaa29-\uaa36\uaa43\uaa4c\uaa4d\uaa50-\uaa59\uaa7b-\uaa7d\uaab0\uaab2-\uaab4\uaab7\uaab8\uaabe\uaabf\uaac1\uaaeb-\uaaef\uaaf5\uaaf6\uabe3-\uabea\uabec\uabed\uabf0-\uabf9\ufb1e\ufe00-\ufe0f\ufe20-\ufe2f\ufe33\ufe34\ufe4d-\ufe4f\uff10-\uff19\uff3f\uff65";

          // This file was generated. Do not modify manually!
          var nonASCIIidentifierStartChars = "\xaa\xb5\xba\xc0-\xd6\xd8-\xf6\xf8-\u02c1\u02c6-\u02d1\u02e0-\u02e4\u02ec\u02ee\u0370-\u0374\u0376\u0377\u037a-\u037d\u037f\u0386\u0388-\u038a\u038c\u038e-\u03a1\u03a3-\u03f5\u03f7-\u0481\u048a-\u052f\u0531-\u0556\u0559\u0560-\u0588\u05d0-\u05ea\u05ef-\u05f2\u0620-\u064a\u066e\u066f\u0671-\u06d3\u06d5\u06e5\u06e6\u06ee\u06ef\u06fa-\u06fc\u06ff\u0710\u0712-\u072f\u074d-\u07a5\u07b1\u07ca-\u07ea\u07f4\u07f5\u07fa\u0800-\u0815\u081a\u0824\u0828\u0840-\u0858\u0860-\u086a\u0870-\u0887\u0889-\u088e\u08a0-\u08c9\u0904-\u0939\u093d\u0950\u0958-\u0961\u0971-\u0980\u0985-\u098c\u098f\u0990\u0993-\u09a8\u09aa-\u09b0\u09b2\u09b6-\u09b9\u09bd\u09ce\u09dc\u09dd\u09df-\u09e1\u09f0\u09f1\u09fc\u0a05-\u0a0a\u0a0f\u0a10\u0a13-\u0a28\u0a2a-\u0a30\u0a32\u0a33\u0a35\u0a36\u0a38\u0a39\u0a59-\u0a5c\u0a5e\u0a72-\u0a74\u0a85-\u0a8d\u0a8f-\u0a91\u0a93-\u0aa8\u0aaa-\u0ab0\u0ab2\u0ab3\u0ab5-\u0ab9\u0abd\u0ad0\u0ae0\u0ae1\u0af9\u0b05-\u0b0c\u0b0f\u0b10\u0b13-\u0b28\u0b2a-\u0b30\u0b32\u0b33\u0b35-\u0b39\u0b3d\u0b5c\u0b5d\u0b5f-\u0b61\u0b71\u0b83\u0b85-\u0b8a\u0b8e-\u0b90\u0b92-\u0b95\u0b99\u0b9a\u0b9c\u0b9e\u0b9f\u0ba3\u0ba4\u0ba8-\u0baa\u0bae-\u0bb9\u0bd0\u0c05-\u0c0c\u0c0e-\u0c10\u0c12-\u0c28\u0c2a-\u0c39\u0c3d\u0c58-\u0c5a\u0c5d\u0c60\u0c61\u0c80\u0c85-\u0c8c\u0c8e-\u0c90\u0c92-\u0ca8\u0caa-\u0cb3\u0cb5-\u0cb9\u0cbd\u0cdd\u0cde\u0ce0\u0ce1\u0cf1\u0cf2\u0d04-\u0d0c\u0d0e-\u0d10\u0d12-\u0d3a\u0d3d\u0d4e\u0d54-\u0d56\u0d5f-\u0d61\u0d7a-\u0d7f\u0d85-\u0d96\u0d9a-\u0db1\u0db3-\u0dbb\u0dbd\u0dc0-\u0dc6\u0e01-\u0e30\u0e32\u0e33\u0e40-\u0e46\u0e81\u0e82\u0e84\u0e86-\u0e8a\u0e8c-\u0ea3\u0ea5\u0ea7-\u0eb0\u0eb2\u0eb3\u0ebd\u0ec0-\u0ec4\u0ec6\u0edc-\u0edf\u0f00\u0f40-\u0f47\u0f49-\u0f6c\u0f88-\u0f8c\u1000-\u102a\u103f\u1050-\u1055\u105a-\u105d\u1061\u1065\u1066\u106e-\u1070\u1075-\u1081\u108e\u10a0-\u10c5\u10c7\u10cd\u10d0-\u10fa\u10fc-\u1248\u124a-\u124d\u1250-\u1256\u1258\u125a-\u125d\u1260-\u1288\u128a-\u128d\u1290-\u12b0\u12b2-\u12b5\u12b8-\u12be\u12c0\u12c2-\u12c5\u12c8-\u12d6\u12d8-\u1310\u1312-\u1315\u1318-\u135a\u1380-\u138f\u13a0-\u13f5\u13f8-\u13fd\u1401-\u166c\u166f-\u167f\u1681-\u169a\u16a0-\u16ea\u16ee-\u16f8\u1700-\u1711\u171f-\u1731\u1740-\u1751\u1760-\u176c\u176e-\u1770\u1780-\u17b3\u17d7\u17dc\u1820-\u1878\u1880-\u18a8\u18aa\u18b0-\u18f5\u1900-\u191e\u1950-\u196d\u1970-\u1974\u1980-\u19ab\u19b0-\u19c9\u1a00-\u1a16\u1a20-\u1a54\u1aa7\u1b05-\u1b33\u1b45-\u1b4c\u1b83-\u1ba0\u1bae\u1baf\u1bba-\u1be5\u1c00-\u1c23\u1c4d-\u1c4f\u1c5a-\u1c7d\u1c80-\u1c8a\u1c90-\u1cba\u1cbd-\u1cbf\u1ce9-\u1cec\u1cee-\u1cf3\u1cf5\u1cf6\u1cfa\u1d00-\u1dbf\u1e00-\u1f15\u1f18-\u1f1d\u1f20-\u1f45\u1f48-\u1f4d\u1f50-\u1f57\u1f59\u1f5b\u1f5d\u1f5f-\u1f7d\u1f80-\u1fb4\u1fb6-\u1fbc\u1fbe\u1fc2-\u1fc4\u1fc6-\u1fcc\u1fd0-\u1fd3\u1fd6-\u1fdb\u1fe0-\u1fec\u1ff2-\u1ff4\u1ff6-\u1ffc\u2071\u207f\u2090-\u209c\u2102\u2107\u210a-\u2113\u2115\u2118-\u211d\u2124\u2126\u2128\u212a-\u2139\u213c-\u213f\u2145-\u2149\u214e\u2160-\u2188\u2c00-\u2ce4\u2ceb-\u2cee\u2cf2\u2cf3\u2d00-\u2d25\u2d27\u2d2d\u2d30-\u2d67\u2d6f\u2d80-\u2d96\u2da0-\u2da6\u2da8-\u2dae\u2db0-\u2db6\u2db8-\u2dbe\u2dc0-\u2dc6\u2dc8-\u2dce\u2dd0-\u2dd6\u2dd8-\u2dde\u3005-\u3007\u3021-\u3029\u3031-\u3035\u3038-\u303c\u3041-\u3096\u309b-\u309f\u30a1-\u30fa\u30fc-\u30ff\u3105-\u312f\u3131-\u318e\u31a0-\u31bf\u31f0-\u31ff\u3400-\u4dbf\u4e00-\ua48c\ua4d0-\ua4fd\ua500-\ua60c\ua610-\ua61f\ua62a\ua62b\ua640-\ua66e\ua67f-\ua69d\ua6a0-\ua6ef\ua717-\ua71f\ua722-\ua788\ua78b-\ua7cd\ua7d0\ua7d1\ua7d3\ua7d5-\ua7dc\ua7f2-\ua801\ua803-\ua805\ua807-\ua80a\ua80c-\ua822\ua840-\ua873\ua882-\ua8b3\ua8f2-\ua8f7\ua8fb\ua8fd\ua8fe\ua90a-\ua925\ua930-\ua946\ua960-\ua97c\ua984-\ua9b2\ua9cf\ua9e0-\ua9e4\ua9e6-\ua9ef\ua9fa-\ua9fe\uaa00-\uaa28\uaa40-\uaa42\uaa44-\uaa4b\uaa60-\uaa76\uaa7a\uaa7e-\uaaaf\uaab1\uaab5\uaab6\uaab9-\uaabd\uaac0\uaac2\uaadb-\uaadd\uaae0-\uaaea\uaaf2-\uaaf4\uab01-\uab06\uab09-\uab0e\uab11-\uab16\uab20-\uab26\uab28-\uab2e\uab30-\uab5a\uab5c-\uab69\uab70-\uabe2\uac00-\ud7a3\ud7b0-\ud7c6\ud7cb-\ud7fb\uf900-\ufa6d\ufa70-\ufad9\ufb00-\ufb06\ufb13-\ufb17\ufb1d\ufb1f-\ufb28\ufb2a-\ufb36\ufb38-\ufb3c\ufb3e\ufb40\ufb41\ufb43\ufb44\ufb46-\ufbb1\ufbd3-\ufd3d\ufd50-\ufd8f\ufd92-\ufdc7\ufdf0-\ufdfb\ufe70-\ufe74\ufe76-\ufefc\uff21-\uff3a\uff41-\uff5a\uff66-\uffbe\uffc2-\uffc7\uffca-\uffcf\uffd2-\uffd7\uffda-\uffdc";

          // These are a run-length and offset encoded representation of the
          // >0xffff code points that are a valid part of identifiers. The
          // offset starts at 0x10000, and each pair of numbers represents an
          // offset to the next range, and then a size of the range.

          // Reserved word lists for various dialects of the language

          var reservedWords = {
            3: "abstract boolean byte char class double enum export extends final float goto implements import int interface long native package private protected public short static super synchronized throws transient volatile",
            5: "class enum extends super const export import",
            6: "enum",
            strict: "implements interface let package private protected public static yield",
            strictBind: "eval arguments"
          };

          // And the keywords

          var ecma5AndLessKeywords = "break case catch continue debugger default do else finally for function if return switch throw try var while with null true false instanceof typeof void delete new in this";

          var keywords$1 = {
            5: ecma5AndLessKeywords,
            "5module": ecma5AndLessKeywords + " export import",
            6: ecma5AndLessKeywords + " const class extends export import super"
          };

          var keywordRelationalOperator = /^in(stanceof)?$/;

          // ## Character categories

          var nonASCIIidentifierStart = new RegExp("[" + nonASCIIidentifierStartChars + "]");
          var nonASCIIidentifier = new RegExp("[" + nonASCIIidentifierStartChars + nonASCIIidentifierChars + "]");

          // This has a complexity linear to the value of the code. The
          // assumption is that looking up astral identifier characters is
          // rare.
          function isInAstralSet(code, set) {
            var pos = 0x10000;
            for (var i = 0; i < set.length; i += 2) {
              pos += set[i];
              if (pos > code) { return false }
              pos += set[i + 1];
              if (pos >= code) { return true }
            }
            return false
          }

          // Test whether a given character code starts an identifier.

          function isIdentifierStart(code, astral) {
            if (code < 65) { return code === 36 }
            if (code < 91) { return true }
            if (code < 97) { return code === 95 }
            if (code < 123) { return true }
            if (code <= 0xffff) { return code >= 0xaa && nonASCIIidentifierStart.test(String.fromCharCode(code)) }
            if (astral === false) { return false }
            return isInAstralSet(code, astralIdentifierStartCodes)
          }

          // Test whether a given character is part of an identifier.

          function isIdentifierChar(code, astral) {
            if (code < 48) { return code === 36 }
            if (code < 58) { return true }
            if (code < 65) { return false }
            if (code < 91) { return true }
            if (code < 97) { return code === 95 }
            if (code < 123) { return true }
            if (code <= 0xffff) { return code >= 0xaa && nonASCIIidentifier.test(String.fromCharCode(code)) }
            if (astral === false) { return false }
            return isInAstralSet(code, astralIdentifierStartCodes) || isInAstralSet(code, astralIdentifierCodes)
          }

          // ## Token types

          // The assignment of fine-grained, information-carrying type objects
          // allows the tokenizer to store the information it has about a
          // token in a way that is very cheap for the parser to look up.

          // All token type variables start with an underscore, to make them
          // easy to recognize.

          // The `beforeExpr` property is used to disambiguate between regular
          // expressions and divisions. It is set on all token types that can
          // be followed by an expression (thus, a slash after them would be a
          // regular expression).
          //
          // The `startsExpr` property is used to check if the token ends a
          // `yield` expression. It is set on all token types that either can
          // directly start an expression (like a quotation mark) or can
          // continue an expression (like the body of a string).
          //
          // `isLoop` marks a keyword as starting a loop, which is important
          // to know when parsing a label, in order to allow or disallow
          // continue jumps to that label.

          var TokenType = function TokenType(label, conf) {
            if ( conf === void 0 ) conf = {};

            this.label = label;
            this.keyword = conf.keyword;
            this.beforeExpr = !!conf.beforeExpr;
            this.startsExpr = !!conf.startsExpr;
            this.isLoop = !!conf.isLoop;
            this.isAssign = !!conf.isAssign;
            this.prefix = !!conf.prefix;
            this.postfix = !!conf.postfix;
            this.binop = conf.binop || null;
            this.updateContext = null;
          };

          function binop(name, prec) {
            return new TokenType(name, {beforeExpr: true, binop: prec})
          }
          var beforeExpr = {beforeExpr: true}, startsExpr = {startsExpr: true};

          // Map keyword names to token types.

          var keywords = {};

          // Succinct definitions of keyword token types
          function kw(name, options) {
            if ( options === void 0 ) options = {};

            options.keyword = name;
            return keywords[name] = new TokenType(name, options)
          }

          var types$1 = {
            num: new TokenType("num", startsExpr),
            regexp: new TokenType("regexp", startsExpr),
            string: new TokenType("string", startsExpr),
            name: new TokenType("name", startsExpr),
            privateId: new TokenType("privateId", startsExpr),
            eof: new TokenType("eof"),

            // Punctuation token types.
            bracketL: new TokenType("[", {beforeExpr: true, startsExpr: true}),
            bracketR: new TokenType("]"),
            braceL: new TokenType("{", {beforeExpr: true, startsExpr: true}),
            braceR: new TokenType("}"),
            parenL: new TokenType("(", {beforeExpr: true, startsExpr: true}),
            parenR: new TokenType(")"),
            comma: new TokenType(",", beforeExpr),
            semi: new TokenType(";", beforeExpr),
            colon: new TokenType(":", beforeExpr),
            dot: new TokenType("."),
            question: new TokenType("?", beforeExpr),
            questionDot: new TokenType("?."),
            arrow: new TokenType("=>", beforeExpr),
            template: new TokenType("template"),
            invalidTemplate: new TokenType("invalidTemplate"),
            ellipsis: new TokenType("...", beforeExpr),
            backQuote: new TokenType("`", startsExpr),
            dollarBraceL: new TokenType("${", {beforeExpr: true, startsExpr: true}),

            // Operators. These carry several kinds of properties to help the
            // parser use them properly (the presence of these properties is
            // what categorizes them as operators).
            //
            // `binop`, when present, specifies that this operator is a binary
            // operator, and will refer to its precedence.
            //
            // `prefix` and `postfix` mark the operator as a prefix or postfix
            // unary operator.
            //
            // `isAssign` marks all of `=`, `+=`, `-=` etcetera, which act as
            // binary operators with a very low precedence, that should result
            // in AssignmentExpression nodes.

            eq: new TokenType("=", {beforeExpr: true, isAssign: true}),
            assign: new TokenType("_=", {beforeExpr: true, isAssign: true}),
            incDec: new TokenType("++/--", {prefix: true, postfix: true, startsExpr: true}),
            prefix: new TokenType("!/~", {beforeExpr: true, prefix: true, startsExpr: true}),
            logicalOR: binop("||", 1),
            logicalAND: binop("&&", 2),
            bitwiseOR: binop("|", 3),
            bitwiseXOR: binop("^", 4),
            bitwiseAND: binop("&", 5),
            equality: binop("==/!=/===/!==", 6),
            relational: binop("</>/<=/>=", 7),
            bitShift: binop("<</>>/>>>", 8),
            plusMin: new TokenType("+/-", {beforeExpr: true, binop: 9, prefix: true, startsExpr: true}),
            modulo: binop("%", 10),
            star: binop("*", 10),
            slash: binop("/", 10),
            starstar: new TokenType("**", {beforeExpr: true}),
            coalesce: binop("??", 1),

            // Keyword token types.
            _break: kw("break"),
            _case: kw("case", beforeExpr),
            _catch: kw("catch"),
            _continue: kw("continue"),
            _debugger: kw("debugger"),
            _default: kw("default", beforeExpr),
            _do: kw("do", {isLoop: true, beforeExpr: true}),
            _else: kw("else", beforeExpr),
            _finally: kw("finally"),
            _for: kw("for", {isLoop: true}),
            _function: kw("function", startsExpr),
            _if: kw("if"),
            _return: kw("return", beforeExpr),
            _switch: kw("switch"),
            _throw: kw("throw", beforeExpr),
            _try: kw("try"),
            _var: kw("var"),
            _const: kw("const"),
            _while: kw("while", {isLoop: true}),
            _with: kw("with"),
            _new: kw("new", {beforeExpr: true, startsExpr: true}),
            _this: kw("this", startsExpr),
            _super: kw("super", startsExpr),
            _class: kw("class", startsExpr),
            _extends: kw("extends", beforeExpr),
            _export: kw("export"),
            _import: kw("import", startsExpr),
            _null: kw("null", startsExpr),
            _true: kw("true", startsExpr),
            _false: kw("false", startsExpr),
            _in: kw("in", {beforeExpr: true, binop: 7}),
            _instanceof: kw("instanceof", {beforeExpr: true, binop: 7}),
            _typeof: kw("typeof", {beforeExpr: true, prefix: true, startsExpr: true}),
            _void: kw("void", {beforeExpr: true, prefix: true, startsExpr: true}),
            _delete: kw("delete", {beforeExpr: true, prefix: true, startsExpr: true})
          };

          // Matches a whole line break (where CRLF is considered a single
          // line break). Used to count lines.

          var lineBreak = /\r\n?|\n|\u2028|\u2029/;
          var lineBreakG = new RegExp(lineBreak.source, "g");

          function isNewLine(code) {
            return code === 10 || code === 13 || code === 0x2028 || code === 0x2029
          }

          function nextLineBreak(code, from, end) {
            if ( end === void 0 ) end = code.length;

            for (var i = from; i < end; i++) {
              var next = code.charCodeAt(i);
              if (isNewLine(next))
                { return i < end - 1 && next === 13 && code.charCodeAt(i + 1) === 10 ? i + 2 : i + 1 }
            }
            return -1
          }

          var nonASCIIwhitespace = /[\u1680\u2000-\u200a\u202f\u205f\u3000\ufeff]/;

          var skipWhiteSpace = /(?:\s|\/\/.*|\/\*[^]*?\*\/)*/g;

          var ref = Object.prototype;
          var hasOwnProperty = ref.hasOwnProperty;
          var toString = ref.toString;

          var hasOwn = Object.hasOwn || (function (obj, propName) { return (
            hasOwnProperty.call(obj, propName)
          ); });

          var isArray = Array.isArray || (function (obj) { return (
            toString.call(obj) === "[object Array]"
          ); });

          var regexpCache = Object.create(null);

          function wordsRegexp(words) {
            return regexpCache[words] || (regexpCache[words] = new RegExp("^(?:" + words.replace(/ /g, "|") + ")$"))
          }

          function codePointToString(code) {
            // UTF-16 Decoding
            if (code <= 0xFFFF) { return String.fromCharCode(code) }
            code -= 0x10000;
            return String.fromCharCode((code >> 10) + 0xD800, (code & 1023) + 0xDC00)
          }

          var loneSurrogate = /(?:[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?:[^\uD800-\uDBFF]|^)[\uDC00-\uDFFF])/;

          // These are used when `options.locations` is on, for the
          // `startLoc` and `endLoc` properties.

          var Position = function Position(line, col) {
            this.line = line;
            this.column = col;
          };

          Position.prototype.offset = function offset (n) {
            return new Position(this.line, this.column + n)
          };

          var SourceLocation = function SourceLocation(p, start, end) {
            this.start = start;
            this.end = end;
            if (p.sourceFile !== null) { this.source = p.sourceFile; }
          };

          // The `getLineInfo` function is mostly useful when the
          // `locations` option is off (for performance reasons) and you
          // want to find the line/column position for a given character
          // offset. `input` should be the code string that the offset refers
          // into.

          function getLineInfo(input, offset) {
            for (var line = 1, cur = 0;;) {
              var nextBreak = nextLineBreak(input, cur, offset);
              if (nextBreak < 0) { return new Position(line, offset - cur) }
              ++line;
              cur = nextBreak;
            }
          }

          // A second argument must be given to configure the parser process.
          // These options are recognized (only `ecmaVersion` is required):

          var defaultOptions = {
            // `ecmaVersion` indicates the ECMAScript version to parse. Must be
            // either 3, 5, 6 (or 2015), 7 (2016), 8 (2017), 9 (2018), 10
            // (2019), 11 (2020), 12 (2021), 13 (2022), 14 (2023), or `"latest"`
            // (the latest version the library supports). This influences
            // support for strict mode, the set of reserved words, and support
            // for new syntax features.
            ecmaVersion: null,
            // `sourceType` indicates the mode the code should be parsed in.
            // Can be either `"script"` or `"module"`. This influences global
            // strict mode and parsing of `import` and `export` declarations.
            sourceType: "script",
            // `onInsertedSemicolon` can be a callback that will be called when
            // a semicolon is automatically inserted. It will be passed the
            // position of the inserted semicolon as an offset, and if
            // `locations` is enabled, it is given the location as a `{line,
            // column}` object as second argument.
            onInsertedSemicolon: null,
            // `onTrailingComma` is similar to `onInsertedSemicolon`, but for
            // trailing commas.
            onTrailingComma: null,
            // By default, reserved words are only enforced if ecmaVersion >= 5.
            // Set `allowReserved` to a boolean value to explicitly turn this on
            // an off. When this option has the value "never", reserved words
            // and keywords can also not be used as property names.
            allowReserved: null,
            // When enabled, a return at the top level is not considered an
            // error.
            allowReturnOutsideFunction: false,
            // When enabled, import/export statements are not constrained to
            // appearing at the top of the program, and an import.meta expression
            // in a script isn't considered an error.
            allowImportExportEverywhere: false,
            // By default, await identifiers are allowed to appear at the top-level scope only if ecmaVersion >= 2022.
            // When enabled, await identifiers are allowed to appear at the top-level scope,
            // but they are still not allowed in non-async functions.
            allowAwaitOutsideFunction: null,
            // When enabled, super identifiers are not constrained to
            // appearing in methods and do not raise an error when they appear elsewhere.
            allowSuperOutsideMethod: null,
            // When enabled, hashbang directive in the beginning of file is
            // allowed and treated as a line comment. Enabled by default when
            // `ecmaVersion` >= 2023.
            allowHashBang: false,
            // By default, the parser will verify that private properties are
            // only used in places where they are valid and have been declared.
            // Set this to false to turn such checks off.
            checkPrivateFields: true,
            // When `locations` is on, `loc` properties holding objects with
            // `start` and `end` properties in `{line, column}` form (with
            // line being 1-based and column 0-based) will be attached to the
            // nodes.
            locations: false,
            // A function can be passed as `onToken` option, which will
            // cause Acorn to call that function with object in the same
            // format as tokens returned from `tokenizer().getToken()`. Note
            // that you are not allowed to call the parser from the
            // callback—that will corrupt its internal state.
            onToken: null,
            // A function can be passed as `onComment` option, which will
            // cause Acorn to call that function with `(block, text, start,
            // end)` parameters whenever a comment is skipped. `block` is a
            // boolean indicating whether this is a block (`/* */`) comment,
            // `text` is the content of the comment, and `start` and `end` are
            // character offsets that denote the start and end of the comment.
            // When the `locations` option is on, two more parameters are
            // passed, the full `{line, column}` locations of the start and
            // end of the comments. Note that you are not allowed to call the
            // parser from the callback—that will corrupt its internal state.
            // When this option has an array as value, objects representing the
            // comments are pushed to it.
            onComment: null,
            // Nodes have their start and end characters offsets recorded in
            // `start` and `end` properties (directly on the node, rather than
            // the `loc` object, which holds line/column data. To also add a
            // [semi-standardized][range] `range` property holding a `[start,
            // end]` array with the same numbers, set the `ranges` option to
            // `true`.
            //
            // [range]: https://bugzilla.mozilla.org/show_bug.cgi?id=745678
            ranges: false,
            // It is possible to parse multiple files into a single AST by
            // passing the tree produced by parsing the first file as
            // `program` option in subsequent parses. This will add the
            // toplevel forms of the parsed file to the `Program` (top) node
            // of an existing parse tree.
            program: null,
            // When `locations` is on, you can pass this to record the source
            // file in every node's `loc` object.
            sourceFile: null,
            // This value, if given, is stored in every node, whether
            // `locations` is on or off.
            directSourceFile: null,
            // When enabled, parenthesized expressions are represented by
            // (non-standard) ParenthesizedExpression nodes
            preserveParens: false
          };

          // Interpret and default an options object

          var warnedAboutEcmaVersion = false;

          function getOptions(opts) {
            var options = {};

            for (var opt in defaultOptions)
              { options[opt] = opts && hasOwn(opts, opt) ? opts[opt] : defaultOptions[opt]; }

            if (options.ecmaVersion === "latest") {
              options.ecmaVersion = 1e8;
            } else if (options.ecmaVersion == null) {
              if (!warnedAboutEcmaVersion && typeof console === "object" && console.warn) {
                warnedAboutEcmaVersion = true;
                console.warn("Since Acorn 8.0.0, options.ecmaVersion is required.\nDefaulting to 2020, but this will stop working in the future.");
              }
              options.ecmaVersion = 11;
            } else if (options.ecmaVersion >= 2015) {
              options.ecmaVersion -= 2009;
            }

            if (options.allowReserved == null)
              { options.allowReserved = options.ecmaVersion < 5; }

            if (!opts || opts.allowHashBang == null)
              { options.allowHashBang = options.ecmaVersion >= 14; }

            if (isArray(options.onToken)) {
              var tokens = options.onToken;
              options.onToken = function (token) { return tokens.push(token); };
            }
            if (isArray(options.onComment))
              { options.onComment = pushComment(options, options.onComment); }

            return options
          }

          function pushComment(options, array) {
            return function(block, text, start, end, startLoc, endLoc) {
              var comment = {
                type: block ? "Block" : "Line",
                value: text,
                start: start,
                end: end
              };
              if (options.locations)
                { comment.loc = new SourceLocation(this, startLoc, endLoc); }
              if (options.ranges)
                { comment.range = [start, end]; }
              array.push(comment);
            }
          }

          // Each scope gets a bitset that may contain these flags
          var
              SCOPE_TOP = 1,
              SCOPE_FUNCTION = 2,
              SCOPE_ASYNC = 4,
              SCOPE_GENERATOR = 8,
              SCOPE_ARROW = 16,
              SCOPE_SIMPLE_CATCH = 32,
              SCOPE_SUPER = 64,
              SCOPE_DIRECT_SUPER = 128,
              SCOPE_CLASS_STATIC_BLOCK = 256,
              SCOPE_CLASS_FIELD_INIT = 512,
              SCOPE_VAR = SCOPE_TOP | SCOPE_FUNCTION | SCOPE_CLASS_STATIC_BLOCK;

          function functionFlags(async, generator) {
            return SCOPE_FUNCTION | (async ? SCOPE_ASYNC : 0) | (generator ? SCOPE_GENERATOR : 0)
          }

          // Used in checkLVal* and declareName to determine the type of a binding
          var
              BIND_NONE = 0, // Not a binding
              BIND_VAR = 1, // Var-style binding
              BIND_LEXICAL = 2, // Let- or const-style binding
              BIND_FUNCTION = 3, // Function declaration
              BIND_SIMPLE_CATCH = 4, // Simple (identifier pattern) catch binding
              BIND_OUTSIDE = 5; // Special case for function names as bound inside the function

          var Parser = function Parser(options, input, startPos) {
            this.options = options = getOptions(options);
            this.sourceFile = options.sourceFile;
            this.keywords = wordsRegexp(keywords$1[options.ecmaVersion >= 6 ? 6 : options.sourceType === "module" ? "5module" : 5]);
            var reserved = "";
            if (options.allowReserved !== true) {
              reserved = reservedWords[options.ecmaVersion >= 6 ? 6 : options.ecmaVersion === 5 ? 5 : 3];
              if (options.sourceType === "module") { reserved += " await"; }
            }
            this.reservedWords = wordsRegexp(reserved);
            var reservedStrict = (reserved ? reserved + " " : "") + reservedWords.strict;
            this.reservedWordsStrict = wordsRegexp(reservedStrict);
            this.reservedWordsStrictBind = wordsRegexp(reservedStrict + " " + reservedWords.strictBind);
            this.input = String(input);

            // Used to signal to callers of `readWord1` whether the word
            // contained any escape sequences. This is needed because words with
            // escape sequences must not be interpreted as keywords.
            this.containsEsc = false;

            // Set up token state

            // The current position of the tokenizer in the input.
            if (startPos) {
              this.pos = startPos;
              this.lineStart = this.input.lastIndexOf("\n", startPos - 1) + 1;
              this.curLine = this.input.slice(0, this.lineStart).split(lineBreak).length;
            } else {
              this.pos = this.lineStart = 0;
              this.curLine = 1;
            }

            // Properties of the current token:
            // Its type
            this.type = types$1.eof;
            // For tokens that include more information than their type, the value
            this.value = null;
            // Its start and end offset
            this.start = this.end = this.pos;
            // And, if locations are used, the {line, column} object
            // corresponding to those offsets
            this.startLoc = this.endLoc = this.curPosition();

            // Position information for the previous token
            this.lastTokEndLoc = this.lastTokStartLoc = null;
            this.lastTokStart = this.lastTokEnd = this.pos;

            // The context stack is used to superficially track syntactic
            // context to predict whether a regular expression is allowed in a
            // given position.
            this.context = this.initialContext();
            this.exprAllowed = true;

            // Figure out if it's a module code.
            this.inModule = options.sourceType === "module";
            this.strict = this.inModule || this.strictDirective(this.pos);

            // Used to signify the start of a potential arrow function
            this.potentialArrowAt = -1;
            this.potentialArrowInForAwait = false;

            // Positions to delayed-check that yield/await does not exist in default parameters.
            this.yieldPos = this.awaitPos = this.awaitIdentPos = 0;
            // Labels in scope.
            this.labels = [];
            // Thus-far undefined exports.
            this.undefinedExports = Object.create(null);

            // If enabled, skip leading hashbang line.
            if (this.pos === 0 && options.allowHashBang && this.input.slice(0, 2) === "#!")
              { this.skipLineComment(2); }

            // Scope tracking for duplicate variable names (see scope.js)
            this.scopeStack = [];
            this.enterScope(SCOPE_TOP);

            // For RegExp validation
            this.regexpState = null;

            // The stack of private names.
            // Each element has two properties: 'declared' and 'used'.
            // When it exited from the outermost class definition, all used private names must be declared.
            this.privateNameStack = [];
          };

          var prototypeAccessors = { inFunction: { configurable: true },inGenerator: { configurable: true },inAsync: { configurable: true },canAwait: { configurable: true },allowSuper: { configurable: true },allowDirectSuper: { configurable: true },treatFunctionsAsVar: { configurable: true },allowNewDotTarget: { configurable: true },inClassStaticBlock: { configurable: true } };

          Parser.prototype.parse = function parse () {
            var node = this.options.program || this.startNode();
            this.nextToken();
            return this.parseTopLevel(node)
          };

          prototypeAccessors.inFunction.get = function () { return (this.currentVarScope().flags & SCOPE_FUNCTION) > 0 };

          prototypeAccessors.inGenerator.get = function () { return (this.currentVarScope().flags & SCOPE_GENERATOR) > 0 };

          prototypeAccessors.inAsync.get = function () { return (this.currentVarScope().flags & SCOPE_ASYNC) > 0 };

          prototypeAccessors.canAwait.get = function () {
            for (var i = this.scopeStack.length - 1; i >= 0; i--) {
              var ref = this.scopeStack[i];
                var flags = ref.flags;
              if (flags & (SCOPE_CLASS_STATIC_BLOCK | SCOPE_CLASS_FIELD_INIT)) { return false }
              if (flags & SCOPE_FUNCTION) { return (flags & SCOPE_ASYNC) > 0 }
            }
            return (this.inModule && this.options.ecmaVersion >= 13) || this.options.allowAwaitOutsideFunction
          };

          prototypeAccessors.allowSuper.get = function () {
            var ref = this.currentThisScope();
              var flags = ref.flags;
            return (flags & SCOPE_SUPER) > 0 || this.options.allowSuperOutsideMethod
          };

          prototypeAccessors.allowDirectSuper.get = function () { return (this.currentThisScope().flags & SCOPE_DIRECT_SUPER) > 0 };

          prototypeAccessors.treatFunctionsAsVar.get = function () { return this.treatFunctionsAsVarInScope(this.currentScope()) };

          prototypeAccessors.allowNewDotTarget.get = function () {
            for (var i = this.scopeStack.length - 1; i >= 0; i--) {
              var ref = this.scopeStack[i];
                var flags = ref.flags;
              if (flags & (SCOPE_CLASS_STATIC_BLOCK | SCOPE_CLASS_FIELD_INIT) ||
                  ((flags & SCOPE_FUNCTION) && !(flags & SCOPE_ARROW))) { return true }
            }
            return false
          };

          prototypeAccessors.inClassStaticBlock.get = function () {
            return (this.currentVarScope().flags & SCOPE_CLASS_STATIC_BLOCK) > 0
          };

          Parser.extend = function extend () {
              var plugins = [], len = arguments.length;
              while ( len-- ) plugins[ len ] = arguments[ len ];

            var cls = this;
            for (var i = 0; i < plugins.length; i++) { cls = plugins[i](cls); }
            return cls
          };

          Parser.parse = function parse (input, options) {
            return new this(options, input).parse()
          };

          Parser.parseExpressionAt = function parseExpressionAt (input, pos, options) {
            var parser = new this(options, input, pos);
            parser.nextToken();
            return parser.parseExpression()
          };

          Parser.tokenizer = function tokenizer (input, options) {
            return new this(options, input)
          };

          Object.defineProperties( Parser.prototype, prototypeAccessors );

          var pp$9 = Parser.prototype;

          // ## Parser utilities

          var literal = /^(?:'((?:\\[^]|[^'\\])*?)'|"((?:\\[^]|[^"\\])*?)")/;
          pp$9.strictDirective = function(start) {
            if (this.options.ecmaVersion < 5) { return false }
            for (;;) {
              // Try to find string literal.
              skipWhiteSpace.lastIndex = start;
              start += skipWhiteSpace.exec(this.input)[0].length;
              var match = literal.exec(this.input.slice(start));
              if (!match) { return false }
              if ((match[1] || match[2]) === "use strict") {
                skipWhiteSpace.lastIndex = start + match[0].length;
                var spaceAfter = skipWhiteSpace.exec(this.input), end = spaceAfter.index + spaceAfter[0].length;
                var next = this.input.charAt(end);
                return next === ";" || next === "}" ||
                  (lineBreak.test(spaceAfter[0]) &&
                   !(/[(`.[+\-/*%<>=,?^&]/.test(next) || next === "!" && this.input.charAt(end + 1) === "="))
              }
              start += match[0].length;

              // Skip semicolon, if any.
              skipWhiteSpace.lastIndex = start;
              start += skipWhiteSpace.exec(this.input)[0].length;
              if (this.input[start] === ";")
                { start++; }
            }
          };

          // Predicate that tests whether the next token is of the given
          // type, and if yes, consumes it as a side effect.

          pp$9.eat = function(type) {
            if (this.type === type) {
              this.next();
              return true
            } else {
              return false
            }
          };

          // Tests whether parsed token is a contextual keyword.

          pp$9.isContextual = function(name) {
            return this.type === types$1.name && this.value === name && !this.containsEsc
          };

          // Consumes contextual keyword if possible.

          pp$9.eatContextual = function(name) {
            if (!this.isContextual(name)) { return false }
            this.next();
            return true
          };

          // Asserts that following token is given contextual keyword.

          pp$9.expectContextual = function(name) {
            if (!this.eatContextual(name)) { this.unexpected(); }
          };

          // Test whether a semicolon can be inserted at the current position.

          pp$9.canInsertSemicolon = function() {
            return this.type === types$1.eof ||
              this.type === types$1.braceR ||
              lineBreak.test(this.input.slice(this.lastTokEnd, this.start))
          };

          pp$9.insertSemicolon = function() {
            if (this.canInsertSemicolon()) {
              if (this.options.onInsertedSemicolon)
                { this.options.onInsertedSemicolon(this.lastTokEnd, this.lastTokEndLoc); }
              return true
            }
          };

          // Consume a semicolon, or, failing that, see if we are allowed to
          // pretend that there is a semicolon at this position.

          pp$9.semicolon = function() {
            if (!this.eat(types$1.semi) && !this.insertSemicolon()) { this.unexpected(); }
          };

          pp$9.afterTrailingComma = function(tokType, notNext) {
            if (this.type === tokType) {
              if (this.options.onTrailingComma)
                { this.options.onTrailingComma(this.lastTokStart, this.lastTokStartLoc); }
              if (!notNext)
                { this.next(); }
              return true
            }
          };

          // Expect a token of a given type. If found, consume it, otherwise,
          // raise an unexpected token error.

          pp$9.expect = function(type) {
            this.eat(type) || this.unexpected();
          };

          // Raise an unexpected token error.

          pp$9.unexpected = function(pos) {
            this.raise(pos != null ? pos : this.start, "Unexpected token");
          };

          var DestructuringErrors = function DestructuringErrors() {
            this.shorthandAssign =
            this.trailingComma =
            this.parenthesizedAssign =
            this.parenthesizedBind =
            this.doubleProto =
              -1;
          };

          pp$9.checkPatternErrors = function(refDestructuringErrors, isAssign) {
            if (!refDestructuringErrors) { return }
            if (refDestructuringErrors.trailingComma > -1)
              { this.raiseRecoverable(refDestructuringErrors.trailingComma, "Comma is not permitted after the rest element"); }
            var parens = isAssign ? refDestructuringErrors.parenthesizedAssign : refDestructuringErrors.parenthesizedBind;
            if (parens > -1) { this.raiseRecoverable(parens, isAssign ? "Assigning to rvalue" : "Parenthesized pattern"); }
          };

          pp$9.checkExpressionErrors = function(refDestructuringErrors, andThrow) {
            if (!refDestructuringErrors) { return false }
            var shorthandAssign = refDestructuringErrors.shorthandAssign;
            var doubleProto = refDestructuringErrors.doubleProto;
            if (!andThrow) { return shorthandAssign >= 0 || doubleProto >= 0 }
            if (shorthandAssign >= 0)
              { this.raise(shorthandAssign, "Shorthand property assignments are valid only in destructuring patterns"); }
            if (doubleProto >= 0)
              { this.raiseRecoverable(doubleProto, "Redefinition of __proto__ property"); }
          };

          pp$9.checkYieldAwaitInDefaultParams = function() {
            if (this.yieldPos && (!this.awaitPos || this.yieldPos < this.awaitPos))
              { this.raise(this.yieldPos, "Yield expression cannot be a default value"); }
            if (this.awaitPos)
              { this.raise(this.awaitPos, "Await expression cannot be a default value"); }
          };

          pp$9.isSimpleAssignTarget = function(expr) {
            if (expr.type === "ParenthesizedExpression")
              { return this.isSimpleAssignTarget(expr.expression) }
            return expr.type === "Identifier" || expr.type === "MemberExpression"
          };

          var pp$8 = Parser.prototype;

          // ### Statement parsing

          // Parse a program. Initializes the parser, reads any number of
          // statements, and wraps them in a Program node.  Optionally takes a
          // `program` argument.  If present, the statements will be appended
          // to its body instead of creating a new node.

          pp$8.parseTopLevel = function(node) {
            var exports = Object.create(null);
            if (!node.body) { node.body = []; }
            while (this.type !== types$1.eof) {
              var stmt = this.parseStatement(null, true, exports);
              node.body.push(stmt);
            }
            if (this.inModule)
              { for (var i = 0, list = Object.keys(this.undefinedExports); i < list.length; i += 1)
                {
                  var name = list[i];

                  this.raiseRecoverable(this.undefinedExports[name].start, ("Export '" + name + "' is not defined"));
                } }
            this.adaptDirectivePrologue(node.body);
            this.next();
            node.sourceType = this.options.sourceType;
            return this.finishNode(node, "Program")
          };

          var loopLabel = {kind: "loop"}, switchLabel = {kind: "switch"};

          pp$8.isLet = function(context) {
            if (this.options.ecmaVersion < 6 || !this.isContextual("let")) { return false }
            skipWhiteSpace.lastIndex = this.pos;
            var skip = skipWhiteSpace.exec(this.input);
            var next = this.pos + skip[0].length, nextCh = this.input.charCodeAt(next);
            // For ambiguous cases, determine if a LexicalDeclaration (or only a
            // Statement) is allowed here. If context is not empty then only a Statement
            // is allowed. However, `let [` is an explicit negative lookahead for
            // ExpressionStatement, so special-case it first.
            if (nextCh === 91 || nextCh === 92) { return true } // '[', '\'
            if (context) { return false }

            if (nextCh === 123 || nextCh > 0xd7ff && nextCh < 0xdc00) { return true } // '{', astral
            if (isIdentifierStart(nextCh, true)) {
              var pos = next + 1;
              while (isIdentifierChar(nextCh = this.input.charCodeAt(pos), true)) { ++pos; }
              if (nextCh === 92 || nextCh > 0xd7ff && nextCh < 0xdc00) { return true }
              var ident = this.input.slice(next, pos);
              if (!keywordRelationalOperator.test(ident)) { return true }
            }
            return false
          };

          // check 'async [no LineTerminator here] function'
          // - 'async /*foo*/ function' is OK.
          // - 'async /*\n*/ function' is invalid.
          pp$8.isAsyncFunction = function() {
            if (this.options.ecmaVersion < 8 || !this.isContextual("async"))
              { return false }

            skipWhiteSpace.lastIndex = this.pos;
            var skip = skipWhiteSpace.exec(this.input);
            var next = this.pos + skip[0].length, after;
            return !lineBreak.test(this.input.slice(this.pos, next)) &&
              this.input.slice(next, next + 8) === "function" &&
              (next + 8 === this.input.length ||
               !(isIdentifierChar(after = this.input.charCodeAt(next + 8)) || after > 0xd7ff && after < 0xdc00))
          };

          pp$8.isUsingKeyword = function(isAwaitUsing, isFor) {
            if (this.options.ecmaVersion < 17 || !this.isContextual(isAwaitUsing ? "await" : "using"))
              { return false }

            skipWhiteSpace.lastIndex = this.pos;
            var skip = skipWhiteSpace.exec(this.input);
            var next = this.pos + skip[0].length;

            if (lineBreak.test(this.input.slice(this.pos, next))) { return false }

            if (isAwaitUsing) {
              var awaitEndPos = next + 5 /* await */, after;
              if (this.input.slice(next, awaitEndPos) !== "using" ||
                awaitEndPos === this.input.length ||
                isIdentifierChar(after = this.input.charCodeAt(awaitEndPos)) ||
                (after > 0xd7ff && after < 0xdc00)
              ) { return false }

              skipWhiteSpace.lastIndex = awaitEndPos;
              var skipAfterUsing = skipWhiteSpace.exec(this.input);
              if (skipAfterUsing && lineBreak.test(this.input.slice(awaitEndPos, awaitEndPos + skipAfterUsing[0].length))) { return false }
            }

            if (isFor) {
              var ofEndPos = next + 2 /* of */, after$1;
              if (this.input.slice(next, ofEndPos) === "of") {
                if (ofEndPos === this.input.length ||
                  (!isIdentifierChar(after$1 = this.input.charCodeAt(ofEndPos)) && !(after$1 > 0xd7ff && after$1 < 0xdc00))) { return false }
              }
            }

            var ch = this.input.charCodeAt(next);
            return isIdentifierStart(ch, true) || ch === 92 // '\'
          };

          pp$8.isAwaitUsing = function(isFor) {
            return this.isUsingKeyword(true, isFor)
          };

          pp$8.isUsing = function(isFor) {
            return this.isUsingKeyword(false, isFor)
          };

          // Parse a single statement.
          //
          // If expecting a statement and finding a slash operator, parse a
          // regular expression literal. This is to handle cases like
          // `if (foo) /blah/.exec(foo)`, where looking at the previous token
          // does not help.

          pp$8.parseStatement = function(context, topLevel, exports) {
            var starttype = this.type, node = this.startNode(), kind;

            if (this.isLet(context)) {
              starttype = types$1._var;
              kind = "let";
            }

            // Most types of statements are recognized by the keyword they
            // start with. Many are trivial to parse, some require a bit of
            // complexity.

            switch (starttype) {
            case types$1._break: case types$1._continue: return this.parseBreakContinueStatement(node, starttype.keyword)
            case types$1._debugger: return this.parseDebuggerStatement(node)
            case types$1._do: return this.parseDoStatement(node)
            case types$1._for: return this.parseForStatement(node)
            case types$1._function:
              // Function as sole body of either an if statement or a labeled statement
              // works, but not when it is part of a labeled statement that is the sole
              // body of an if statement.
              if ((context && (this.strict || context !== "if" && context !== "label")) && this.options.ecmaVersion >= 6) { this.unexpected(); }
              return this.parseFunctionStatement(node, false, !context)
            case types$1._class:
              if (context) { this.unexpected(); }
              return this.parseClass(node, true)
            case types$1._if: return this.parseIfStatement(node)
            case types$1._return: return this.parseReturnStatement(node)
            case types$1._switch: return this.parseSwitchStatement(node)
            case types$1._throw: return this.parseThrowStatement(node)
            case types$1._try: return this.parseTryStatement(node)
            case types$1._const: case types$1._var:
              kind = kind || this.value;
              if (context && kind !== "var") { this.unexpected(); }
              return this.parseVarStatement(node, kind)
            case types$1._while: return this.parseWhileStatement(node)
            case types$1._with: return this.parseWithStatement(node)
            case types$1.braceL: return this.parseBlock(true, node)
            case types$1.semi: return this.parseEmptyStatement(node)
            case types$1._export:
            case types$1._import:
              if (this.options.ecmaVersion > 10 && starttype === types$1._import) {
                skipWhiteSpace.lastIndex = this.pos;
                var skip = skipWhiteSpace.exec(this.input);
                var next = this.pos + skip[0].length, nextCh = this.input.charCodeAt(next);
                if (nextCh === 40 || nextCh === 46) // '(' or '.'
                  { return this.parseExpressionStatement(node, this.parseExpression()) }
              }

              if (!this.options.allowImportExportEverywhere) {
                if (!topLevel)
                  { this.raise(this.start, "'import' and 'export' may only appear at the top level"); }
                if (!this.inModule)
                  { this.raise(this.start, "'import' and 'export' may appear only with 'sourceType: module'"); }
              }
              return starttype === types$1._import ? this.parseImport(node) : this.parseExport(node, exports)

              // If the statement does not start with a statement keyword or a
              // brace, it's an ExpressionStatement or LabeledStatement. We
              // simply start parsing an expression, and afterwards, if the
              // next token is a colon and the expression was a simple
              // Identifier node, we switch to interpreting it as a label.
            default:
              if (this.isAsyncFunction()) {
                if (context) { this.unexpected(); }
                this.next();
                return this.parseFunctionStatement(node, true, !context)
              }

              var usingKind = this.isAwaitUsing(false) ? "await using" : this.isUsing(false) ? "using" : null;
              if (usingKind) {
                if (topLevel && this.options.sourceType === "script") {
                  this.raise(this.start, "Using declaration cannot appear in the top level when source type is `script`");
                }
                if (usingKind === "await using") {
                  if (!this.canAwait) {
                    this.raise(this.start, "Await using cannot appear outside of async function");
                  }
                  this.next();
                }
                this.next();
                this.parseVar(node, false, usingKind);
                this.semicolon();
                return this.finishNode(node, "VariableDeclaration")
              }

              var maybeName = this.value, expr = this.parseExpression();
              if (starttype === types$1.name && expr.type === "Identifier" && this.eat(types$1.colon))
                { return this.parseLabeledStatement(node, maybeName, expr, context) }
              else { return this.parseExpressionStatement(node, expr) }
            }
          };

          pp$8.parseBreakContinueStatement = function(node, keyword) {
            var isBreak = keyword === "break";
            this.next();
            if (this.eat(types$1.semi) || this.insertSemicolon()) { node.label = null; }
            else if (this.type !== types$1.name) { this.unexpected(); }
            else {
              node.label = this.parseIdent();
              this.semicolon();
            }

            // Verify that there is an actual destination to break or
            // continue to.
            var i = 0;
            for (; i < this.labels.length; ++i) {
              var lab = this.labels[i];
              if (node.label == null || lab.name === node.label.name) {
                if (lab.kind != null && (isBreak || lab.kind === "loop")) { break }
                if (node.label && isBreak) { break }
              }
            }
            if (i === this.labels.length) { this.raise(node.start, "Unsyntactic " + keyword); }
            return this.finishNode(node, isBreak ? "BreakStatement" : "ContinueStatement")
          };

          pp$8.parseDebuggerStatement = function(node) {
            this.next();
            this.semicolon();
            return this.finishNode(node, "DebuggerStatement")
          };

          pp$8.parseDoStatement = function(node) {
            this.next();
            this.labels.push(loopLabel);
            node.body = this.parseStatement("do");
            this.labels.pop();
            this.expect(types$1._while);
            node.test = this.parseParenExpression();
            if (this.options.ecmaVersion >= 6)
              { this.eat(types$1.semi); }
            else
              { this.semicolon(); }
            return this.finishNode(node, "DoWhileStatement")
          };

          // Disambiguating between a `for` and a `for`/`in` or `for`/`of`
          // loop is non-trivial. Basically, we have to parse the init `var`
          // statement or expression, disallowing the `in` operator (see
          // the second parameter to `parseExpression`), and then check
          // whether the next token is `in` or `of`. When there is no init
          // part (semicolon immediately after the opening parenthesis), it
          // is a regular `for` loop.

          pp$8.parseForStatement = function(node) {
            this.next();
            var awaitAt = (this.options.ecmaVersion >= 9 && this.canAwait && this.eatContextual("await")) ? this.lastTokStart : -1;
            this.labels.push(loopLabel);
            this.enterScope(0);
            this.expect(types$1.parenL);
            if (this.type === types$1.semi) {
              if (awaitAt > -1) { this.unexpected(awaitAt); }
              return this.parseFor(node, null)
            }
            var isLet = this.isLet();
            if (this.type === types$1._var || this.type === types$1._const || isLet) {
              var init$1 = this.startNode(), kind = isLet ? "let" : this.value;
              this.next();
              this.parseVar(init$1, true, kind);
              this.finishNode(init$1, "VariableDeclaration");
              return this.parseForAfterInit(node, init$1, awaitAt)
            }
            var startsWithLet = this.isContextual("let"), isForOf = false;

            var usingKind = this.isUsing(true) ? "using" : this.isAwaitUsing(true) ? "await using" : null;
            if (usingKind) {
              var init$2 = this.startNode();
              this.next();
              if (usingKind === "await using") { this.next(); }
              this.parseVar(init$2, true, usingKind);
              this.finishNode(init$2, "VariableDeclaration");
              return this.parseForAfterInit(node, init$2, awaitAt)
            }
            var containsEsc = this.containsEsc;
            var refDestructuringErrors = new DestructuringErrors;
            var initPos = this.start;
            var init = awaitAt > -1
              ? this.parseExprSubscripts(refDestructuringErrors, "await")
              : this.parseExpression(true, refDestructuringErrors);
            if (this.type === types$1._in || (isForOf = this.options.ecmaVersion >= 6 && this.isContextual("of"))) {
              if (awaitAt > -1) { // implies `ecmaVersion >= 9` (see declaration of awaitAt)
                if (this.type === types$1._in) { this.unexpected(awaitAt); }
                node.await = true;
              } else if (isForOf && this.options.ecmaVersion >= 8) {
                if (init.start === initPos && !containsEsc && init.type === "Identifier" && init.name === "async") { this.unexpected(); }
                else if (this.options.ecmaVersion >= 9) { node.await = false; }
              }
              if (startsWithLet && isForOf) { this.raise(init.start, "The left-hand side of a for-of loop may not start with 'let'."); }
              this.toAssignable(init, false, refDestructuringErrors);
              this.checkLValPattern(init);
              return this.parseForIn(node, init)
            } else {
              this.checkExpressionErrors(refDestructuringErrors, true);
            }
            if (awaitAt > -1) { this.unexpected(awaitAt); }
            return this.parseFor(node, init)
          };

          // Helper method to parse for loop after variable initialization
          pp$8.parseForAfterInit = function(node, init, awaitAt) {
            if ((this.type === types$1._in || (this.options.ecmaVersion >= 6 && this.isContextual("of"))) && init.declarations.length === 1) {
              if (this.options.ecmaVersion >= 9) {
                if (this.type === types$1._in) {
                  if (awaitAt > -1) { this.unexpected(awaitAt); }
                } else { node.await = awaitAt > -1; }
              }
              return this.parseForIn(node, init)
            }
            if (awaitAt > -1) { this.unexpected(awaitAt); }
            return this.parseFor(node, init)
          };

          pp$8.parseFunctionStatement = function(node, isAsync, declarationPosition) {
            this.next();
            return this.parseFunction(node, FUNC_STATEMENT | (declarationPosition ? 0 : FUNC_HANGING_STATEMENT), false, isAsync)
          };

          pp$8.parseIfStatement = function(node) {
            this.next();
            node.test = this.parseParenExpression();
            // allow function declarations in branches, but only in non-strict mode
            node.consequent = this.parseStatement("if");
            node.alternate = this.eat(types$1._else) ? this.parseStatement("if") : null;
            return this.finishNode(node, "IfStatement")
          };

          pp$8.parseReturnStatement = function(node) {
            if (!this.inFunction && !this.options.allowReturnOutsideFunction)
              { this.raise(this.start, "'return' outside of function"); }
            this.next();

            // In `return` (and `break`/`continue`), the keywords with
            // optional arguments, we eagerly look for a semicolon or the
            // possibility to insert one.

            if (this.eat(types$1.semi) || this.insertSemicolon()) { node.argument = null; }
            else { node.argument = this.parseExpression(); this.semicolon(); }
            return this.finishNode(node, "ReturnStatement")
          };

          pp$8.parseSwitchStatement = function(node) {
            this.next();
            node.discriminant = this.parseParenExpression();
            node.cases = [];
            this.expect(types$1.braceL);
            this.labels.push(switchLabel);
            this.enterScope(0);

            // Statements under must be grouped (by label) in SwitchCase
            // nodes. `cur` is used to keep the node that we are currently
            // adding statements to.

            var cur;
            for (var sawDefault = false; this.type !== types$1.braceR;) {
              if (this.type === types$1._case || this.type === types$1._default) {
                var isCase = this.type === types$1._case;
                if (cur) { this.finishNode(cur, "SwitchCase"); }
                node.cases.push(cur = this.startNode());
                cur.consequent = [];
                this.next();
                if (isCase) {
                  cur.test = this.parseExpression();
                } else {
                  if (sawDefault) { this.raiseRecoverable(this.lastTokStart, "Multiple default clauses"); }
                  sawDefault = true;
                  cur.test = null;
                }
                this.expect(types$1.colon);
              } else {
                if (!cur) { this.unexpected(); }
                cur.consequent.push(this.parseStatement(null));
              }
            }
            this.exitScope();
            if (cur) { this.finishNode(cur, "SwitchCase"); }
            this.next(); // Closing brace
            this.labels.pop();
            return this.finishNode(node, "SwitchStatement")
          };

          pp$8.parseThrowStatement = function(node) {
            this.next();
            if (lineBreak.test(this.input.slice(this.lastTokEnd, this.start)))
              { this.raise(this.lastTokEnd, "Illegal newline after throw"); }
            node.argument = this.parseExpression();
            this.semicolon();
            return this.finishNode(node, "ThrowStatement")
          };

          // Reused empty array added for node fields that are always empty.

          var empty$1 = [];

          pp$8.parseCatchClauseParam = function() {
            var param = this.parseBindingAtom();
            var simple = param.type === "Identifier";
            this.enterScope(simple ? SCOPE_SIMPLE_CATCH : 0);
            this.checkLValPattern(param, simple ? BIND_SIMPLE_CATCH : BIND_LEXICAL);
            this.expect(types$1.parenR);

            return param
          };

          pp$8.parseTryStatement = function(node) {
            this.next();
            node.block = this.parseBlock();
            node.handler = null;
            if (this.type === types$1._catch) {
              var clause = this.startNode();
              this.next();
              if (this.eat(types$1.parenL)) {
                clause.param = this.parseCatchClauseParam();
              } else {
                if (this.options.ecmaVersion < 10) { this.unexpected(); }
                clause.param = null;
                this.enterScope(0);
              }
              clause.body = this.parseBlock(false);
              this.exitScope();
              node.handler = this.finishNode(clause, "CatchClause");
            }
            node.finalizer = this.eat(types$1._finally) ? this.parseBlock() : null;
            if (!node.handler && !node.finalizer)
              { this.raise(node.start, "Missing catch or finally clause"); }
            return this.finishNode(node, "TryStatement")
          };

          pp$8.parseVarStatement = function(node, kind, allowMissingInitializer) {
            this.next();
            this.parseVar(node, false, kind, allowMissingInitializer);
            this.semicolon();
            return this.finishNode(node, "VariableDeclaration")
          };

          pp$8.parseWhileStatement = function(node) {
            this.next();
            node.test = this.parseParenExpression();
            this.labels.push(loopLabel);
            node.body = this.parseStatement("while");
            this.labels.pop();
            return this.finishNode(node, "WhileStatement")
          };

          pp$8.parseWithStatement = function(node) {
            if (this.strict) { this.raise(this.start, "'with' in strict mode"); }
            this.next();
            node.object = this.parseParenExpression();
            node.body = this.parseStatement("with");
            return this.finishNode(node, "WithStatement")
          };

          pp$8.parseEmptyStatement = function(node) {
            this.next();
            return this.finishNode(node, "EmptyStatement")
          };

          pp$8.parseLabeledStatement = function(node, maybeName, expr, context) {
            for (var i$1 = 0, list = this.labels; i$1 < list.length; i$1 += 1)
              {
              var label = list[i$1];

              if (label.name === maybeName)
                { this.raise(expr.start, "Label '" + maybeName + "' is already declared");
            } }
            var kind = this.type.isLoop ? "loop" : this.type === types$1._switch ? "switch" : null;
            for (var i = this.labels.length - 1; i >= 0; i--) {
              var label$1 = this.labels[i];
              if (label$1.statementStart === node.start) {
                // Update information about previous labels on this node
                label$1.statementStart = this.start;
                label$1.kind = kind;
              } else { break }
            }
            this.labels.push({name: maybeName, kind: kind, statementStart: this.start});
            node.body = this.parseStatement(context ? context.indexOf("label") === -1 ? context + "label" : context : "label");
            this.labels.pop();
            node.label = expr;
            return this.finishNode(node, "LabeledStatement")
          };

          pp$8.parseExpressionStatement = function(node, expr) {
            node.expression = expr;
            this.semicolon();
            return this.finishNode(node, "ExpressionStatement")
          };

          // Parse a semicolon-enclosed block of statements, handling `"use
          // strict"` declarations when `allowStrict` is true (used for
          // function bodies).

          pp$8.parseBlock = function(createNewLexicalScope, node, exitStrict) {
            if ( createNewLexicalScope === void 0 ) createNewLexicalScope = true;
            if ( node === void 0 ) node = this.startNode();

            node.body = [];
            this.expect(types$1.braceL);
            if (createNewLexicalScope) { this.enterScope(0); }
            while (this.type !== types$1.braceR) {
              var stmt = this.parseStatement(null);
              node.body.push(stmt);
            }
            if (exitStrict) { this.strict = false; }
            this.next();
            if (createNewLexicalScope) { this.exitScope(); }
            return this.finishNode(node, "BlockStatement")
          };

          // Parse a regular `for` loop. The disambiguation code in
          // `parseStatement` will already have parsed the init statement or
          // expression.

          pp$8.parseFor = function(node, init) {
            node.init = init;
            this.expect(types$1.semi);
            node.test = this.type === types$1.semi ? null : this.parseExpression();
            this.expect(types$1.semi);
            node.update = this.type === types$1.parenR ? null : this.parseExpression();
            this.expect(types$1.parenR);
            node.body = this.parseStatement("for");
            this.exitScope();
            this.labels.pop();
            return this.finishNode(node, "ForStatement")
          };

          // Parse a `for`/`in` and `for`/`of` loop, which are almost
          // same from parser's perspective.

          pp$8.parseForIn = function(node, init) {
            var isForIn = this.type === types$1._in;
            this.next();

            if (
              init.type === "VariableDeclaration" &&
              init.declarations[0].init != null &&
              (
                !isForIn ||
                this.options.ecmaVersion < 8 ||
                this.strict ||
                init.kind !== "var" ||
                init.declarations[0].id.type !== "Identifier"
              )
            ) {
              this.raise(
                init.start,
                ((isForIn ? "for-in" : "for-of") + " loop variable declaration may not have an initializer")
              );
            }
            node.left = init;
            node.right = isForIn ? this.parseExpression() : this.parseMaybeAssign();
            this.expect(types$1.parenR);
            node.body = this.parseStatement("for");
            this.exitScope();
            this.labels.pop();
            return this.finishNode(node, isForIn ? "ForInStatement" : "ForOfStatement")
          };

          // Parse a list of variable declarations.

          pp$8.parseVar = function(node, isFor, kind, allowMissingInitializer) {
            node.declarations = [];
            node.kind = kind;
            for (;;) {
              var decl = this.startNode();
              this.parseVarId(decl, kind);
              if (this.eat(types$1.eq)) {
                decl.init = this.parseMaybeAssign(isFor);
              } else if (!allowMissingInitializer && kind === "const" && !(this.type === types$1._in || (this.options.ecmaVersion >= 6 && this.isContextual("of")))) {
                this.unexpected();
              } else if (!allowMissingInitializer && (kind === "using" || kind === "await using") && this.options.ecmaVersion >= 17 && this.type !== types$1._in && !this.isContextual("of")) {
                this.raise(this.lastTokEnd, ("Missing initializer in " + kind + " declaration"));
              } else if (!allowMissingInitializer && decl.id.type !== "Identifier" && !(isFor && (this.type === types$1._in || this.isContextual("of")))) {
                this.raise(this.lastTokEnd, "Complex binding patterns require an initialization value");
              } else {
                decl.init = null;
              }
              node.declarations.push(this.finishNode(decl, "VariableDeclarator"));
              if (!this.eat(types$1.comma)) { break }
            }
            return node
          };

          pp$8.parseVarId = function(decl, kind) {
            decl.id = kind === "using" || kind === "await using"
              ? this.parseIdent()
              : this.parseBindingAtom();

            this.checkLValPattern(decl.id, kind === "var" ? BIND_VAR : BIND_LEXICAL, false);
          };

          var FUNC_STATEMENT = 1, FUNC_HANGING_STATEMENT = 2, FUNC_NULLABLE_ID = 4;

          // Parse a function declaration or literal (depending on the
          // `statement & FUNC_STATEMENT`).

          // Remove `allowExpressionBody` for 7.0.0, as it is only called with false
          pp$8.parseFunction = function(node, statement, allowExpressionBody, isAsync, forInit) {
            this.initFunction(node);
            if (this.options.ecmaVersion >= 9 || this.options.ecmaVersion >= 6 && !isAsync) {
              if (this.type === types$1.star && (statement & FUNC_HANGING_STATEMENT))
                { this.unexpected(); }
              node.generator = this.eat(types$1.star);
            }
            if (this.options.ecmaVersion >= 8)
              { node.async = !!isAsync; }

            if (statement & FUNC_STATEMENT) {
              node.id = (statement & FUNC_NULLABLE_ID) && this.type !== types$1.name ? null : this.parseIdent();
              if (node.id && !(statement & FUNC_HANGING_STATEMENT))
                // If it is a regular function declaration in sloppy mode, then it is
                // subject to Annex B semantics (BIND_FUNCTION). Otherwise, the binding
                // mode depends on properties of the current scope (see
                // treatFunctionsAsVar).
                { this.checkLValSimple(node.id, (this.strict || node.generator || node.async) ? this.treatFunctionsAsVar ? BIND_VAR : BIND_LEXICAL : BIND_FUNCTION); }
            }

            var oldYieldPos = this.yieldPos, oldAwaitPos = this.awaitPos, oldAwaitIdentPos = this.awaitIdentPos;
            this.yieldPos = 0;
            this.awaitPos = 0;
            this.awaitIdentPos = 0;
            this.enterScope(functionFlags(node.async, node.generator));

            if (!(statement & FUNC_STATEMENT))
              { node.id = this.type === types$1.name ? this.parseIdent() : null; }

            this.parseFunctionParams(node);
            this.parseFunctionBody(node, allowExpressionBody, false, forInit);

            this.yieldPos = oldYieldPos;
            this.awaitPos = oldAwaitPos;
            this.awaitIdentPos = oldAwaitIdentPos;
            return this.finishNode(node, (statement & FUNC_STATEMENT) ? "FunctionDeclaration" : "FunctionExpression")
          };

          pp$8.parseFunctionParams = function(node) {
            this.expect(types$1.parenL);
            node.params = this.parseBindingList(types$1.parenR, false, this.options.ecmaVersion >= 8);
            this.checkYieldAwaitInDefaultParams();
          };

          // Parse a class declaration or literal (depending on the
          // `isStatement` parameter).

          pp$8.parseClass = function(node, isStatement) {
            this.next();

            // ecma-262 14.6 Class Definitions
            // A class definition is always strict mode code.
            var oldStrict = this.strict;
            this.strict = true;

            this.parseClassId(node, isStatement);
            this.parseClassSuper(node);
            var privateNameMap = this.enterClassBody();
            var classBody = this.startNode();
            var hadConstructor = false;
            classBody.body = [];
            this.expect(types$1.braceL);
            while (this.type !== types$1.braceR) {
              var element = this.parseClassElement(node.superClass !== null);
              if (element) {
                classBody.body.push(element);
                if (element.type === "MethodDefinition" && element.kind === "constructor") {
                  if (hadConstructor) { this.raiseRecoverable(element.start, "Duplicate constructor in the same class"); }
                  hadConstructor = true;
                } else if (element.key && element.key.type === "PrivateIdentifier" && isPrivateNameConflicted(privateNameMap, element)) {
                  this.raiseRecoverable(element.key.start, ("Identifier '#" + (element.key.name) + "' has already been declared"));
                }
              }
            }
            this.strict = oldStrict;
            this.next();
            node.body = this.finishNode(classBody, "ClassBody");
            this.exitClassBody();
            return this.finishNode(node, isStatement ? "ClassDeclaration" : "ClassExpression")
          };

          pp$8.parseClassElement = function(constructorAllowsSuper) {
            if (this.eat(types$1.semi)) { return null }

            var ecmaVersion = this.options.ecmaVersion;
            var node = this.startNode();
            var keyName = "";
            var isGenerator = false;
            var isAsync = false;
            var kind = "method";
            var isStatic = false;

            if (this.eatContextual("static")) {
              // Parse static init block
              if (ecmaVersion >= 13 && this.eat(types$1.braceL)) {
                this.parseClassStaticBlock(node);
                return node
              }
              if (this.isClassElementNameStart() || this.type === types$1.star) {
                isStatic = true;
              } else {
                keyName = "static";
              }
            }
            node.static = isStatic;
            if (!keyName && ecmaVersion >= 8 && this.eatContextual("async")) {
              if ((this.isClassElementNameStart() || this.type === types$1.star) && !this.canInsertSemicolon()) {
                isAsync = true;
              } else {
                keyName = "async";
              }
            }
            if (!keyName && (ecmaVersion >= 9 || !isAsync) && this.eat(types$1.star)) {
              isGenerator = true;
            }
            if (!keyName && !isAsync && !isGenerator) {
              var lastValue = this.value;
              if (this.eatContextual("get") || this.eatContextual("set")) {
                if (this.isClassElementNameStart()) {
                  kind = lastValue;
                } else {
                  keyName = lastValue;
                }
              }
            }

            // Parse element name
            if (keyName) {
              // 'async', 'get', 'set', or 'static' were not a keyword contextually.
              // The last token is any of those. Make it the element name.
              node.computed = false;
              node.key = this.startNodeAt(this.lastTokStart, this.lastTokStartLoc);
              node.key.name = keyName;
              this.finishNode(node.key, "Identifier");
            } else {
              this.parseClassElementName(node);
            }

            // Parse element value
            if (ecmaVersion < 13 || this.type === types$1.parenL || kind !== "method" || isGenerator || isAsync) {
              var isConstructor = !node.static && checkKeyName(node, "constructor");
              var allowsDirectSuper = isConstructor && constructorAllowsSuper;
              // Couldn't move this check into the 'parseClassMethod' method for backward compatibility.
              if (isConstructor && kind !== "method") { this.raise(node.key.start, "Constructor can't have get/set modifier"); }
              node.kind = isConstructor ? "constructor" : kind;
              this.parseClassMethod(node, isGenerator, isAsync, allowsDirectSuper);
            } else {
              this.parseClassField(node);
            }

            return node
          };

          pp$8.isClassElementNameStart = function() {
            return (
              this.type === types$1.name ||
              this.type === types$1.privateId ||
              this.type === types$1.num ||
              this.type === types$1.string ||
              this.type === types$1.bracketL ||
              this.type.keyword
            )
          };

          pp$8.parseClassElementName = function(element) {
            if (this.type === types$1.privateId) {
              if (this.value === "constructor") {
                this.raise(this.start, "Classes can't have an element named '#constructor'");
              }
              element.computed = false;
              element.key = this.parsePrivateIdent();
            } else {
              this.parsePropertyName(element);
            }
          };

          pp$8.parseClassMethod = function(method, isGenerator, isAsync, allowsDirectSuper) {
            // Check key and flags
            var key = method.key;
            if (method.kind === "constructor") {
              if (isGenerator) { this.raise(key.start, "Constructor can't be a generator"); }
              if (isAsync) { this.raise(key.start, "Constructor can't be an async method"); }
            } else if (method.static && checkKeyName(method, "prototype")) {
              this.raise(key.start, "Classes may not have a static property named prototype");
            }

            // Parse value
            var value = method.value = this.parseMethod(isGenerator, isAsync, allowsDirectSuper);

            // Check value
            if (method.kind === "get" && value.params.length !== 0)
              { this.raiseRecoverable(value.start, "getter should have no params"); }
            if (method.kind === "set" && value.params.length !== 1)
              { this.raiseRecoverable(value.start, "setter should have exactly one param"); }
            if (method.kind === "set" && value.params[0].type === "RestElement")
              { this.raiseRecoverable(value.params[0].start, "Setter cannot use rest params"); }

            return this.finishNode(method, "MethodDefinition")
          };

          pp$8.parseClassField = function(field) {
            if (checkKeyName(field, "constructor")) {
              this.raise(field.key.start, "Classes can't have a field named 'constructor'");
            } else if (field.static && checkKeyName(field, "prototype")) {
              this.raise(field.key.start, "Classes can't have a static field named 'prototype'");
            }

            if (this.eat(types$1.eq)) {
              // To raise SyntaxError if 'arguments' exists in the initializer.
              this.enterScope(SCOPE_CLASS_FIELD_INIT | SCOPE_SUPER);
              field.value = this.parseMaybeAssign();
              this.exitScope();
            } else {
              field.value = null;
            }
            this.semicolon();

            return this.finishNode(field, "PropertyDefinition")
          };

          pp$8.parseClassStaticBlock = function(node) {
            node.body = [];

            var oldLabels = this.labels;
            this.labels = [];
            this.enterScope(SCOPE_CLASS_STATIC_BLOCK | SCOPE_SUPER);
            while (this.type !== types$1.braceR) {
              var stmt = this.parseStatement(null);
              node.body.push(stmt);
            }
            this.next();
            this.exitScope();
            this.labels = oldLabels;

            return this.finishNode(node, "StaticBlock")
          };

          pp$8.parseClassId = function(node, isStatement) {
            if (this.type === types$1.name) {
              node.id = this.parseIdent();
              if (isStatement)
                { this.checkLValSimple(node.id, BIND_LEXICAL, false); }
            } else {
              if (isStatement === true)
                { this.unexpected(); }
              node.id = null;
            }
          };

          pp$8.parseClassSuper = function(node) {
            node.superClass = this.eat(types$1._extends) ? this.parseExprSubscripts(null, false) : null;
          };

          pp$8.enterClassBody = function() {
            var element = {declared: Object.create(null), used: []};
            this.privateNameStack.push(element);
            return element.declared
          };

          pp$8.exitClassBody = function() {
            var ref = this.privateNameStack.pop();
            var declared = ref.declared;
            var used = ref.used;
            if (!this.options.checkPrivateFields) { return }
            var len = this.privateNameStack.length;
            var parent = len === 0 ? null : this.privateNameStack[len - 1];
            for (var i = 0; i < used.length; ++i) {
              var id = used[i];
              if (!hasOwn(declared, id.name)) {
                if (parent) {
                  parent.used.push(id);
                } else {
                  this.raiseRecoverable(id.start, ("Private field '#" + (id.name) + "' must be declared in an enclosing class"));
                }
              }
            }
          };

          function isPrivateNameConflicted(privateNameMap, element) {
            var name = element.key.name;
            var curr = privateNameMap[name];

            var next = "true";
            if (element.type === "MethodDefinition" && (element.kind === "get" || element.kind === "set")) {
              next = (element.static ? "s" : "i") + element.kind;
            }

            // `class { get #a(){}; static set #a(_){} }` is also conflict.
            if (
              curr === "iget" && next === "iset" ||
              curr === "iset" && next === "iget" ||
              curr === "sget" && next === "sset" ||
              curr === "sset" && next === "sget"
            ) {
              privateNameMap[name] = "true";
              return false
            } else if (!curr) {
              privateNameMap[name] = next;
              return false
            } else {
              return true
            }
          }

          function checkKeyName(node, name) {
            var computed = node.computed;
            var key = node.key;
            return !computed && (
              key.type === "Identifier" && key.name === name ||
              key.type === "Literal" && key.value === name
            )
          }

          // Parses module export declaration.

          pp$8.parseExportAllDeclaration = function(node, exports) {
            if (this.options.ecmaVersion >= 11) {
              if (this.eatContextual("as")) {
                node.exported = this.parseModuleExportName();
                this.checkExport(exports, node.exported, this.lastTokStart);
              } else {
                node.exported = null;
              }
            }
            this.expectContextual("from");
            if (this.type !== types$1.string) { this.unexpected(); }
            node.source = this.parseExprAtom();
            if (this.options.ecmaVersion >= 16)
              { node.attributes = this.parseWithClause(); }
            this.semicolon();
            return this.finishNode(node, "ExportAllDeclaration")
          };

          pp$8.parseExport = function(node, exports) {
            this.next();
            // export * from '...'
            if (this.eat(types$1.star)) {
              return this.parseExportAllDeclaration(node, exports)
            }
            if (this.eat(types$1._default)) { // export default ...
              this.checkExport(exports, "default", this.lastTokStart);
              node.declaration = this.parseExportDefaultDeclaration();
              return this.finishNode(node, "ExportDefaultDeclaration")
            }
            // export var|const|let|function|class ...
            if (this.shouldParseExportStatement()) {
              node.declaration = this.parseExportDeclaration(node);
              if (node.declaration.type === "VariableDeclaration")
                { this.checkVariableExport(exports, node.declaration.declarations); }
              else
                { this.checkExport(exports, node.declaration.id, node.declaration.id.start); }
              node.specifiers = [];
              node.source = null;
              if (this.options.ecmaVersion >= 16)
                { node.attributes = []; }
            } else { // export { x, y as z } [from '...']
              node.declaration = null;
              node.specifiers = this.parseExportSpecifiers(exports);
              if (this.eatContextual("from")) {
                if (this.type !== types$1.string) { this.unexpected(); }
                node.source = this.parseExprAtom();
                if (this.options.ecmaVersion >= 16)
                  { node.attributes = this.parseWithClause(); }
              } else {
                for (var i = 0, list = node.specifiers; i < list.length; i += 1) {
                  // check for keywords used as local names
                  var spec = list[i];

                  this.checkUnreserved(spec.local);
                  // check if export is defined
                  this.checkLocalExport(spec.local);

                  if (spec.local.type === "Literal") {
                    this.raise(spec.local.start, "A string literal cannot be used as an exported binding without `from`.");
                  }
                }

                node.source = null;
                if (this.options.ecmaVersion >= 16)
                  { node.attributes = []; }
              }
              this.semicolon();
            }
            return this.finishNode(node, "ExportNamedDeclaration")
          };

          pp$8.parseExportDeclaration = function(node) {
            return this.parseStatement(null)
          };

          pp$8.parseExportDefaultDeclaration = function() {
            var isAsync;
            if (this.type === types$1._function || (isAsync = this.isAsyncFunction())) {
              var fNode = this.startNode();
              this.next();
              if (isAsync) { this.next(); }
              return this.parseFunction(fNode, FUNC_STATEMENT | FUNC_NULLABLE_ID, false, isAsync)
            } else if (this.type === types$1._class) {
              var cNode = this.startNode();
              return this.parseClass(cNode, "nullableID")
            } else {
              var declaration = this.parseMaybeAssign();
              this.semicolon();
              return declaration
            }
          };

          pp$8.checkExport = function(exports, name, pos) {
            if (!exports) { return }
            if (typeof name !== "string")
              { name = name.type === "Identifier" ? name.name : name.value; }
            if (hasOwn(exports, name))
              { this.raiseRecoverable(pos, "Duplicate export '" + name + "'"); }
            exports[name] = true;
          };

          pp$8.checkPatternExport = function(exports, pat) {
            var type = pat.type;
            if (type === "Identifier")
              { this.checkExport(exports, pat, pat.start); }
            else if (type === "ObjectPattern")
              { for (var i = 0, list = pat.properties; i < list.length; i += 1)
                {
                  var prop = list[i];

                  this.checkPatternExport(exports, prop);
                } }
            else if (type === "ArrayPattern")
              { for (var i$1 = 0, list$1 = pat.elements; i$1 < list$1.length; i$1 += 1) {
                var elt = list$1[i$1];

                  if (elt) { this.checkPatternExport(exports, elt); }
              } }
            else if (type === "Property")
              { this.checkPatternExport(exports, pat.value); }
            else if (type === "AssignmentPattern")
              { this.checkPatternExport(exports, pat.left); }
            else if (type === "RestElement")
              { this.checkPatternExport(exports, pat.argument); }
          };

          pp$8.checkVariableExport = function(exports, decls) {
            if (!exports) { return }
            for (var i = 0, list = decls; i < list.length; i += 1)
              {
              var decl = list[i];

              this.checkPatternExport(exports, decl.id);
            }
          };

          pp$8.shouldParseExportStatement = function() {
            return this.type.keyword === "var" ||
              this.type.keyword === "const" ||
              this.type.keyword === "class" ||
              this.type.keyword === "function" ||
              this.isLet() ||
              this.isAsyncFunction()
          };

          // Parses a comma-separated list of module exports.

          pp$8.parseExportSpecifier = function(exports) {
            var node = this.startNode();
            node.local = this.parseModuleExportName();

            node.exported = this.eatContextual("as") ? this.parseModuleExportName() : node.local;
            this.checkExport(
              exports,
              node.exported,
              node.exported.start
            );

            return this.finishNode(node, "ExportSpecifier")
          };

          pp$8.parseExportSpecifiers = function(exports) {
            var nodes = [], first = true;
            // export { x, y as z } [from '...']
            this.expect(types$1.braceL);
            while (!this.eat(types$1.braceR)) {
              if (!first) {
                this.expect(types$1.comma);
                if (this.afterTrailingComma(types$1.braceR)) { break }
              } else { first = false; }

              nodes.push(this.parseExportSpecifier(exports));
            }
            return nodes
          };

          // Parses import declaration.

          pp$8.parseImport = function(node) {
            this.next();

            // import '...'
            if (this.type === types$1.string) {
              node.specifiers = empty$1;
              node.source = this.parseExprAtom();
            } else {
              node.specifiers = this.parseImportSpecifiers();
              this.expectContextual("from");
              node.source = this.type === types$1.string ? this.parseExprAtom() : this.unexpected();
            }
            if (this.options.ecmaVersion >= 16)
              { node.attributes = this.parseWithClause(); }
            this.semicolon();
            return this.finishNode(node, "ImportDeclaration")
          };

          // Parses a comma-separated list of module imports.

          pp$8.parseImportSpecifier = function() {
            var node = this.startNode();
            node.imported = this.parseModuleExportName();

            if (this.eatContextual("as")) {
              node.local = this.parseIdent();
            } else {
              this.checkUnreserved(node.imported);
              node.local = node.imported;
            }
            this.checkLValSimple(node.local, BIND_LEXICAL);

            return this.finishNode(node, "ImportSpecifier")
          };

          pp$8.parseImportDefaultSpecifier = function() {
            // import defaultObj, { x, y as z } from '...'
            var node = this.startNode();
            node.local = this.parseIdent();
            this.checkLValSimple(node.local, BIND_LEXICAL);
            return this.finishNode(node, "ImportDefaultSpecifier")
          };

          pp$8.parseImportNamespaceSpecifier = function() {
            var node = this.startNode();
            this.next();
            this.expectContextual("as");
            node.local = this.parseIdent();
            this.checkLValSimple(node.local, BIND_LEXICAL);
            return this.finishNode(node, "ImportNamespaceSpecifier")
          };

          pp$8.parseImportSpecifiers = function() {
            var nodes = [], first = true;
            if (this.type === types$1.name) {
              nodes.push(this.parseImportDefaultSpecifier());
              if (!this.eat(types$1.comma)) { return nodes }
            }
            if (this.type === types$1.star) {
              nodes.push(this.parseImportNamespaceSpecifier());
              return nodes
            }
            this.expect(types$1.braceL);
            while (!this.eat(types$1.braceR)) {
              if (!first) {
                this.expect(types$1.comma);
                if (this.afterTrailingComma(types$1.braceR)) { break }
              } else { first = false; }

              nodes.push(this.parseImportSpecifier());
            }
            return nodes
          };

          pp$8.parseWithClause = function() {
            var nodes = [];
            if (!this.eat(types$1._with)) {
              return nodes
            }
            this.expect(types$1.braceL);
            var attributeKeys = {};
            var first = true;
            while (!this.eat(types$1.braceR)) {
              if (!first) {
                this.expect(types$1.comma);
                if (this.afterTrailingComma(types$1.braceR)) { break }
              } else { first = false; }

              var attr = this.parseImportAttribute();
              var keyName = attr.key.type === "Identifier" ? attr.key.name : attr.key.value;
              if (hasOwn(attributeKeys, keyName))
                { this.raiseRecoverable(attr.key.start, "Duplicate attribute key '" + keyName + "'"); }
              attributeKeys[keyName] = true;
              nodes.push(attr);
            }
            return nodes
          };

          pp$8.parseImportAttribute = function() {
            var node = this.startNode();
            node.key = this.type === types$1.string ? this.parseExprAtom() : this.parseIdent(this.options.allowReserved !== "never");
            this.expect(types$1.colon);
            if (this.type !== types$1.string) {
              this.unexpected();
            }
            node.value = this.parseExprAtom();
            return this.finishNode(node, "ImportAttribute")
          };

          pp$8.parseModuleExportName = function() {
            if (this.options.ecmaVersion >= 13 && this.type === types$1.string) {
              var stringLiteral = this.parseLiteral(this.value);
              if (loneSurrogate.test(stringLiteral.value)) {
                this.raise(stringLiteral.start, "An export name cannot include a lone surrogate.");
              }
              return stringLiteral
            }
            return this.parseIdent(true)
          };

          // Set `ExpressionStatement#directive` property for directive prologues.
          pp$8.adaptDirectivePrologue = function(statements) {
            for (var i = 0; i < statements.length && this.isDirectiveCandidate(statements[i]); ++i) {
              statements[i].directive = statements[i].expression.raw.slice(1, -1);
            }
          };
          pp$8.isDirectiveCandidate = function(statement) {
            return (
              this.options.ecmaVersion >= 5 &&
              statement.type === "ExpressionStatement" &&
              statement.expression.type === "Literal" &&
              typeof statement.expression.value === "string" &&
              // Reject parenthesized strings.
              (this.input[statement.start] === "\"" || this.input[statement.start] === "'")
            )
          };

          var pp$7 = Parser.prototype;

          // Convert existing expression atom to assignable pattern
          // if possible.

          pp$7.toAssignable = function(node, isBinding, refDestructuringErrors) {
            if (this.options.ecmaVersion >= 6 && node) {
              switch (node.type) {
              case "Identifier":
                if (this.inAsync && node.name === "await")
                  { this.raise(node.start, "Cannot use 'await' as identifier inside an async function"); }
                break

              case "ObjectPattern":
              case "ArrayPattern":
              case "AssignmentPattern":
              case "RestElement":
                break

              case "ObjectExpression":
                node.type = "ObjectPattern";
                if (refDestructuringErrors) { this.checkPatternErrors(refDestructuringErrors, true); }
                for (var i = 0, list = node.properties; i < list.length; i += 1) {
                  var prop = list[i];

                this.toAssignable(prop, isBinding);
                  // Early error:
                  //   AssignmentRestProperty[Yield, Await] :
                  //     `...` DestructuringAssignmentTarget[Yield, Await]
                  //
                  //   It is a Syntax Error if |DestructuringAssignmentTarget| is an |ArrayLiteral| or an |ObjectLiteral|.
                  if (
                    prop.type === "RestElement" &&
                    (prop.argument.type === "ArrayPattern" || prop.argument.type === "ObjectPattern")
                  ) {
                    this.raise(prop.argument.start, "Unexpected token");
                  }
                }
                break

              case "Property":
                // AssignmentProperty has type === "Property"
                if (node.kind !== "init") { this.raise(node.key.start, "Object pattern can't contain getter or setter"); }
                this.toAssignable(node.value, isBinding);
                break

              case "ArrayExpression":
                node.type = "ArrayPattern";
                if (refDestructuringErrors) { this.checkPatternErrors(refDestructuringErrors, true); }
                this.toAssignableList(node.elements, isBinding);
                break

              case "SpreadElement":
                node.type = "RestElement";
                this.toAssignable(node.argument, isBinding);
                if (node.argument.type === "AssignmentPattern")
                  { this.raise(node.argument.start, "Rest elements cannot have a default value"); }
                break

              case "AssignmentExpression":
                if (node.operator !== "=") { this.raise(node.left.end, "Only '=' operator can be used for specifying default value."); }
                node.type = "AssignmentPattern";
                delete node.operator;
                this.toAssignable(node.left, isBinding);
                break

              case "ParenthesizedExpression":
                this.toAssignable(node.expression, isBinding, refDestructuringErrors);
                break

              case "ChainExpression":
                this.raiseRecoverable(node.start, "Optional chaining cannot appear in left-hand side");
                break

              case "MemberExpression":
                if (!isBinding) { break }

              default:
                this.raise(node.start, "Assigning to rvalue");
              }
            } else if (refDestructuringErrors) { this.checkPatternErrors(refDestructuringErrors, true); }
            return node
          };

          // Convert list of expression atoms to binding list.

          pp$7.toAssignableList = function(exprList, isBinding) {
            var end = exprList.length;
            for (var i = 0; i < end; i++) {
              var elt = exprList[i];
              if (elt) { this.toAssignable(elt, isBinding); }
            }
            if (end) {
              var last = exprList[end - 1];
              if (this.options.ecmaVersion === 6 && isBinding && last && last.type === "RestElement" && last.argument.type !== "Identifier")
                { this.unexpected(last.argument.start); }
            }
            return exprList
          };

          // Parses spread element.

          pp$7.parseSpread = function(refDestructuringErrors) {
            var node = this.startNode();
            this.next();
            node.argument = this.parseMaybeAssign(false, refDestructuringErrors);
            return this.finishNode(node, "SpreadElement")
          };

          pp$7.parseRestBinding = function() {
            var node = this.startNode();
            this.next();

            // RestElement inside of a function parameter must be an identifier
            if (this.options.ecmaVersion === 6 && this.type !== types$1.name)
              { this.unexpected(); }

            node.argument = this.parseBindingAtom();

            return this.finishNode(node, "RestElement")
          };

          // Parses lvalue (assignable) atom.

          pp$7.parseBindingAtom = function() {
            if (this.options.ecmaVersion >= 6) {
              switch (this.type) {
              case types$1.bracketL:
                var node = this.startNode();
                this.next();
                node.elements = this.parseBindingList(types$1.bracketR, true, true);
                return this.finishNode(node, "ArrayPattern")

              case types$1.braceL:
                return this.parseObj(true)
              }
            }
            return this.parseIdent()
          };

          pp$7.parseBindingList = function(close, allowEmpty, allowTrailingComma, allowModifiers) {
            var elts = [], first = true;
            while (!this.eat(close)) {
              if (first) { first = false; }
              else { this.expect(types$1.comma); }
              if (allowEmpty && this.type === types$1.comma) {
                elts.push(null);
              } else if (allowTrailingComma && this.afterTrailingComma(close)) {
                break
              } else if (this.type === types$1.ellipsis) {
                var rest = this.parseRestBinding();
                this.parseBindingListItem(rest);
                elts.push(rest);
                if (this.type === types$1.comma) { this.raiseRecoverable(this.start, "Comma is not permitted after the rest element"); }
                this.expect(close);
                break
              } else {
                elts.push(this.parseAssignableListItem(allowModifiers));
              }
            }
            return elts
          };

          pp$7.parseAssignableListItem = function(allowModifiers) {
            var elem = this.parseMaybeDefault(this.start, this.startLoc);
            this.parseBindingListItem(elem);
            return elem
          };

          pp$7.parseBindingListItem = function(param) {
            return param
          };

          // Parses assignment pattern around given atom if possible.

          pp$7.parseMaybeDefault = function(startPos, startLoc, left) {
            left = left || this.parseBindingAtom();
            if (this.options.ecmaVersion < 6 || !this.eat(types$1.eq)) { return left }
            var node = this.startNodeAt(startPos, startLoc);
            node.left = left;
            node.right = this.parseMaybeAssign();
            return this.finishNode(node, "AssignmentPattern")
          };

          // The following three functions all verify that a node is an lvalue —
          // something that can be bound, or assigned to. In order to do so, they perform
          // a variety of checks:
          //
          // - Check that none of the bound/assigned-to identifiers are reserved words.
          // - Record name declarations for bindings in the appropriate scope.
          // - Check duplicate argument names, if checkClashes is set.
          //
          // If a complex binding pattern is encountered (e.g., object and array
          // destructuring), the entire pattern is recursively checked.
          //
          // There are three versions of checkLVal*() appropriate for different
          // circumstances:
          //
          // - checkLValSimple() shall be used if the syntactic construct supports
          //   nothing other than identifiers and member expressions. Parenthesized
          //   expressions are also correctly handled. This is generally appropriate for
          //   constructs for which the spec says
          //
          //   > It is a Syntax Error if AssignmentTargetType of [the production] is not
          //   > simple.
          //
          //   It is also appropriate for checking if an identifier is valid and not
          //   defined elsewhere, like import declarations or function/class identifiers.
          //
          //   Examples where this is used include:
          //     a += …;
          //     import a from '…';
          //   where a is the node to be checked.
          //
          // - checkLValPattern() shall be used if the syntactic construct supports
          //   anything checkLValSimple() supports, as well as object and array
          //   destructuring patterns. This is generally appropriate for constructs for
          //   which the spec says
          //
          //   > It is a Syntax Error if [the production] is neither an ObjectLiteral nor
          //   > an ArrayLiteral and AssignmentTargetType of [the production] is not
          //   > simple.
          //
          //   Examples where this is used include:
          //     (a = …);
          //     const a = …;
          //     try { … } catch (a) { … }
          //   where a is the node to be checked.
          //
          // - checkLValInnerPattern() shall be used if the syntactic construct supports
          //   anything checkLValPattern() supports, as well as default assignment
          //   patterns, rest elements, and other constructs that may appear within an
          //   object or array destructuring pattern.
          //
          //   As a special case, function parameters also use checkLValInnerPattern(),
          //   as they also support defaults and rest constructs.
          //
          // These functions deliberately support both assignment and binding constructs,
          // as the logic for both is exceedingly similar. If the node is the target of
          // an assignment, then bindingType should be set to BIND_NONE. Otherwise, it
          // should be set to the appropriate BIND_* constant, like BIND_VAR or
          // BIND_LEXICAL.
          //
          // If the function is called with a non-BIND_NONE bindingType, then
          // additionally a checkClashes object may be specified to allow checking for
          // duplicate argument names. checkClashes is ignored if the provided construct
          // is an assignment (i.e., bindingType is BIND_NONE).

          pp$7.checkLValSimple = function(expr, bindingType, checkClashes) {
            if ( bindingType === void 0 ) bindingType = BIND_NONE;

            var isBind = bindingType !== BIND_NONE;

            switch (expr.type) {
            case "Identifier":
              if (this.strict && this.reservedWordsStrictBind.test(expr.name))
                { this.raiseRecoverable(expr.start, (isBind ? "Binding " : "Assigning to ") + expr.name + " in strict mode"); }
              if (isBind) {
                if (bindingType === BIND_LEXICAL && expr.name === "let")
                  { this.raiseRecoverable(expr.start, "let is disallowed as a lexically bound name"); }
                if (checkClashes) {
                  if (hasOwn(checkClashes, expr.name))
                    { this.raiseRecoverable(expr.start, "Argument name clash"); }
                  checkClashes[expr.name] = true;
                }
                if (bindingType !== BIND_OUTSIDE) { this.declareName(expr.name, bindingType, expr.start); }
              }
              break

            case "ChainExpression":
              this.raiseRecoverable(expr.start, "Optional chaining cannot appear in left-hand side");
              break

            case "MemberExpression":
              if (isBind) { this.raiseRecoverable(expr.start, "Binding member expression"); }
              break

            case "ParenthesizedExpression":
              if (isBind) { this.raiseRecoverable(expr.start, "Binding parenthesized expression"); }
              return this.checkLValSimple(expr.expression, bindingType, checkClashes)

            default:
              this.raise(expr.start, (isBind ? "Binding" : "Assigning to") + " rvalue");
            }
          };

          pp$7.checkLValPattern = function(expr, bindingType, checkClashes) {
            if ( bindingType === void 0 ) bindingType = BIND_NONE;

            switch (expr.type) {
            case "ObjectPattern":
              for (var i = 0, list = expr.properties; i < list.length; i += 1) {
                var prop = list[i];

              this.checkLValInnerPattern(prop, bindingType, checkClashes);
              }
              break

            case "ArrayPattern":
              for (var i$1 = 0, list$1 = expr.elements; i$1 < list$1.length; i$1 += 1) {
                var elem = list$1[i$1];

              if (elem) { this.checkLValInnerPattern(elem, bindingType, checkClashes); }
              }
              break

            default:
              this.checkLValSimple(expr, bindingType, checkClashes);
            }
          };

          pp$7.checkLValInnerPattern = function(expr, bindingType, checkClashes) {
            if ( bindingType === void 0 ) bindingType = BIND_NONE;

            switch (expr.type) {
            case "Property":
              // AssignmentProperty has type === "Property"
              this.checkLValInnerPattern(expr.value, bindingType, checkClashes);
              break

            case "AssignmentPattern":
              this.checkLValPattern(expr.left, bindingType, checkClashes);
              break

            case "RestElement":
              this.checkLValPattern(expr.argument, bindingType, checkClashes);
              break

            default:
              this.checkLValPattern(expr, bindingType, checkClashes);
            }
          };

          // The algorithm used to determine whether a regexp can appear at a
          // given point in the program is loosely based on sweet.js' approach.
          // See https://github.com/mozilla/sweet.js/wiki/design


          var TokContext = function TokContext(token, isExpr, preserveSpace, override, generator) {
            this.token = token;
            this.isExpr = !!isExpr;
            this.preserveSpace = !!preserveSpace;
            this.override = override;
            this.generator = !!generator;
          };

          var types = {
            b_stat: new TokContext("{", false),
            b_expr: new TokContext("{", true),
            b_tmpl: new TokContext("${", false),
            p_stat: new TokContext("(", false),
            p_expr: new TokContext("(", true),
            q_tmpl: new TokContext("`", true, true, function (p) { return p.tryReadTemplateToken(); }),
            f_stat: new TokContext("function", false),
            f_expr: new TokContext("function", true),
            f_expr_gen: new TokContext("function", true, false, null, true),
            f_gen: new TokContext("function", false, false, null, true)
          };

          var pp$6 = Parser.prototype;

          pp$6.initialContext = function() {
            return [types.b_stat]
          };

          pp$6.curContext = function() {
            return this.context[this.context.length - 1]
          };

          pp$6.braceIsBlock = function(prevType) {
            var parent = this.curContext();
            if (parent === types.f_expr || parent === types.f_stat)
              { return true }
            if (prevType === types$1.colon && (parent === types.b_stat || parent === types.b_expr))
              { return !parent.isExpr }

            // The check for `tt.name && exprAllowed` detects whether we are
            // after a `yield` or `of` construct. See the `updateContext` for
            // `tt.name`.
            if (prevType === types$1._return || prevType === types$1.name && this.exprAllowed)
              { return lineBreak.test(this.input.slice(this.lastTokEnd, this.start)) }
            if (prevType === types$1._else || prevType === types$1.semi || prevType === types$1.eof || prevType === types$1.parenR || prevType === types$1.arrow)
              { return true }
            if (prevType === types$1.braceL)
              { return parent === types.b_stat }
            if (prevType === types$1._var || prevType === types$1._const || prevType === types$1.name)
              { return false }
            return !this.exprAllowed
          };

          pp$6.inGeneratorContext = function() {
            for (var i = this.context.length - 1; i >= 1; i--) {
              var context = this.context[i];
              if (context.token === "function")
                { return context.generator }
            }
            return false
          };

          pp$6.updateContext = function(prevType) {
            var update, type = this.type;
            if (type.keyword && prevType === types$1.dot)
              { this.exprAllowed = false; }
            else if (update = type.updateContext)
              { update.call(this, prevType); }
            else
              { this.exprAllowed = type.beforeExpr; }
          };

          // Used to handle edge cases when token context could not be inferred correctly during tokenization phase

          pp$6.overrideContext = function(tokenCtx) {
            if (this.curContext() !== tokenCtx) {
              this.context[this.context.length - 1] = tokenCtx;
            }
          };

          // Token-specific context update code

          types$1.parenR.updateContext = types$1.braceR.updateContext = function() {
            if (this.context.length === 1) {
              this.exprAllowed = true;
              return
            }
            var out = this.context.pop();
            if (out === types.b_stat && this.curContext().token === "function") {
              out = this.context.pop();
            }
            this.exprAllowed = !out.isExpr;
          };

          types$1.braceL.updateContext = function(prevType) {
            this.context.push(this.braceIsBlock(prevType) ? types.b_stat : types.b_expr);
            this.exprAllowed = true;
          };

          types$1.dollarBraceL.updateContext = function() {
            this.context.push(types.b_tmpl);
            this.exprAllowed = true;
          };

          types$1.parenL.updateContext = function(prevType) {
            var statementParens = prevType === types$1._if || prevType === types$1._for || prevType === types$1._with || prevType === types$1._while;
            this.context.push(statementParens ? types.p_stat : types.p_expr);
            this.exprAllowed = true;
          };

          types$1.incDec.updateContext = function() {
            // tokExprAllowed stays unchanged
          };

          types$1._function.updateContext = types$1._class.updateContext = function(prevType) {
            if (prevType.beforeExpr && prevType !== types$1._else &&
                !(prevType === types$1.semi && this.curContext() !== types.p_stat) &&
                !(prevType === types$1._return && lineBreak.test(this.input.slice(this.lastTokEnd, this.start))) &&
                !((prevType === types$1.colon || prevType === types$1.braceL) && this.curContext() === types.b_stat))
              { this.context.push(types.f_expr); }
            else
              { this.context.push(types.f_stat); }
            this.exprAllowed = false;
          };

          types$1.colon.updateContext = function() {
            if (this.curContext().token === "function") { this.context.pop(); }
            this.exprAllowed = true;
          };

          types$1.backQuote.updateContext = function() {
            if (this.curContext() === types.q_tmpl)
              { this.context.pop(); }
            else
              { this.context.push(types.q_tmpl); }
            this.exprAllowed = false;
          };

          types$1.star.updateContext = function(prevType) {
            if (prevType === types$1._function) {
              var index = this.context.length - 1;
              if (this.context[index] === types.f_expr)
                { this.context[index] = types.f_expr_gen; }
              else
                { this.context[index] = types.f_gen; }
            }
            this.exprAllowed = true;
          };

          types$1.name.updateContext = function(prevType) {
            var allowed = false;
            if (this.options.ecmaVersion >= 6 && prevType !== types$1.dot) {
              if (this.value === "of" && !this.exprAllowed ||
                  this.value === "yield" && this.inGeneratorContext())
                { allowed = true; }
            }
            this.exprAllowed = allowed;
          };

          // A recursive descent parser operates by defining functions for all
          // syntactic elements, and recursively calling those, each function
          // advancing the input stream and returning an AST node. Precedence
          // of constructs (for example, the fact that `!x[1]` means `!(x[1])`
          // instead of `(!x)[1]` is handled by the fact that the parser
          // function that parses unary prefix operators is called first, and
          // in turn calls the function that parses `[]` subscripts — that
          // way, it'll receive the node for `x[1]` already parsed, and wraps
          // *that* in the unary operator node.
          //
          // Acorn uses an [operator precedence parser][opp] to handle binary
          // operator precedence, because it is much more compact than using
          // the technique outlined above, which uses different, nesting
          // functions to specify precedence, for all of the ten binary
          // precedence levels that JavaScript defines.
          //
          // [opp]: http://en.wikipedia.org/wiki/Operator-precedence_parser


          var pp$5 = Parser.prototype;

          // Check if property name clashes with already added.
          // Object/class getters and setters are not allowed to clash —
          // either with each other or with an init property — and in
          // strict mode, init properties are also not allowed to be repeated.

          pp$5.checkPropClash = function(prop, propHash, refDestructuringErrors) {
            if (this.options.ecmaVersion >= 9 && prop.type === "SpreadElement")
              { return }
            if (this.options.ecmaVersion >= 6 && (prop.computed || prop.method || prop.shorthand))
              { return }
            var key = prop.key;
            var name;
            switch (key.type) {
            case "Identifier": name = key.name; break
            case "Literal": name = String(key.value); break
            default: return
            }
            var kind = prop.kind;
            if (this.options.ecmaVersion >= 6) {
              if (name === "__proto__" && kind === "init") {
                if (propHash.proto) {
                  if (refDestructuringErrors) {
                    if (refDestructuringErrors.doubleProto < 0) {
                      refDestructuringErrors.doubleProto = key.start;
                    }
                  } else {
                    this.raiseRecoverable(key.start, "Redefinition of __proto__ property");
                  }
                }
                propHash.proto = true;
              }
              return
            }
            name = "$" + name;
            var other = propHash[name];
            if (other) {
              var redefinition;
              if (kind === "init") {
                redefinition = this.strict && other.init || other.get || other.set;
              } else {
                redefinition = other.init || other[kind];
              }
              if (redefinition)
                { this.raiseRecoverable(key.start, "Redefinition of property"); }
            } else {
              other = propHash[name] = {
                init: false,
                get: false,
                set: false
              };
            }
            other[kind] = true;
          };

          // ### Expression parsing

          // These nest, from the most general expression type at the top to
          // 'atomic', nondivisible expression types at the bottom. Most of
          // the functions will simply let the function(s) below them parse,
          // and, *if* the syntactic construct they handle is present, wrap
          // the AST node that the inner parser gave them in another node.

          // Parse a full expression. The optional arguments are used to
          // forbid the `in` operator (in for loops initalization expressions)
          // and provide reference for storing '=' operator inside shorthand
          // property assignment in contexts where both object expression
          // and object pattern might appear (so it's possible to raise
          // delayed syntax error at correct position).

          pp$5.parseExpression = function(forInit, refDestructuringErrors) {
            var startPos = this.start, startLoc = this.startLoc;
            var expr = this.parseMaybeAssign(forInit, refDestructuringErrors);
            if (this.type === types$1.comma) {
              var node = this.startNodeAt(startPos, startLoc);
              node.expressions = [expr];
              while (this.eat(types$1.comma)) { node.expressions.push(this.parseMaybeAssign(forInit, refDestructuringErrors)); }
              return this.finishNode(node, "SequenceExpression")
            }
            return expr
          };

          // Parse an assignment expression. This includes applications of
          // operators like `+=`.

          pp$5.parseMaybeAssign = function(forInit, refDestructuringErrors, afterLeftParse) {
            if (this.isContextual("yield")) {
              if (this.inGenerator) { return this.parseYield(forInit) }
              // The tokenizer will assume an expression is allowed after
              // `yield`, but this isn't that kind of yield
              else { this.exprAllowed = false; }
            }

            var ownDestructuringErrors = false, oldParenAssign = -1, oldTrailingComma = -1, oldDoubleProto = -1;
            if (refDestructuringErrors) {
              oldParenAssign = refDestructuringErrors.parenthesizedAssign;
              oldTrailingComma = refDestructuringErrors.trailingComma;
              oldDoubleProto = refDestructuringErrors.doubleProto;
              refDestructuringErrors.parenthesizedAssign = refDestructuringErrors.trailingComma = -1;
            } else {
              refDestructuringErrors = new DestructuringErrors;
              ownDestructuringErrors = true;
            }

            var startPos = this.start, startLoc = this.startLoc;
            if (this.type === types$1.parenL || this.type === types$1.name) {
              this.potentialArrowAt = this.start;
              this.potentialArrowInForAwait = forInit === "await";
            }
            var left = this.parseMaybeConditional(forInit, refDestructuringErrors);
            if (afterLeftParse) { left = afterLeftParse.call(this, left, startPos, startLoc); }
            if (this.type.isAssign) {
              var node = this.startNodeAt(startPos, startLoc);
              node.operator = this.value;
              if (this.type === types$1.eq)
                { left = this.toAssignable(left, false, refDestructuringErrors); }
              if (!ownDestructuringErrors) {
                refDestructuringErrors.parenthesizedAssign = refDestructuringErrors.trailingComma = refDestructuringErrors.doubleProto = -1;
              }
              if (refDestructuringErrors.shorthandAssign >= left.start)
                { refDestructuringErrors.shorthandAssign = -1; } // reset because shorthand default was used correctly
              if (this.type === types$1.eq)
                { this.checkLValPattern(left); }
              else
                { this.checkLValSimple(left); }
              node.left = left;
              this.next();
              node.right = this.parseMaybeAssign(forInit);
              if (oldDoubleProto > -1) { refDestructuringErrors.doubleProto = oldDoubleProto; }
              return this.finishNode(node, "AssignmentExpression")
            } else {
              if (ownDestructuringErrors) { this.checkExpressionErrors(refDestructuringErrors, true); }
            }
            if (oldParenAssign > -1) { refDestructuringErrors.parenthesizedAssign = oldParenAssign; }
            if (oldTrailingComma > -1) { refDestructuringErrors.trailingComma = oldTrailingComma; }
            return left
          };

          // Parse a ternary conditional (`?:`) operator.

          pp$5.parseMaybeConditional = function(forInit, refDestructuringErrors) {
            var startPos = this.start, startLoc = this.startLoc;
            var expr = this.parseExprOps(forInit, refDestructuringErrors);
            if (this.checkExpressionErrors(refDestructuringErrors)) { return expr }
            if (this.eat(types$1.question)) {
              var node = this.startNodeAt(startPos, startLoc);
              node.test = expr;
              node.consequent = this.parseMaybeAssign();
              this.expect(types$1.colon);
              node.alternate = this.parseMaybeAssign(forInit);
              return this.finishNode(node, "ConditionalExpression")
            }
            return expr
          };

          // Start the precedence parser.

          pp$5.parseExprOps = function(forInit, refDestructuringErrors) {
            var startPos = this.start, startLoc = this.startLoc;
            var expr = this.parseMaybeUnary(refDestructuringErrors, false, false, forInit);
            if (this.checkExpressionErrors(refDestructuringErrors)) { return expr }
            return expr.start === startPos && expr.type === "ArrowFunctionExpression" ? expr : this.parseExprOp(expr, startPos, startLoc, -1, forInit)
          };

          // Parse binary operators with the operator precedence parsing
          // algorithm. `left` is the left-hand side of the operator.
          // `minPrec` provides context that allows the function to stop and
          // defer further parser to one of its callers when it encounters an
          // operator that has a lower precedence than the set it is parsing.

          pp$5.parseExprOp = function(left, leftStartPos, leftStartLoc, minPrec, forInit) {
            var prec = this.type.binop;
            if (prec != null && (!forInit || this.type !== types$1._in)) {
              if (prec > minPrec) {
                var logical = this.type === types$1.logicalOR || this.type === types$1.logicalAND;
                var coalesce = this.type === types$1.coalesce;
                if (coalesce) {
                  // Handle the precedence of `tt.coalesce` as equal to the range of logical expressions.
                  // In other words, `node.right` shouldn't contain logical expressions in order to check the mixed error.
                  prec = types$1.logicalAND.binop;
                }
                var op = this.value;
                this.next();
                var startPos = this.start, startLoc = this.startLoc;
                var right = this.parseExprOp(this.parseMaybeUnary(null, false, false, forInit), startPos, startLoc, prec, forInit);
                var node = this.buildBinary(leftStartPos, leftStartLoc, left, right, op, logical || coalesce);
                if ((logical && this.type === types$1.coalesce) || (coalesce && (this.type === types$1.logicalOR || this.type === types$1.logicalAND))) {
                  this.raiseRecoverable(this.start, "Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses");
                }
                return this.parseExprOp(node, leftStartPos, leftStartLoc, minPrec, forInit)
              }
            }
            return left
          };

          pp$5.buildBinary = function(startPos, startLoc, left, right, op, logical) {
            if (right.type === "PrivateIdentifier") { this.raise(right.start, "Private identifier can only be left side of binary expression"); }
            var node = this.startNodeAt(startPos, startLoc);
            node.left = left;
            node.operator = op;
            node.right = right;
            return this.finishNode(node, logical ? "LogicalExpression" : "BinaryExpression")
          };

          // Parse unary operators, both prefix and postfix.

          pp$5.parseMaybeUnary = function(refDestructuringErrors, sawUnary, incDec, forInit) {
            var startPos = this.start, startLoc = this.startLoc, expr;
            if (this.isContextual("await") && this.canAwait) {
              expr = this.parseAwait(forInit);
              sawUnary = true;
            } else if (this.type.prefix) {
              var node = this.startNode(), update = this.type === types$1.incDec;
              node.operator = this.value;
              node.prefix = true;
              this.next();
              node.argument = this.parseMaybeUnary(null, true, update, forInit);
              this.checkExpressionErrors(refDestructuringErrors, true);
              if (update) { this.checkLValSimple(node.argument); }
              else if (this.strict && node.operator === "delete" && isLocalVariableAccess(node.argument))
                { this.raiseRecoverable(node.start, "Deleting local variable in strict mode"); }
              else if (node.operator === "delete" && isPrivateFieldAccess(node.argument))
                { this.raiseRecoverable(node.start, "Private fields can not be deleted"); }
              else { sawUnary = true; }
              expr = this.finishNode(node, update ? "UpdateExpression" : "UnaryExpression");
            } else if (!sawUnary && this.type === types$1.privateId) {
              if ((forInit || this.privateNameStack.length === 0) && this.options.checkPrivateFields) { this.unexpected(); }
              expr = this.parsePrivateIdent();
              // only could be private fields in 'in', such as #x in obj
              if (this.type !== types$1._in) { this.unexpected(); }
            } else {
              expr = this.parseExprSubscripts(refDestructuringErrors, forInit);
              if (this.checkExpressionErrors(refDestructuringErrors)) { return expr }
              while (this.type.postfix && !this.canInsertSemicolon()) {
                var node$1 = this.startNodeAt(startPos, startLoc);
                node$1.operator = this.value;
                node$1.prefix = false;
                node$1.argument = expr;
                this.checkLValSimple(expr);
                this.next();
                expr = this.finishNode(node$1, "UpdateExpression");
              }
            }

            if (!incDec && this.eat(types$1.starstar)) {
              if (sawUnary)
                { this.unexpected(this.lastTokStart); }
              else
                { return this.buildBinary(startPos, startLoc, expr, this.parseMaybeUnary(null, false, false, forInit), "**", false) }
            } else {
              return expr
            }
          };

          function isLocalVariableAccess(node) {
            return (
              node.type === "Identifier" ||
              node.type === "ParenthesizedExpression" && isLocalVariableAccess(node.expression)
            )
          }

          function isPrivateFieldAccess(node) {
            return (
              node.type === "MemberExpression" && node.property.type === "PrivateIdentifier" ||
              node.type === "ChainExpression" && isPrivateFieldAccess(node.expression) ||
              node.type === "ParenthesizedExpression" && isPrivateFieldAccess(node.expression)
            )
          }

          // Parse call, dot, and `[]`-subscript expressions.

          pp$5.parseExprSubscripts = function(refDestructuringErrors, forInit) {
            var startPos = this.start, startLoc = this.startLoc;
            var expr = this.parseExprAtom(refDestructuringErrors, forInit);
            if (expr.type === "ArrowFunctionExpression" && this.input.slice(this.lastTokStart, this.lastTokEnd) !== ")")
              { return expr }
            var result = this.parseSubscripts(expr, startPos, startLoc, false, forInit);
            if (refDestructuringErrors && result.type === "MemberExpression") {
              if (refDestructuringErrors.parenthesizedAssign >= result.start) { refDestructuringErrors.parenthesizedAssign = -1; }
              if (refDestructuringErrors.parenthesizedBind >= result.start) { refDestructuringErrors.parenthesizedBind = -1; }
              if (refDestructuringErrors.trailingComma >= result.start) { refDestructuringErrors.trailingComma = -1; }
            }
            return result
          };

          pp$5.parseSubscripts = function(base, startPos, startLoc, noCalls, forInit) {
            var maybeAsyncArrow = this.options.ecmaVersion >= 8 && base.type === "Identifier" && base.name === "async" &&
                this.lastTokEnd === base.end && !this.canInsertSemicolon() && base.end - base.start === 5 &&
                this.potentialArrowAt === base.start;
            var optionalChained = false;

            while (true) {
              var element = this.parseSubscript(base, startPos, startLoc, noCalls, maybeAsyncArrow, optionalChained, forInit);

              if (element.optional) { optionalChained = true; }
              if (element === base || element.type === "ArrowFunctionExpression") {
                if (optionalChained) {
                  var chainNode = this.startNodeAt(startPos, startLoc);
                  chainNode.expression = element;
                  element = this.finishNode(chainNode, "ChainExpression");
                }
                return element
              }

              base = element;
            }
          };

          pp$5.shouldParseAsyncArrow = function() {
            return !this.canInsertSemicolon() && this.eat(types$1.arrow)
          };

          pp$5.parseSubscriptAsyncArrow = function(startPos, startLoc, exprList, forInit) {
            return this.parseArrowExpression(this.startNodeAt(startPos, startLoc), exprList, true, forInit)
          };

          pp$5.parseSubscript = function(base, startPos, startLoc, noCalls, maybeAsyncArrow, optionalChained, forInit) {
            var optionalSupported = this.options.ecmaVersion >= 11;
            var optional = optionalSupported && this.eat(types$1.questionDot);
            if (noCalls && optional) { this.raise(this.lastTokStart, "Optional chaining cannot appear in the callee of new expressions"); }

            var computed = this.eat(types$1.bracketL);
            if (computed || (optional && this.type !== types$1.parenL && this.type !== types$1.backQuote) || this.eat(types$1.dot)) {
              var node = this.startNodeAt(startPos, startLoc);
              node.object = base;
              if (computed) {
                node.property = this.parseExpression();
                this.expect(types$1.bracketR);
              } else if (this.type === types$1.privateId && base.type !== "Super") {
                node.property = this.parsePrivateIdent();
              } else {
                node.property = this.parseIdent(this.options.allowReserved !== "never");
              }
              node.computed = !!computed;
              if (optionalSupported) {
                node.optional = optional;
              }
              base = this.finishNode(node, "MemberExpression");
            } else if (!noCalls && this.eat(types$1.parenL)) {
              var refDestructuringErrors = new DestructuringErrors, oldYieldPos = this.yieldPos, oldAwaitPos = this.awaitPos, oldAwaitIdentPos = this.awaitIdentPos;
              this.yieldPos = 0;
              this.awaitPos = 0;
              this.awaitIdentPos = 0;
              var exprList = this.parseExprList(types$1.parenR, this.options.ecmaVersion >= 8, false, refDestructuringErrors);
              if (maybeAsyncArrow && !optional && this.shouldParseAsyncArrow()) {
                this.checkPatternErrors(refDestructuringErrors, false);
                this.checkYieldAwaitInDefaultParams();
                if (this.awaitIdentPos > 0)
                  { this.raise(this.awaitIdentPos, "Cannot use 'await' as identifier inside an async function"); }
                this.yieldPos = oldYieldPos;
                this.awaitPos = oldAwaitPos;
                this.awaitIdentPos = oldAwaitIdentPos;
                return this.parseSubscriptAsyncArrow(startPos, startLoc, exprList, forInit)
              }
              this.checkExpressionErrors(refDestructuringErrors, true);
              this.yieldPos = oldYieldPos || this.yieldPos;
              this.awaitPos = oldAwaitPos || this.awaitPos;
              this.awaitIdentPos = oldAwaitIdentPos || this.awaitIdentPos;
              var node$1 = this.startNodeAt(startPos, startLoc);
              node$1.callee = base;
              node$1.arguments = exprList;
              if (optionalSupported) {
                node$1.optional = optional;
              }
              base = this.finishNode(node$1, "CallExpression");
            } else if (this.type === types$1.backQuote) {
              if (optional || optionalChained) {
                this.raise(this.start, "Optional chaining cannot appear in the tag of tagged template expressions");
              }
              var node$2 = this.startNodeAt(startPos, startLoc);
              node$2.tag = base;
              node$2.quasi = this.parseTemplate({isTagged: true});
              base = this.finishNode(node$2, "TaggedTemplateExpression");
            }
            return base
          };

          // Parse an atomic expression — either a single token that is an
          // expression, an expression started by a keyword like `function` or
          // `new`, or an expression wrapped in punctuation like `()`, `[]`,
          // or `{}`.

          pp$5.parseExprAtom = function(refDestructuringErrors, forInit, forNew) {
            // If a division operator appears in an expression position, the
            // tokenizer got confused, and we force it to read a regexp instead.
            if (this.type === types$1.slash) { this.readRegexp(); }

            var node, canBeArrow = this.potentialArrowAt === this.start;
            switch (this.type) {
            case types$1._super:
              if (!this.allowSuper)
                { this.raise(this.start, "'super' keyword outside a method"); }
              node = this.startNode();
              this.next();
              if (this.type === types$1.parenL && !this.allowDirectSuper)
                { this.raise(node.start, "super() call outside constructor of a subclass"); }
              // The `super` keyword can appear at below:
              // SuperProperty:
              //     super [ Expression ]
              //     super . IdentifierName
              // SuperCall:
              //     super ( Arguments )
              if (this.type !== types$1.dot && this.type !== types$1.bracketL && this.type !== types$1.parenL)
                { this.unexpected(); }
              return this.finishNode(node, "Super")

            case types$1._this:
              node = this.startNode();
              this.next();
              return this.finishNode(node, "ThisExpression")

            case types$1.name:
              var startPos = this.start, startLoc = this.startLoc, containsEsc = this.containsEsc;
              var id = this.parseIdent(false);
              if (this.options.ecmaVersion >= 8 && !containsEsc && id.name === "async" && !this.canInsertSemicolon() && this.eat(types$1._function)) {
                this.overrideContext(types.f_expr);
                return this.parseFunction(this.startNodeAt(startPos, startLoc), 0, false, true, forInit)
              }
              if (canBeArrow && !this.canInsertSemicolon()) {
                if (this.eat(types$1.arrow))
                  { return this.parseArrowExpression(this.startNodeAt(startPos, startLoc), [id], false, forInit) }
                if (this.options.ecmaVersion >= 8 && id.name === "async" && this.type === types$1.name && !containsEsc &&
                    (!this.potentialArrowInForAwait || this.value !== "of" || this.containsEsc)) {
                  id = this.parseIdent(false);
                  if (this.canInsertSemicolon() || !this.eat(types$1.arrow))
                    { this.unexpected(); }
                  return this.parseArrowExpression(this.startNodeAt(startPos, startLoc), [id], true, forInit)
                }
              }
              return id

            case types$1.regexp:
              var value = this.value;
              node = this.parseLiteral(value.value);
              node.regex = {pattern: value.pattern, flags: value.flags};
              return node

            case types$1.num: case types$1.string:
              return this.parseLiteral(this.value)

            case types$1._null: case types$1._true: case types$1._false:
              node = this.startNode();
              node.value = this.type === types$1._null ? null : this.type === types$1._true;
              node.raw = this.type.keyword;
              this.next();
              return this.finishNode(node, "Literal")

            case types$1.parenL:
              var start = this.start, expr = this.parseParenAndDistinguishExpression(canBeArrow, forInit);
              if (refDestructuringErrors) {
                if (refDestructuringErrors.parenthesizedAssign < 0 && !this.isSimpleAssignTarget(expr))
                  { refDestructuringErrors.parenthesizedAssign = start; }
                if (refDestructuringErrors.parenthesizedBind < 0)
                  { refDestructuringErrors.parenthesizedBind = start; }
              }
              return expr

            case types$1.bracketL:
              node = this.startNode();
              this.next();
              node.elements = this.parseExprList(types$1.bracketR, true, true, refDestructuringErrors);
              return this.finishNode(node, "ArrayExpression")

            case types$1.braceL:
              this.overrideContext(types.b_expr);
              return this.parseObj(false, refDestructuringErrors)

            case types$1._function:
              node = this.startNode();
              this.next();
              return this.parseFunction(node, 0)

            case types$1._class:
              return this.parseClass(this.startNode(), false)

            case types$1._new:
              return this.parseNew()

            case types$1.backQuote:
              return this.parseTemplate()

            case types$1._import:
              if (this.options.ecmaVersion >= 11) {
                return this.parseExprImport(forNew)
              } else {
                return this.unexpected()
              }

            default:
              return this.parseExprAtomDefault()
            }
          };

          pp$5.parseExprAtomDefault = function() {
            this.unexpected();
          };

          pp$5.parseExprImport = function(forNew) {
            var node = this.startNode();

            // Consume `import` as an identifier for `import.meta`.
            // Because `this.parseIdent(true)` doesn't check escape sequences, it needs the check of `this.containsEsc`.
            if (this.containsEsc) { this.raiseRecoverable(this.start, "Escape sequence in keyword import"); }
            this.next();

            if (this.type === types$1.parenL && !forNew) {
              return this.parseDynamicImport(node)
            } else if (this.type === types$1.dot) {
              var meta = this.startNodeAt(node.start, node.loc && node.loc.start);
              meta.name = "import";
              node.meta = this.finishNode(meta, "Identifier");
              return this.parseImportMeta(node)
            } else {
              this.unexpected();
            }
          };

          pp$5.parseDynamicImport = function(node) {
            this.next(); // skip `(`

            // Parse node.source.
            node.source = this.parseMaybeAssign();

            if (this.options.ecmaVersion >= 16) {
              if (!this.eat(types$1.parenR)) {
                this.expect(types$1.comma);
                if (!this.afterTrailingComma(types$1.parenR)) {
                  node.options = this.parseMaybeAssign();
                  if (!this.eat(types$1.parenR)) {
                    this.expect(types$1.comma);
                    if (!this.afterTrailingComma(types$1.parenR)) {
                      this.unexpected();
                    }
                  }
                } else {
                  node.options = null;
                }
              } else {
                node.options = null;
              }
            } else {
              // Verify ending.
              if (!this.eat(types$1.parenR)) {
                var errorPos = this.start;
                if (this.eat(types$1.comma) && this.eat(types$1.parenR)) {
                  this.raiseRecoverable(errorPos, "Trailing comma is not allowed in import()");
                } else {
                  this.unexpected(errorPos);
                }
              }
            }

            return this.finishNode(node, "ImportExpression")
          };

          pp$5.parseImportMeta = function(node) {
            this.next(); // skip `.`

            var containsEsc = this.containsEsc;
            node.property = this.parseIdent(true);

            if (node.property.name !== "meta")
              { this.raiseRecoverable(node.property.start, "The only valid meta property for import is 'import.meta'"); }
            if (containsEsc)
              { this.raiseRecoverable(node.start, "'import.meta' must not contain escaped characters"); }
            if (this.options.sourceType !== "module" && !this.options.allowImportExportEverywhere)
              { this.raiseRecoverable(node.start, "Cannot use 'import.meta' outside a module"); }

            return this.finishNode(node, "MetaProperty")
          };

          pp$5.parseLiteral = function(value) {
            var node = this.startNode();
            node.value = value;
            node.raw = this.input.slice(this.start, this.end);
            if (node.raw.charCodeAt(node.raw.length - 1) === 110)
              { node.bigint = node.value != null ? node.value.toString() : node.raw.slice(0, -1).replace(/_/g, ""); }
            this.next();
            return this.finishNode(node, "Literal")
          };

          pp$5.parseParenExpression = function() {
            this.expect(types$1.parenL);
            var val = this.parseExpression();
            this.expect(types$1.parenR);
            return val
          };

          pp$5.shouldParseArrow = function(exprList) {
            return !this.canInsertSemicolon()
          };

          pp$5.parseParenAndDistinguishExpression = function(canBeArrow, forInit) {
            var startPos = this.start, startLoc = this.startLoc, val, allowTrailingComma = this.options.ecmaVersion >= 8;
            if (this.options.ecmaVersion >= 6) {
              this.next();

              var innerStartPos = this.start, innerStartLoc = this.startLoc;
              var exprList = [], first = true, lastIsComma = false;
              var refDestructuringErrors = new DestructuringErrors, oldYieldPos = this.yieldPos, oldAwaitPos = this.awaitPos, spreadStart;
              this.yieldPos = 0;
              this.awaitPos = 0;
              // Do not save awaitIdentPos to allow checking awaits nested in parameters
              while (this.type !== types$1.parenR) {
                first ? first = false : this.expect(types$1.comma);
                if (allowTrailingComma && this.afterTrailingComma(types$1.parenR, true)) {
                  lastIsComma = true;
                  break
                } else if (this.type === types$1.ellipsis) {
                  spreadStart = this.start;
                  exprList.push(this.parseParenItem(this.parseRestBinding()));
                  if (this.type === types$1.comma) {
                    this.raiseRecoverable(
                      this.start,
                      "Comma is not permitted after the rest element"
                    );
                  }
                  break
                } else {
                  exprList.push(this.parseMaybeAssign(false, refDestructuringErrors, this.parseParenItem));
                }
              }
              var innerEndPos = this.lastTokEnd, innerEndLoc = this.lastTokEndLoc;
              this.expect(types$1.parenR);

              if (canBeArrow && this.shouldParseArrow(exprList) && this.eat(types$1.arrow)) {
                this.checkPatternErrors(refDestructuringErrors, false);
                this.checkYieldAwaitInDefaultParams();
                this.yieldPos = oldYieldPos;
                this.awaitPos = oldAwaitPos;
                return this.parseParenArrowList(startPos, startLoc, exprList, forInit)
              }

              if (!exprList.length || lastIsComma) { this.unexpected(this.lastTokStart); }
              if (spreadStart) { this.unexpected(spreadStart); }
              this.checkExpressionErrors(refDestructuringErrors, true);
              this.yieldPos = oldYieldPos || this.yieldPos;
              this.awaitPos = oldAwaitPos || this.awaitPos;

              if (exprList.length > 1) {
                val = this.startNodeAt(innerStartPos, innerStartLoc);
                val.expressions = exprList;
                this.finishNodeAt(val, "SequenceExpression", innerEndPos, innerEndLoc);
              } else {
                val = exprList[0];
              }
            } else {
              val = this.parseParenExpression();
            }

            if (this.options.preserveParens) {
              var par = this.startNodeAt(startPos, startLoc);
              par.expression = val;
              return this.finishNode(par, "ParenthesizedExpression")
            } else {
              return val
            }
          };

          pp$5.parseParenItem = function(item) {
            return item
          };

          pp$5.parseParenArrowList = function(startPos, startLoc, exprList, forInit) {
            return this.parseArrowExpression(this.startNodeAt(startPos, startLoc), exprList, false, forInit)
          };

          // New's precedence is slightly tricky. It must allow its argument to
          // be a `[]` or dot subscript expression, but not a call — at least,
          // not without wrapping it in parentheses. Thus, it uses the noCalls
          // argument to parseSubscripts to prevent it from consuming the
          // argument list.

          var empty = [];

          pp$5.parseNew = function() {
            if (this.containsEsc) { this.raiseRecoverable(this.start, "Escape sequence in keyword new"); }
            var node = this.startNode();
            this.next();
            if (this.options.ecmaVersion >= 6 && this.type === types$1.dot) {
              var meta = this.startNodeAt(node.start, node.loc && node.loc.start);
              meta.name = "new";
              node.meta = this.finishNode(meta, "Identifier");
              this.next();
              var containsEsc = this.containsEsc;
              node.property = this.parseIdent(true);
              if (node.property.name !== "target")
                { this.raiseRecoverable(node.property.start, "The only valid meta property for new is 'new.target'"); }
              if (containsEsc)
                { this.raiseRecoverable(node.start, "'new.target' must not contain escaped characters"); }
              if (!this.allowNewDotTarget)
                { this.raiseRecoverable(node.start, "'new.target' can only be used in functions and class static block"); }
              return this.finishNode(node, "MetaProperty")
            }
            var startPos = this.start, startLoc = this.startLoc;
            node.callee = this.parseSubscripts(this.parseExprAtom(null, false, true), startPos, startLoc, true, false);
            if (this.eat(types$1.parenL)) { node.arguments = this.parseExprList(types$1.parenR, this.options.ecmaVersion >= 8, false); }
            else { node.arguments = empty; }
            return this.finishNode(node, "NewExpression")
          };

          // Parse template expression.

          pp$5.parseTemplateElement = function(ref) {
            var isTagged = ref.isTagged;

            var elem = this.startNode();
            if (this.type === types$1.invalidTemplate) {
              if (!isTagged) {
                this.raiseRecoverable(this.start, "Bad escape sequence in untagged template literal");
              }
              elem.value = {
                raw: this.value.replace(/\r\n?/g, "\n"),
                cooked: null
              };
            } else {
              elem.value = {
                raw: this.input.slice(this.start, this.end).replace(/\r\n?/g, "\n"),
                cooked: this.value
              };
            }
            this.next();
            elem.tail = this.type === types$1.backQuote;
            return this.finishNode(elem, "TemplateElement")
          };

          pp$5.parseTemplate = function(ref) {
            if ( ref === void 0 ) ref = {};
            var isTagged = ref.isTagged; if ( isTagged === void 0 ) isTagged = false;

            var node = this.startNode();
            this.next();
            node.expressions = [];
            var curElt = this.parseTemplateElement({isTagged: isTagged});
            node.quasis = [curElt];
            while (!curElt.tail) {
              if (this.type === types$1.eof) { this.raise(this.pos, "Unterminated template literal"); }
              this.expect(types$1.dollarBraceL);
              node.expressions.push(this.parseExpression());
              this.expect(types$1.braceR);
              node.quasis.push(curElt = this.parseTemplateElement({isTagged: isTagged}));
            }
            this.next();
            return this.finishNode(node, "TemplateLiteral")
          };

          pp$5.isAsyncProp = function(prop) {
            return !prop.computed && prop.key.type === "Identifier" && prop.key.name === "async" &&
              (this.type === types$1.name || this.type === types$1.num || this.type === types$1.string || this.type === types$1.bracketL || this.type.keyword || (this.options.ecmaVersion >= 9 && this.type === types$1.star)) &&
              !lineBreak.test(this.input.slice(this.lastTokEnd, this.start))
          };

          // Parse an object literal or binding pattern.

          pp$5.parseObj = function(isPattern, refDestructuringErrors) {
            var node = this.startNode(), first = true, propHash = {};
            node.properties = [];
            this.next();
            while (!this.eat(types$1.braceR)) {
              if (!first) {
                this.expect(types$1.comma);
                if (this.options.ecmaVersion >= 5 && this.afterTrailingComma(types$1.braceR)) { break }
              } else { first = false; }

              var prop = this.parseProperty(isPattern, refDestructuringErrors);
              if (!isPattern) { this.checkPropClash(prop, propHash, refDestructuringErrors); }
              node.properties.push(prop);
            }
            return this.finishNode(node, isPattern ? "ObjectPattern" : "ObjectExpression")
          };

          pp$5.parseProperty = function(isPattern, refDestructuringErrors) {
            var prop = this.startNode(), isGenerator, isAsync, startPos, startLoc;
            if (this.options.ecmaVersion >= 9 && this.eat(types$1.ellipsis)) {
              if (isPattern) {
                prop.argument = this.parseIdent(false);
                if (this.type === types$1.comma) {
                  this.raiseRecoverable(this.start, "Comma is not permitted after the rest element");
                }
                return this.finishNode(prop, "RestElement")
              }
              // Parse argument.
              prop.argument = this.parseMaybeAssign(false, refDestructuringErrors);
              // To disallow trailing comma via `this.toAssignable()`.
              if (this.type === types$1.comma && refDestructuringErrors && refDestructuringErrors.trailingComma < 0) {
                refDestructuringErrors.trailingComma = this.start;
              }
              // Finish
              return this.finishNode(prop, "SpreadElement")
            }
            if (this.options.ecmaVersion >= 6) {
              prop.method = false;
              prop.shorthand = false;
              if (isPattern || refDestructuringErrors) {
                startPos = this.start;
                startLoc = this.startLoc;
              }
              if (!isPattern)
                { isGenerator = this.eat(types$1.star); }
            }
            var containsEsc = this.containsEsc;
            this.parsePropertyName(prop);
            if (!isPattern && !containsEsc && this.options.ecmaVersion >= 8 && !isGenerator && this.isAsyncProp(prop)) {
              isAsync = true;
              isGenerator = this.options.ecmaVersion >= 9 && this.eat(types$1.star);
              this.parsePropertyName(prop);
            } else {
              isAsync = false;
            }
            this.parsePropertyValue(prop, isPattern, isGenerator, isAsync, startPos, startLoc, refDestructuringErrors, containsEsc);
            return this.finishNode(prop, "Property")
          };

          pp$5.parseGetterSetter = function(prop) {
            var kind = prop.key.name;
            this.parsePropertyName(prop);
            prop.value = this.parseMethod(false);
            prop.kind = kind;
            var paramCount = prop.kind === "get" ? 0 : 1;
            if (prop.value.params.length !== paramCount) {
              var start = prop.value.start;
              if (prop.kind === "get")
                { this.raiseRecoverable(start, "getter should have no params"); }
              else
                { this.raiseRecoverable(start, "setter should have exactly one param"); }
            } else {
              if (prop.kind === "set" && prop.value.params[0].type === "RestElement")
                { this.raiseRecoverable(prop.value.params[0].start, "Setter cannot use rest params"); }
            }
          };

          pp$5.parsePropertyValue = function(prop, isPattern, isGenerator, isAsync, startPos, startLoc, refDestructuringErrors, containsEsc) {
            if ((isGenerator || isAsync) && this.type === types$1.colon)
              { this.unexpected(); }

            if (this.eat(types$1.colon)) {
              prop.value = isPattern ? this.parseMaybeDefault(this.start, this.startLoc) : this.parseMaybeAssign(false, refDestructuringErrors);
              prop.kind = "init";
            } else if (this.options.ecmaVersion >= 6 && this.type === types$1.parenL) {
              if (isPattern) { this.unexpected(); }
              prop.method = true;
              prop.value = this.parseMethod(isGenerator, isAsync);
              prop.kind = "init";
            } else if (!isPattern && !containsEsc &&
                       this.options.ecmaVersion >= 5 && !prop.computed && prop.key.type === "Identifier" &&
                       (prop.key.name === "get" || prop.key.name === "set") &&
                       (this.type !== types$1.comma && this.type !== types$1.braceR && this.type !== types$1.eq)) {
              if (isGenerator || isAsync) { this.unexpected(); }
              this.parseGetterSetter(prop);
            } else if (this.options.ecmaVersion >= 6 && !prop.computed && prop.key.type === "Identifier") {
              if (isGenerator || isAsync) { this.unexpected(); }
              this.checkUnreserved(prop.key);
              if (prop.key.name === "await" && !this.awaitIdentPos)
                { this.awaitIdentPos = startPos; }
              if (isPattern) {
                prop.value = this.parseMaybeDefault(startPos, startLoc, this.copyNode(prop.key));
              } else if (this.type === types$1.eq && refDestructuringErrors) {
                if (refDestructuringErrors.shorthandAssign < 0)
                  { refDestructuringErrors.shorthandAssign = this.start; }
                prop.value = this.parseMaybeDefault(startPos, startLoc, this.copyNode(prop.key));
              } else {
                prop.value = this.copyNode(prop.key);
              }
              prop.kind = "init";
              prop.shorthand = true;
            } else { this.unexpected(); }
          };

          pp$5.parsePropertyName = function(prop) {
            if (this.options.ecmaVersion >= 6) {
              if (this.eat(types$1.bracketL)) {
                prop.computed = true;
                prop.key = this.parseMaybeAssign();
                this.expect(types$1.bracketR);
                return prop.key
              } else {
                prop.computed = false;
              }
            }
            return prop.key = this.type === types$1.num || this.type === types$1.string ? this.parseExprAtom() : this.parseIdent(this.options.allowReserved !== "never")
          };

          // Initialize empty function node.

          pp$5.initFunction = function(node) {
            node.id = null;
            if (this.options.ecmaVersion >= 6) { node.generator = node.expression = false; }
            if (this.options.ecmaVersion >= 8) { node.async = false; }
          };

          // Parse object or class method.

          pp$5.parseMethod = function(isGenerator, isAsync, allowDirectSuper) {
            var node = this.startNode(), oldYieldPos = this.yieldPos, oldAwaitPos = this.awaitPos, oldAwaitIdentPos = this.awaitIdentPos;

            this.initFunction(node);
            if (this.options.ecmaVersion >= 6)
              { node.generator = isGenerator; }
            if (this.options.ecmaVersion >= 8)
              { node.async = !!isAsync; }

            this.yieldPos = 0;
            this.awaitPos = 0;
            this.awaitIdentPos = 0;
            this.enterScope(functionFlags(isAsync, node.generator) | SCOPE_SUPER | (allowDirectSuper ? SCOPE_DIRECT_SUPER : 0));

            this.expect(types$1.parenL);
            node.params = this.parseBindingList(types$1.parenR, false, this.options.ecmaVersion >= 8);
            this.checkYieldAwaitInDefaultParams();
            this.parseFunctionBody(node, false, true, false);

            this.yieldPos = oldYieldPos;
            this.awaitPos = oldAwaitPos;
            this.awaitIdentPos = oldAwaitIdentPos;
            return this.finishNode(node, "FunctionExpression")
          };

          // Parse arrow function expression with given parameters.

          pp$5.parseArrowExpression = function(node, params, isAsync, forInit) {
            var oldYieldPos = this.yieldPos, oldAwaitPos = this.awaitPos, oldAwaitIdentPos = this.awaitIdentPos;

            this.enterScope(functionFlags(isAsync, false) | SCOPE_ARROW);
            this.initFunction(node);
            if (this.options.ecmaVersion >= 8) { node.async = !!isAsync; }

            this.yieldPos = 0;
            this.awaitPos = 0;
            this.awaitIdentPos = 0;

            node.params = this.toAssignableList(params, true);
            this.parseFunctionBody(node, true, false, forInit);

            this.yieldPos = oldYieldPos;
            this.awaitPos = oldAwaitPos;
            this.awaitIdentPos = oldAwaitIdentPos;
            return this.finishNode(node, "ArrowFunctionExpression")
          };

          // Parse function body and check parameters.

          pp$5.parseFunctionBody = function(node, isArrowFunction, isMethod, forInit) {
            var isExpression = isArrowFunction && this.type !== types$1.braceL;
            var oldStrict = this.strict, useStrict = false;

            if (isExpression) {
              node.body = this.parseMaybeAssign(forInit);
              node.expression = true;
              this.checkParams(node, false);
            } else {
              var nonSimple = this.options.ecmaVersion >= 7 && !this.isSimpleParamList(node.params);
              if (!oldStrict || nonSimple) {
                useStrict = this.strictDirective(this.end);
                // If this is a strict mode function, verify that argument names
                // are not repeated, and it does not try to bind the words `eval`
                // or `arguments`.
                if (useStrict && nonSimple)
                  { this.raiseRecoverable(node.start, "Illegal 'use strict' directive in function with non-simple parameter list"); }
              }
              // Start a new scope with regard to labels and the `inFunction`
              // flag (restore them to their old value afterwards).
              var oldLabels = this.labels;
              this.labels = [];
              if (useStrict) { this.strict = true; }

              // Add the params to varDeclaredNames to ensure that an error is thrown
              // if a let/const declaration in the function clashes with one of the params.
              this.checkParams(node, !oldStrict && !useStrict && !isArrowFunction && !isMethod && this.isSimpleParamList(node.params));
              // Ensure the function name isn't a forbidden identifier in strict mode, e.g. 'eval'
              if (this.strict && node.id) { this.checkLValSimple(node.id, BIND_OUTSIDE); }
              node.body = this.parseBlock(false, undefined, useStrict && !oldStrict);
              node.expression = false;
              this.adaptDirectivePrologue(node.body.body);
              this.labels = oldLabels;
            }
            this.exitScope();
          };

          pp$5.isSimpleParamList = function(params) {
            for (var i = 0, list = params; i < list.length; i += 1)
              {
              var param = list[i];

              if (param.type !== "Identifier") { return false
            } }
            return true
          };

          // Checks function params for various disallowed patterns such as using "eval"
          // or "arguments" and duplicate parameters.

          pp$5.checkParams = function(node, allowDuplicates) {
            var nameHash = Object.create(null);
            for (var i = 0, list = node.params; i < list.length; i += 1)
              {
              var param = list[i];

              this.checkLValInnerPattern(param, BIND_VAR, allowDuplicates ? null : nameHash);
            }
          };

          // Parses a comma-separated list of expressions, and returns them as
          // an array. `close` is the token type that ends the list, and
          // `allowEmpty` can be turned on to allow subsequent commas with
          // nothing in between them to be parsed as `null` (which is needed
          // for array literals).

          pp$5.parseExprList = function(close, allowTrailingComma, allowEmpty, refDestructuringErrors) {
            var elts = [], first = true;
            while (!this.eat(close)) {
              if (!first) {
                this.expect(types$1.comma);
                if (allowTrailingComma && this.afterTrailingComma(close)) { break }
              } else { first = false; }

              var elt = (void 0);
              if (allowEmpty && this.type === types$1.comma)
                { elt = null; }
              else if (this.type === types$1.ellipsis) {
                elt = this.parseSpread(refDestructuringErrors);
                if (refDestructuringErrors && this.type === types$1.comma && refDestructuringErrors.trailingComma < 0)
                  { refDestructuringErrors.trailingComma = this.start; }
              } else {
                elt = this.parseMaybeAssign(false, refDestructuringErrors);
              }
              elts.push(elt);
            }
            return elts
          };

          pp$5.checkUnreserved = function(ref) {
            var start = ref.start;
            var end = ref.end;
            var name = ref.name;

            if (this.inGenerator && name === "yield")
              { this.raiseRecoverable(start, "Cannot use 'yield' as identifier inside a generator"); }
            if (this.inAsync && name === "await")
              { this.raiseRecoverable(start, "Cannot use 'await' as identifier inside an async function"); }
            if (!(this.currentThisScope().flags & SCOPE_VAR) && name === "arguments")
              { this.raiseRecoverable(start, "Cannot use 'arguments' in class field initializer"); }
            if (this.inClassStaticBlock && (name === "arguments" || name === "await"))
              { this.raise(start, ("Cannot use " + name + " in class static initialization block")); }
            if (this.keywords.test(name))
              { this.raise(start, ("Unexpected keyword '" + name + "'")); }
            if (this.options.ecmaVersion < 6 &&
              this.input.slice(start, end).indexOf("\\") !== -1) { return }
            var re = this.strict ? this.reservedWordsStrict : this.reservedWords;
            if (re.test(name)) {
              if (!this.inAsync && name === "await")
                { this.raiseRecoverable(start, "Cannot use keyword 'await' outside an async function"); }
              this.raiseRecoverable(start, ("The keyword '" + name + "' is reserved"));
            }
          };

          // Parse the next token as an identifier. If `liberal` is true (used
          // when parsing properties), it will also convert keywords into
          // identifiers.

          pp$5.parseIdent = function(liberal) {
            var node = this.parseIdentNode();
            this.next(!!liberal);
            this.finishNode(node, "Identifier");
            if (!liberal) {
              this.checkUnreserved(node);
              if (node.name === "await" && !this.awaitIdentPos)
                { this.awaitIdentPos = node.start; }
            }
            return node
          };

          pp$5.parseIdentNode = function() {
            var node = this.startNode();
            if (this.type === types$1.name) {
              node.name = this.value;
            } else if (this.type.keyword) {
              node.name = this.type.keyword;

              // To fix https://github.com/acornjs/acorn/issues/575
              // `class` and `function` keywords push new context into this.context.
              // But there is no chance to pop the context if the keyword is consumed as an identifier such as a property name.
              // If the previous token is a dot, this does not apply because the context-managing code already ignored the keyword
              if ((node.name === "class" || node.name === "function") &&
                (this.lastTokEnd !== this.lastTokStart + 1 || this.input.charCodeAt(this.lastTokStart) !== 46)) {
                this.context.pop();
              }
              this.type = types$1.name;
            } else {
              this.unexpected();
            }
            return node
          };

          pp$5.parsePrivateIdent = function() {
            var node = this.startNode();
            if (this.type === types$1.privateId) {
              node.name = this.value;
            } else {
              this.unexpected();
            }
            this.next();
            this.finishNode(node, "PrivateIdentifier");

            // For validating existence
            if (this.options.checkPrivateFields) {
              if (this.privateNameStack.length === 0) {
                this.raise(node.start, ("Private field '#" + (node.name) + "' must be declared in an enclosing class"));
              } else {
                this.privateNameStack[this.privateNameStack.length - 1].used.push(node);
              }
            }

            return node
          };

          // Parses yield expression inside generator.

          pp$5.parseYield = function(forInit) {
            if (!this.yieldPos) { this.yieldPos = this.start; }

            var node = this.startNode();
            this.next();
            if (this.type === types$1.semi || this.canInsertSemicolon() || (this.type !== types$1.star && !this.type.startsExpr)) {
              node.delegate = false;
              node.argument = null;
            } else {
              node.delegate = this.eat(types$1.star);
              node.argument = this.parseMaybeAssign(forInit);
            }
            return this.finishNode(node, "YieldExpression")
          };

          pp$5.parseAwait = function(forInit) {
            if (!this.awaitPos) { this.awaitPos = this.start; }

            var node = this.startNode();
            this.next();
            node.argument = this.parseMaybeUnary(null, true, false, forInit);
            return this.finishNode(node, "AwaitExpression")
          };

          var pp$4 = Parser.prototype;

          // This function is used to raise exceptions on parse errors. It
          // takes an offset integer (into the current `input`) to indicate
          // the location of the error, attaches the position to the end
          // of the error message, and then raises a `SyntaxError` with that
          // message.

          pp$4.raise = function(pos, message) {
            var loc = getLineInfo(this.input, pos);
            message += " (" + loc.line + ":" + loc.column + ")";
            if (this.sourceFile) {
              message += " in " + this.sourceFile;
            }
            var err = new SyntaxError(message);
            err.pos = pos; err.loc = loc; err.raisedAt = this.pos;
            throw err
          };

          pp$4.raiseRecoverable = pp$4.raise;

          pp$4.curPosition = function() {
            if (this.options.locations) {
              return new Position(this.curLine, this.pos - this.lineStart)
            }
          };

          var pp$3 = Parser.prototype;

          var Scope = function Scope(flags) {
            this.flags = flags;
            // A list of var-declared names in the current lexical scope
            this.var = [];
            // A list of lexically-declared names in the current lexical scope
            this.lexical = [];
            // A list of lexically-declared FunctionDeclaration names in the current lexical scope
            this.functions = [];
          };

          // The functions in this module keep track of declared variables in the current scope in order to detect duplicate variable names.

          pp$3.enterScope = function(flags) {
            this.scopeStack.push(new Scope(flags));
          };

          pp$3.exitScope = function() {
            this.scopeStack.pop();
          };

          // The spec says:
          // > At the top level of a function, or script, function declarations are
          // > treated like var declarations rather than like lexical declarations.
          pp$3.treatFunctionsAsVarInScope = function(scope) {
            return (scope.flags & SCOPE_FUNCTION) || !this.inModule && (scope.flags & SCOPE_TOP)
          };

          pp$3.declareName = function(name, bindingType, pos) {
            var redeclared = false;
            if (bindingType === BIND_LEXICAL) {
              var scope = this.currentScope();
              redeclared = scope.lexical.indexOf(name) > -1 || scope.functions.indexOf(name) > -1 || scope.var.indexOf(name) > -1;
              scope.lexical.push(name);
              if (this.inModule && (scope.flags & SCOPE_TOP))
                { delete this.undefinedExports[name]; }
            } else if (bindingType === BIND_SIMPLE_CATCH) {
              var scope$1 = this.currentScope();
              scope$1.lexical.push(name);
            } else if (bindingType === BIND_FUNCTION) {
              var scope$2 = this.currentScope();
              if (this.treatFunctionsAsVar)
                { redeclared = scope$2.lexical.indexOf(name) > -1; }
              else
                { redeclared = scope$2.lexical.indexOf(name) > -1 || scope$2.var.indexOf(name) > -1; }
              scope$2.functions.push(name);
            } else {
              for (var i = this.scopeStack.length - 1; i >= 0; --i) {
                var scope$3 = this.scopeStack[i];
                if (scope$3.lexical.indexOf(name) > -1 && !((scope$3.flags & SCOPE_SIMPLE_CATCH) && scope$3.lexical[0] === name) ||
                    !this.treatFunctionsAsVarInScope(scope$3) && scope$3.functions.indexOf(name) > -1) {
                  redeclared = true;
                  break
                }
                scope$3.var.push(name);
                if (this.inModule && (scope$3.flags & SCOPE_TOP))
                  { delete this.undefinedExports[name]; }
                if (scope$3.flags & SCOPE_VAR) { break }
              }
            }
            if (redeclared) { this.raiseRecoverable(pos, ("Identifier '" + name + "' has already been declared")); }
          };

          pp$3.checkLocalExport = function(id) {
            // scope.functions must be empty as Module code is always strict.
            if (this.scopeStack[0].lexical.indexOf(id.name) === -1 &&
                this.scopeStack[0].var.indexOf(id.name) === -1) {
              this.undefinedExports[id.name] = id;
            }
          };

          pp$3.currentScope = function() {
            return this.scopeStack[this.scopeStack.length - 1]
          };

          pp$3.currentVarScope = function() {
            for (var i = this.scopeStack.length - 1;; i--) {
              var scope = this.scopeStack[i];
              if (scope.flags & (SCOPE_VAR | SCOPE_CLASS_FIELD_INIT | SCOPE_CLASS_STATIC_BLOCK)) { return scope }
            }
          };

          // Could be useful for `this`, `new.target`, `super()`, `super.property`, and `super[property]`.
          pp$3.currentThisScope = function() {
            for (var i = this.scopeStack.length - 1;; i--) {
              var scope = this.scopeStack[i];
              if (scope.flags & (SCOPE_VAR | SCOPE_CLASS_FIELD_INIT | SCOPE_CLASS_STATIC_BLOCK) &&
                  !(scope.flags & SCOPE_ARROW)) { return scope }
            }
          };

          var Node = function Node(parser, pos, loc) {
            this.type = "";
            this.start = pos;
            this.end = 0;
            if (parser.options.locations)
              { this.loc = new SourceLocation(parser, loc); }
            if (parser.options.directSourceFile)
              { this.sourceFile = parser.options.directSourceFile; }
            if (parser.options.ranges)
              { this.range = [pos, 0]; }
          };

          // Start an AST node, attaching a start offset.

          var pp$2 = Parser.prototype;

          pp$2.startNode = function() {
            return new Node(this, this.start, this.startLoc)
          };

          pp$2.startNodeAt = function(pos, loc) {
            return new Node(this, pos, loc)
          };

          // Finish an AST node, adding `type` and `end` properties.

          function finishNodeAt(node, type, pos, loc) {
            node.type = type;
            node.end = pos;
            if (this.options.locations)
              { node.loc.end = loc; }
            if (this.options.ranges)
              { node.range[1] = pos; }
            return node
          }

          pp$2.finishNode = function(node, type) {
            return finishNodeAt.call(this, node, type, this.lastTokEnd, this.lastTokEndLoc)
          };

          // Finish node at given position

          pp$2.finishNodeAt = function(node, type, pos, loc) {
            return finishNodeAt.call(this, node, type, pos, loc)
          };

          pp$2.copyNode = function(node) {
            var newNode = new Node(this, node.start, this.startLoc);
            for (var prop in node) { newNode[prop] = node[prop]; }
            return newNode
          };

          // This file was generated by "bin/generate-unicode-script-values.js". Do not modify manually!
          var scriptValuesAddedInUnicode = "Gara Garay Gukh Gurung_Khema Hrkt Katakana_Or_Hiragana Kawi Kirat_Rai Krai Nag_Mundari Nagm Ol_Onal Onao Sunu Sunuwar Todhri Todr Tulu_Tigalari Tutg Unknown Zzzz";

          // This file contains Unicode properties extracted from the ECMAScript specification.
          // The lists are extracted like so:
          // $$('#table-binary-unicode-properties > figure > table > tbody > tr > td:nth-child(1) code').map(el => el.innerText)

          // #table-binary-unicode-properties
          var ecma9BinaryProperties = "ASCII ASCII_Hex_Digit AHex Alphabetic Alpha Any Assigned Bidi_Control Bidi_C Bidi_Mirrored Bidi_M Case_Ignorable CI Cased Changes_When_Casefolded CWCF Changes_When_Casemapped CWCM Changes_When_Lowercased CWL Changes_When_NFKC_Casefolded CWKCF Changes_When_Titlecased CWT Changes_When_Uppercased CWU Dash Default_Ignorable_Code_Point DI Deprecated Dep Diacritic Dia Emoji Emoji_Component Emoji_Modifier Emoji_Modifier_Base Emoji_Presentation Extender Ext Grapheme_Base Gr_Base Grapheme_Extend Gr_Ext Hex_Digit Hex IDS_Binary_Operator IDSB IDS_Trinary_Operator IDST ID_Continue IDC ID_Start IDS Ideographic Ideo Join_Control Join_C Logical_Order_Exception LOE Lowercase Lower Math Noncharacter_Code_Point NChar Pattern_Syntax Pat_Syn Pattern_White_Space Pat_WS Quotation_Mark QMark Radical Regional_Indicator RI Sentence_Terminal STerm Soft_Dotted SD Terminal_Punctuation Term Unified_Ideograph UIdeo Uppercase Upper Variation_Selector VS White_Space space XID_Continue XIDC XID_Start XIDS";
          var ecma10BinaryProperties = ecma9BinaryProperties + " Extended_Pictographic";
          var ecma11BinaryProperties = ecma10BinaryProperties;
          var ecma12BinaryProperties = ecma11BinaryProperties + " EBase EComp EMod EPres ExtPict";
          var ecma13BinaryProperties = ecma12BinaryProperties;
          var ecma14BinaryProperties = ecma13BinaryProperties;

          var unicodeBinaryProperties = {
            9: ecma9BinaryProperties,
            10: ecma10BinaryProperties,
            11: ecma11BinaryProperties,
            12: ecma12BinaryProperties,
            13: ecma13BinaryProperties,
            14: ecma14BinaryProperties
          };

          // #table-binary-unicode-properties-of-strings
          var ecma14BinaryPropertiesOfStrings = "Basic_Emoji Emoji_Keycap_Sequence RGI_Emoji_Modifier_Sequence RGI_Emoji_Flag_Sequence RGI_Emoji_Tag_Sequence RGI_Emoji_ZWJ_Sequence RGI_Emoji";

          var unicodeBinaryPropertiesOfStrings = {
            9: "",
            10: "",
            11: "",
            12: "",
            13: "",
            14: ecma14BinaryPropertiesOfStrings
          };

          // #table-unicode-general-category-values
          var unicodeGeneralCategoryValues = "Cased_Letter LC Close_Punctuation Pe Connector_Punctuation Pc Control Cc cntrl Currency_Symbol Sc Dash_Punctuation Pd Decimal_Number Nd digit Enclosing_Mark Me Final_Punctuation Pf Format Cf Initial_Punctuation Pi Letter L Letter_Number Nl Line_Separator Zl Lowercase_Letter Ll Mark M Combining_Mark Math_Symbol Sm Modifier_Letter Lm Modifier_Symbol Sk Nonspacing_Mark Mn Number N Open_Punctuation Ps Other C Other_Letter Lo Other_Number No Other_Punctuation Po Other_Symbol So Paragraph_Separator Zp Private_Use Co Punctuation P punct Separator Z Space_Separator Zs Spacing_Mark Mc Surrogate Cs Symbol S Titlecase_Letter Lt Unassigned Cn Uppercase_Letter Lu";

          // #table-unicode-script-values
          var ecma9ScriptValues = "Adlam Adlm Ahom Anatolian_Hieroglyphs Hluw Arabic Arab Armenian Armn Avestan Avst Balinese Bali Bamum Bamu Bassa_Vah Bass Batak Batk Bengali Beng Bhaiksuki Bhks Bopomofo Bopo Brahmi Brah Braille Brai Buginese Bugi Buhid Buhd Canadian_Aboriginal Cans Carian Cari Caucasian_Albanian Aghb Chakma Cakm Cham Cham Cherokee Cher Common Zyyy Coptic Copt Qaac Cuneiform Xsux Cypriot Cprt Cyrillic Cyrl Deseret Dsrt Devanagari Deva Duployan Dupl Egyptian_Hieroglyphs Egyp Elbasan Elba Ethiopic Ethi Georgian Geor Glagolitic Glag Gothic Goth Grantha Gran Greek Grek Gujarati Gujr Gurmukhi Guru Han Hani Hangul Hang Hanunoo Hano Hatran Hatr Hebrew Hebr Hiragana Hira Imperial_Aramaic Armi Inherited Zinh Qaai Inscriptional_Pahlavi Phli Inscriptional_Parthian Prti Javanese Java Kaithi Kthi Kannada Knda Katakana Kana Kayah_Li Kali Kharoshthi Khar Khmer Khmr Khojki Khoj Khudawadi Sind Lao Laoo Latin Latn Lepcha Lepc Limbu Limb Linear_A Lina Linear_B Linb Lisu Lisu Lycian Lyci Lydian Lydi Mahajani Mahj Malayalam Mlym Mandaic Mand Manichaean Mani Marchen Marc Masaram_Gondi Gonm Meetei_Mayek Mtei Mende_Kikakui Mend Meroitic_Cursive Merc Meroitic_Hieroglyphs Mero Miao Plrd Modi Mongolian Mong Mro Mroo Multani Mult Myanmar Mymr Nabataean Nbat New_Tai_Lue Talu Newa Newa Nko Nkoo Nushu Nshu Ogham Ogam Ol_Chiki Olck Old_Hungarian Hung Old_Italic Ital Old_North_Arabian Narb Old_Permic Perm Old_Persian Xpeo Old_South_Arabian Sarb Old_Turkic Orkh Oriya Orya Osage Osge Osmanya Osma Pahawh_Hmong Hmng Palmyrene Palm Pau_Cin_Hau Pauc Phags_Pa Phag Phoenician Phnx Psalter_Pahlavi Phlp Rejang Rjng Runic Runr Samaritan Samr Saurashtra Saur Sharada Shrd Shavian Shaw Siddham Sidd SignWriting Sgnw Sinhala Sinh Sora_Sompeng Sora Soyombo Soyo Sundanese Sund Syloti_Nagri Sylo Syriac Syrc Tagalog Tglg Tagbanwa Tagb Tai_Le Tale Tai_Tham Lana Tai_Viet Tavt Takri Takr Tamil Taml Tangut Tang Telugu Telu Thaana Thaa Thai Thai Tibetan Tibt Tifinagh Tfng Tirhuta Tirh Ugaritic Ugar Vai Vaii Warang_Citi Wara Yi Yiii Zanabazar_Square Zanb";
          var ecma10ScriptValues = ecma9ScriptValues + " Dogra Dogr Gunjala_Gondi Gong Hanifi_Rohingya Rohg Makasar Maka Medefaidrin Medf Old_Sogdian Sogo Sogdian Sogd";
          var ecma11ScriptValues = ecma10ScriptValues + " Elymaic Elym Nandinagari Nand Nyiakeng_Puachue_Hmong Hmnp Wancho Wcho";
          var ecma12ScriptValues = ecma11ScriptValues + " Chorasmian Chrs Diak Dives_Akuru Khitan_Small_Script Kits Yezi Yezidi";
          var ecma13ScriptValues = ecma12ScriptValues + " Cypro_Minoan Cpmn Old_Uyghur Ougr Tangsa Tnsa Toto Vithkuqi Vith";
          var ecma14ScriptValues = ecma13ScriptValues + " " + scriptValuesAddedInUnicode;

          var unicodeScriptValues = {
            9: ecma9ScriptValues,
            10: ecma10ScriptValues,
            11: ecma11ScriptValues,
            12: ecma12ScriptValues,
            13: ecma13ScriptValues,
            14: ecma14ScriptValues
          };

          var data = {};
          function buildUnicodeData(ecmaVersion) {
            var d = data[ecmaVersion] = {
              binary: wordsRegexp(unicodeBinaryProperties[ecmaVersion] + " " + unicodeGeneralCategoryValues),
              binaryOfStrings: wordsRegexp(unicodeBinaryPropertiesOfStrings[ecmaVersion]),
              nonBinary: {
                General_Category: wordsRegexp(unicodeGeneralCategoryValues),
                Script: wordsRegexp(unicodeScriptValues[ecmaVersion])
              }
            };
            d.nonBinary.Script_Extensions = d.nonBinary.Script;

            d.nonBinary.gc = d.nonBinary.General_Category;
            d.nonBinary.sc = d.nonBinary.Script;
            d.nonBinary.scx = d.nonBinary.Script_Extensions;
          }

          for (var i = 0, list = [9, 10, 11, 12, 13, 14]; i < list.length; i += 1) {
            var ecmaVersion = list[i];

            buildUnicodeData(ecmaVersion);
          }

          var pp$1 = Parser.prototype;

          // Track disjunction structure to determine whether a duplicate
          // capture group name is allowed because it is in a separate branch.
          var BranchID = function BranchID(parent, base) {
            // Parent disjunction branch
            this.parent = parent;
            // Identifies this set of sibling branches
            this.base = base || this;
          };

          BranchID.prototype.separatedFrom = function separatedFrom (alt) {
            // A branch is separate from another branch if they or any of
            // their parents are siblings in a given disjunction
            for (var self = this; self; self = self.parent) {
              for (var other = alt; other; other = other.parent) {
                if (self.base === other.base && self !== other) { return true }
              }
            }
            return false
          };

          BranchID.prototype.sibling = function sibling () {
            return new BranchID(this.parent, this.base)
          };

          var RegExpValidationState = function RegExpValidationState(parser) {
            this.parser = parser;
            this.validFlags = "gim" + (parser.options.ecmaVersion >= 6 ? "uy" : "") + (parser.options.ecmaVersion >= 9 ? "s" : "") + (parser.options.ecmaVersion >= 13 ? "d" : "") + (parser.options.ecmaVersion >= 15 ? "v" : "");
            this.unicodeProperties = data[parser.options.ecmaVersion >= 14 ? 14 : parser.options.ecmaVersion];
            this.source = "";
            this.flags = "";
            this.start = 0;
            this.switchU = false;
            this.switchV = false;
            this.switchN = false;
            this.pos = 0;
            this.lastIntValue = 0;
            this.lastStringValue = "";
            this.lastAssertionIsQuantifiable = false;
            this.numCapturingParens = 0;
            this.maxBackReference = 0;
            this.groupNames = Object.create(null);
            this.backReferenceNames = [];
            this.branchID = null;
          };

          RegExpValidationState.prototype.reset = function reset (start, pattern, flags) {
            var unicodeSets = flags.indexOf("v") !== -1;
            var unicode = flags.indexOf("u") !== -1;
            this.start = start | 0;
            this.source = pattern + "";
            this.flags = flags;
            if (unicodeSets && this.parser.options.ecmaVersion >= 15) {
              this.switchU = true;
              this.switchV = true;
              this.switchN = true;
            } else {
              this.switchU = unicode && this.parser.options.ecmaVersion >= 6;
              this.switchV = false;
              this.switchN = unicode && this.parser.options.ecmaVersion >= 9;
            }
          };

          RegExpValidationState.prototype.raise = function raise (message) {
            this.parser.raiseRecoverable(this.start, ("Invalid regular expression: /" + (this.source) + "/: " + message));
          };

          // If u flag is given, this returns the code point at the index (it combines a surrogate pair).
          // Otherwise, this returns the code unit of the index (can be a part of a surrogate pair).
          RegExpValidationState.prototype.at = function at (i, forceU) {
              if ( forceU === void 0 ) forceU = false;

            var s = this.source;
            var l = s.length;
            if (i >= l) {
              return -1
            }
            var c = s.charCodeAt(i);
            if (!(forceU || this.switchU) || c <= 0xD7FF || c >= 0xE000 || i + 1 >= l) {
              return c
            }
            var next = s.charCodeAt(i + 1);
            return next >= 0xDC00 && next <= 0xDFFF ? (c << 10) + next - 0x35FDC00 : c
          };

          RegExpValidationState.prototype.nextIndex = function nextIndex (i, forceU) {
              if ( forceU === void 0 ) forceU = false;

            var s = this.source;
            var l = s.length;
            if (i >= l) {
              return l
            }
            var c = s.charCodeAt(i), next;
            if (!(forceU || this.switchU) || c <= 0xD7FF || c >= 0xE000 || i + 1 >= l ||
                (next = s.charCodeAt(i + 1)) < 0xDC00 || next > 0xDFFF) {
              return i + 1
            }
            return i + 2
          };

          RegExpValidationState.prototype.current = function current (forceU) {
              if ( forceU === void 0 ) forceU = false;

            return this.at(this.pos, forceU)
          };

          RegExpValidationState.prototype.lookahead = function lookahead (forceU) {
              if ( forceU === void 0 ) forceU = false;

            return this.at(this.nextIndex(this.pos, forceU), forceU)
          };

          RegExpValidationState.prototype.advance = function advance (forceU) {
              if ( forceU === void 0 ) forceU = false;

            this.pos = this.nextIndex(this.pos, forceU);
          };

          RegExpValidationState.prototype.eat = function eat (ch, forceU) {
              if ( forceU === void 0 ) forceU = false;

            if (this.current(forceU) === ch) {
              this.advance(forceU);
              return true
            }
            return false
          };

          RegExpValidationState.prototype.eatChars = function eatChars (chs, forceU) {
              if ( forceU === void 0 ) forceU = false;

            var pos = this.pos;
            for (var i = 0, list = chs; i < list.length; i += 1) {
              var ch = list[i];

                var current = this.at(pos, forceU);
              if (current === -1 || current !== ch) {
                return false
              }
              pos = this.nextIndex(pos, forceU);
            }
            this.pos = pos;
            return true
          };

          /**
           * Validate the flags part of a given RegExpLiteral.
           *
           * @param {RegExpValidationState} state The state to validate RegExp.
           * @returns {void}
           */
          pp$1.validateRegExpFlags = function(state) {
            var validFlags = state.validFlags;
            var flags = state.flags;

            var u = false;
            var v = false;

            for (var i = 0; i < flags.length; i++) {
              var flag = flags.charAt(i);
              if (validFlags.indexOf(flag) === -1) {
                this.raise(state.start, "Invalid regular expression flag");
              }
              if (flags.indexOf(flag, i + 1) > -1) {
                this.raise(state.start, "Duplicate regular expression flag");
              }
              if (flag === "u") { u = true; }
              if (flag === "v") { v = true; }
            }
            if (this.options.ecmaVersion >= 15 && u && v) {
              this.raise(state.start, "Invalid regular expression flag");
            }
          };

          function hasProp(obj) {
            for (var _ in obj) { return true }
            return false
          }

          /**
           * Validate the pattern part of a given RegExpLiteral.
           *
           * @param {RegExpValidationState} state The state to validate RegExp.
           * @returns {void}
           */
          pp$1.validateRegExpPattern = function(state) {
            this.regexp_pattern(state);

            // The goal symbol for the parse is |Pattern[~U, ~N]|. If the result of
            // parsing contains a |GroupName|, reparse with the goal symbol
            // |Pattern[~U, +N]| and use this result instead. Throw a *SyntaxError*
            // exception if _P_ did not conform to the grammar, if any elements of _P_
            // were not matched by the parse, or if any Early Error conditions exist.
            if (!state.switchN && this.options.ecmaVersion >= 9 && hasProp(state.groupNames)) {
              state.switchN = true;
              this.regexp_pattern(state);
            }
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-Pattern
          pp$1.regexp_pattern = function(state) {
            state.pos = 0;
            state.lastIntValue = 0;
            state.lastStringValue = "";
            state.lastAssertionIsQuantifiable = false;
            state.numCapturingParens = 0;
            state.maxBackReference = 0;
            state.groupNames = Object.create(null);
            state.backReferenceNames.length = 0;
            state.branchID = null;

            this.regexp_disjunction(state);

            if (state.pos !== state.source.length) {
              // Make the same messages as V8.
              if (state.eat(0x29 /* ) */)) {
                state.raise("Unmatched ')'");
              }
              if (state.eat(0x5D /* ] */) || state.eat(0x7D /* } */)) {
                state.raise("Lone quantifier brackets");
              }
            }
            if (state.maxBackReference > state.numCapturingParens) {
              state.raise("Invalid escape");
            }
            for (var i = 0, list = state.backReferenceNames; i < list.length; i += 1) {
              var name = list[i];

              if (!state.groupNames[name]) {
                state.raise("Invalid named capture referenced");
              }
            }
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-Disjunction
          pp$1.regexp_disjunction = function(state) {
            var trackDisjunction = this.options.ecmaVersion >= 16;
            if (trackDisjunction) { state.branchID = new BranchID(state.branchID, null); }
            this.regexp_alternative(state);
            while (state.eat(0x7C /* | */)) {
              if (trackDisjunction) { state.branchID = state.branchID.sibling(); }
              this.regexp_alternative(state);
            }
            if (trackDisjunction) { state.branchID = state.branchID.parent; }

            // Make the same message as V8.
            if (this.regexp_eatQuantifier(state, true)) {
              state.raise("Nothing to repeat");
            }
            if (state.eat(0x7B /* { */)) {
              state.raise("Lone quantifier brackets");
            }
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-Alternative
          pp$1.regexp_alternative = function(state) {
            while (state.pos < state.source.length && this.regexp_eatTerm(state)) {}
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-Term
          pp$1.regexp_eatTerm = function(state) {
            if (this.regexp_eatAssertion(state)) {
              // Handle `QuantifiableAssertion Quantifier` alternative.
              // `state.lastAssertionIsQuantifiable` is true if the last eaten Assertion
              // is a QuantifiableAssertion.
              if (state.lastAssertionIsQuantifiable && this.regexp_eatQuantifier(state)) {
                // Make the same message as V8.
                if (state.switchU) {
                  state.raise("Invalid quantifier");
                }
              }
              return true
            }

            if (state.switchU ? this.regexp_eatAtom(state) : this.regexp_eatExtendedAtom(state)) {
              this.regexp_eatQuantifier(state);
              return true
            }

            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-Assertion
          pp$1.regexp_eatAssertion = function(state) {
            var start = state.pos;
            state.lastAssertionIsQuantifiable = false;

            // ^, $
            if (state.eat(0x5E /* ^ */) || state.eat(0x24 /* $ */)) {
              return true
            }

            // \b \B
            if (state.eat(0x5C /* \ */)) {
              if (state.eat(0x42 /* B */) || state.eat(0x62 /* b */)) {
                return true
              }
              state.pos = start;
            }

            // Lookahead / Lookbehind
            if (state.eat(0x28 /* ( */) && state.eat(0x3F /* ? */)) {
              var lookbehind = false;
              if (this.options.ecmaVersion >= 9) {
                lookbehind = state.eat(0x3C /* < */);
              }
              if (state.eat(0x3D /* = */) || state.eat(0x21 /* ! */)) {
                this.regexp_disjunction(state);
                if (!state.eat(0x29 /* ) */)) {
                  state.raise("Unterminated group");
                }
                state.lastAssertionIsQuantifiable = !lookbehind;
                return true
              }
            }

            state.pos = start;
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-Quantifier
          pp$1.regexp_eatQuantifier = function(state, noError) {
            if ( noError === void 0 ) noError = false;

            if (this.regexp_eatQuantifierPrefix(state, noError)) {
              state.eat(0x3F /* ? */);
              return true
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-QuantifierPrefix
          pp$1.regexp_eatQuantifierPrefix = function(state, noError) {
            return (
              state.eat(0x2A /* * */) ||
              state.eat(0x2B /* + */) ||
              state.eat(0x3F /* ? */) ||
              this.regexp_eatBracedQuantifier(state, noError)
            )
          };
          pp$1.regexp_eatBracedQuantifier = function(state, noError) {
            var start = state.pos;
            if (state.eat(0x7B /* { */)) {
              var min = 0, max = -1;
              if (this.regexp_eatDecimalDigits(state)) {
                min = state.lastIntValue;
                if (state.eat(0x2C /* , */) && this.regexp_eatDecimalDigits(state)) {
                  max = state.lastIntValue;
                }
                if (state.eat(0x7D /* } */)) {
                  // SyntaxError in https://www.ecma-international.org/ecma-262/8.0/#sec-term
                  if (max !== -1 && max < min && !noError) {
                    state.raise("numbers out of order in {} quantifier");
                  }
                  return true
                }
              }
              if (state.switchU && !noError) {
                state.raise("Incomplete quantifier");
              }
              state.pos = start;
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-Atom
          pp$1.regexp_eatAtom = function(state) {
            return (
              this.regexp_eatPatternCharacters(state) ||
              state.eat(0x2E /* . */) ||
              this.regexp_eatReverseSolidusAtomEscape(state) ||
              this.regexp_eatCharacterClass(state) ||
              this.regexp_eatUncapturingGroup(state) ||
              this.regexp_eatCapturingGroup(state)
            )
          };
          pp$1.regexp_eatReverseSolidusAtomEscape = function(state) {
            var start = state.pos;
            if (state.eat(0x5C /* \ */)) {
              if (this.regexp_eatAtomEscape(state)) {
                return true
              }
              state.pos = start;
            }
            return false
          };
          pp$1.regexp_eatUncapturingGroup = function(state) {
            var start = state.pos;
            if (state.eat(0x28 /* ( */)) {
              if (state.eat(0x3F /* ? */)) {
                if (this.options.ecmaVersion >= 16) {
                  var addModifiers = this.regexp_eatModifiers(state);
                  var hasHyphen = state.eat(0x2D /* - */);
                  if (addModifiers || hasHyphen) {
                    for (var i = 0; i < addModifiers.length; i++) {
                      var modifier = addModifiers.charAt(i);
                      if (addModifiers.indexOf(modifier, i + 1) > -1) {
                        state.raise("Duplicate regular expression modifiers");
                      }
                    }
                    if (hasHyphen) {
                      var removeModifiers = this.regexp_eatModifiers(state);
                      if (!addModifiers && !removeModifiers && state.current() === 0x3A /* : */) {
                        state.raise("Invalid regular expression modifiers");
                      }
                      for (var i$1 = 0; i$1 < removeModifiers.length; i$1++) {
                        var modifier$1 = removeModifiers.charAt(i$1);
                        if (
                          removeModifiers.indexOf(modifier$1, i$1 + 1) > -1 ||
                          addModifiers.indexOf(modifier$1) > -1
                        ) {
                          state.raise("Duplicate regular expression modifiers");
                        }
                      }
                    }
                  }
                }
                if (state.eat(0x3A /* : */)) {
                  this.regexp_disjunction(state);
                  if (state.eat(0x29 /* ) */)) {
                    return true
                  }
                  state.raise("Unterminated group");
                }
              }
              state.pos = start;
            }
            return false
          };
          pp$1.regexp_eatCapturingGroup = function(state) {
            if (state.eat(0x28 /* ( */)) {
              if (this.options.ecmaVersion >= 9) {
                this.regexp_groupSpecifier(state);
              } else if (state.current() === 0x3F /* ? */) {
                state.raise("Invalid group");
              }
              this.regexp_disjunction(state);
              if (state.eat(0x29 /* ) */)) {
                state.numCapturingParens += 1;
                return true
              }
              state.raise("Unterminated group");
            }
            return false
          };
          // RegularExpressionModifiers ::
          //   [empty]
          //   RegularExpressionModifiers RegularExpressionModifier
          pp$1.regexp_eatModifiers = function(state) {
            var modifiers = "";
            var ch = 0;
            while ((ch = state.current()) !== -1 && isRegularExpressionModifier(ch)) {
              modifiers += codePointToString(ch);
              state.advance();
            }
            return modifiers
          };
          // RegularExpressionModifier :: one of
          //   `i` `m` `s`
          function isRegularExpressionModifier(ch) {
            return ch === 0x69 /* i */ || ch === 0x6d /* m */ || ch === 0x73 /* s */
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-ExtendedAtom
          pp$1.regexp_eatExtendedAtom = function(state) {
            return (
              state.eat(0x2E /* . */) ||
              this.regexp_eatReverseSolidusAtomEscape(state) ||
              this.regexp_eatCharacterClass(state) ||
              this.regexp_eatUncapturingGroup(state) ||
              this.regexp_eatCapturingGroup(state) ||
              this.regexp_eatInvalidBracedQuantifier(state) ||
              this.regexp_eatExtendedPatternCharacter(state)
            )
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-InvalidBracedQuantifier
          pp$1.regexp_eatInvalidBracedQuantifier = function(state) {
            if (this.regexp_eatBracedQuantifier(state, true)) {
              state.raise("Nothing to repeat");
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-SyntaxCharacter
          pp$1.regexp_eatSyntaxCharacter = function(state) {
            var ch = state.current();
            if (isSyntaxCharacter(ch)) {
              state.lastIntValue = ch;
              state.advance();
              return true
            }
            return false
          };
          function isSyntaxCharacter(ch) {
            return (
              ch === 0x24 /* $ */ ||
              ch >= 0x28 /* ( */ && ch <= 0x2B /* + */ ||
              ch === 0x2E /* . */ ||
              ch === 0x3F /* ? */ ||
              ch >= 0x5B /* [ */ && ch <= 0x5E /* ^ */ ||
              ch >= 0x7B /* { */ && ch <= 0x7D /* } */
            )
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-PatternCharacter
          // But eat eager.
          pp$1.regexp_eatPatternCharacters = function(state) {
            var start = state.pos;
            var ch = 0;
            while ((ch = state.current()) !== -1 && !isSyntaxCharacter(ch)) {
              state.advance();
            }
            return state.pos !== start
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-ExtendedPatternCharacter
          pp$1.regexp_eatExtendedPatternCharacter = function(state) {
            var ch = state.current();
            if (
              ch !== -1 &&
              ch !== 0x24 /* $ */ &&
              !(ch >= 0x28 /* ( */ && ch <= 0x2B /* + */) &&
              ch !== 0x2E /* . */ &&
              ch !== 0x3F /* ? */ &&
              ch !== 0x5B /* [ */ &&
              ch !== 0x5E /* ^ */ &&
              ch !== 0x7C /* | */
            ) {
              state.advance();
              return true
            }
            return false
          };

          // GroupSpecifier ::
          //   [empty]
          //   `?` GroupName
          pp$1.regexp_groupSpecifier = function(state) {
            if (state.eat(0x3F /* ? */)) {
              if (!this.regexp_eatGroupName(state)) { state.raise("Invalid group"); }
              var trackDisjunction = this.options.ecmaVersion >= 16;
              var known = state.groupNames[state.lastStringValue];
              if (known) {
                if (trackDisjunction) {
                  for (var i = 0, list = known; i < list.length; i += 1) {
                    var altID = list[i];

                    if (!altID.separatedFrom(state.branchID))
                      { state.raise("Duplicate capture group name"); }
                  }
                } else {
                  state.raise("Duplicate capture group name");
                }
              }
              if (trackDisjunction) {
                (known || (state.groupNames[state.lastStringValue] = [])).push(state.branchID);
              } else {
                state.groupNames[state.lastStringValue] = true;
              }
            }
          };

          // GroupName ::
          //   `<` RegExpIdentifierName `>`
          // Note: this updates `state.lastStringValue` property with the eaten name.
          pp$1.regexp_eatGroupName = function(state) {
            state.lastStringValue = "";
            if (state.eat(0x3C /* < */)) {
              if (this.regexp_eatRegExpIdentifierName(state) && state.eat(0x3E /* > */)) {
                return true
              }
              state.raise("Invalid capture group name");
            }
            return false
          };

          // RegExpIdentifierName ::
          //   RegExpIdentifierStart
          //   RegExpIdentifierName RegExpIdentifierPart
          // Note: this updates `state.lastStringValue` property with the eaten name.
          pp$1.regexp_eatRegExpIdentifierName = function(state) {
            state.lastStringValue = "";
            if (this.regexp_eatRegExpIdentifierStart(state)) {
              state.lastStringValue += codePointToString(state.lastIntValue);
              while (this.regexp_eatRegExpIdentifierPart(state)) {
                state.lastStringValue += codePointToString(state.lastIntValue);
              }
              return true
            }
            return false
          };

          // RegExpIdentifierStart ::
          //   UnicodeIDStart
          //   `$`
          //   `_`
          //   `\` RegExpUnicodeEscapeSequence[+U]
          pp$1.regexp_eatRegExpIdentifierStart = function(state) {
            var start = state.pos;
            var forceU = this.options.ecmaVersion >= 11;
            var ch = state.current(forceU);
            state.advance(forceU);

            if (ch === 0x5C /* \ */ && this.regexp_eatRegExpUnicodeEscapeSequence(state, forceU)) {
              ch = state.lastIntValue;
            }
            if (isRegExpIdentifierStart(ch)) {
              state.lastIntValue = ch;
              return true
            }

            state.pos = start;
            return false
          };
          function isRegExpIdentifierStart(ch) {
            return isIdentifierStart(ch, true) || ch === 0x24 /* $ */ || ch === 0x5F /* _ */
          }

          // RegExpIdentifierPart ::
          //   UnicodeIDContinue
          //   `$`
          //   `_`
          //   `\` RegExpUnicodeEscapeSequence[+U]
          //   <ZWNJ>
          //   <ZWJ>
          pp$1.regexp_eatRegExpIdentifierPart = function(state) {
            var start = state.pos;
            var forceU = this.options.ecmaVersion >= 11;
            var ch = state.current(forceU);
            state.advance(forceU);

            if (ch === 0x5C /* \ */ && this.regexp_eatRegExpUnicodeEscapeSequence(state, forceU)) {
              ch = state.lastIntValue;
            }
            if (isRegExpIdentifierPart(ch)) {
              state.lastIntValue = ch;
              return true
            }

            state.pos = start;
            return false
          };
          function isRegExpIdentifierPart(ch) {
            return isIdentifierChar(ch, true) || ch === 0x24 /* $ */ || ch === 0x5F /* _ */ || ch === 0x200C /* <ZWNJ> */ || ch === 0x200D /* <ZWJ> */
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-AtomEscape
          pp$1.regexp_eatAtomEscape = function(state) {
            if (
              this.regexp_eatBackReference(state) ||
              this.regexp_eatCharacterClassEscape(state) ||
              this.regexp_eatCharacterEscape(state) ||
              (state.switchN && this.regexp_eatKGroupName(state))
            ) {
              return true
            }
            if (state.switchU) {
              // Make the same message as V8.
              if (state.current() === 0x63 /* c */) {
                state.raise("Invalid unicode escape");
              }
              state.raise("Invalid escape");
            }
            return false
          };
          pp$1.regexp_eatBackReference = function(state) {
            var start = state.pos;
            if (this.regexp_eatDecimalEscape(state)) {
              var n = state.lastIntValue;
              if (state.switchU) {
                // For SyntaxError in https://www.ecma-international.org/ecma-262/8.0/#sec-atomescape
                if (n > state.maxBackReference) {
                  state.maxBackReference = n;
                }
                return true
              }
              if (n <= state.numCapturingParens) {
                return true
              }
              state.pos = start;
            }
            return false
          };
          pp$1.regexp_eatKGroupName = function(state) {
            if (state.eat(0x6B /* k */)) {
              if (this.regexp_eatGroupName(state)) {
                state.backReferenceNames.push(state.lastStringValue);
                return true
              }
              state.raise("Invalid named reference");
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-CharacterEscape
          pp$1.regexp_eatCharacterEscape = function(state) {
            return (
              this.regexp_eatControlEscape(state) ||
              this.regexp_eatCControlLetter(state) ||
              this.regexp_eatZero(state) ||
              this.regexp_eatHexEscapeSequence(state) ||
              this.regexp_eatRegExpUnicodeEscapeSequence(state, false) ||
              (!state.switchU && this.regexp_eatLegacyOctalEscapeSequence(state)) ||
              this.regexp_eatIdentityEscape(state)
            )
          };
          pp$1.regexp_eatCControlLetter = function(state) {
            var start = state.pos;
            if (state.eat(0x63 /* c */)) {
              if (this.regexp_eatControlLetter(state)) {
                return true
              }
              state.pos = start;
            }
            return false
          };
          pp$1.regexp_eatZero = function(state) {
            if (state.current() === 0x30 /* 0 */ && !isDecimalDigit(state.lookahead())) {
              state.lastIntValue = 0;
              state.advance();
              return true
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-ControlEscape
          pp$1.regexp_eatControlEscape = function(state) {
            var ch = state.current();
            if (ch === 0x74 /* t */) {
              state.lastIntValue = 0x09; /* \t */
              state.advance();
              return true
            }
            if (ch === 0x6E /* n */) {
              state.lastIntValue = 0x0A; /* \n */
              state.advance();
              return true
            }
            if (ch === 0x76 /* v */) {
              state.lastIntValue = 0x0B; /* \v */
              state.advance();
              return true
            }
            if (ch === 0x66 /* f */) {
              state.lastIntValue = 0x0C; /* \f */
              state.advance();
              return true
            }
            if (ch === 0x72 /* r */) {
              state.lastIntValue = 0x0D; /* \r */
              state.advance();
              return true
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-ControlLetter
          pp$1.regexp_eatControlLetter = function(state) {
            var ch = state.current();
            if (isControlLetter(ch)) {
              state.lastIntValue = ch % 0x20;
              state.advance();
              return true
            }
            return false
          };
          function isControlLetter(ch) {
            return (
              (ch >= 0x41 /* A */ && ch <= 0x5A /* Z */) ||
              (ch >= 0x61 /* a */ && ch <= 0x7A /* z */)
            )
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-RegExpUnicodeEscapeSequence
          pp$1.regexp_eatRegExpUnicodeEscapeSequence = function(state, forceU) {
            if ( forceU === void 0 ) forceU = false;

            var start = state.pos;
            var switchU = forceU || state.switchU;

            if (state.eat(0x75 /* u */)) {
              if (this.regexp_eatFixedHexDigits(state, 4)) {
                var lead = state.lastIntValue;
                if (switchU && lead >= 0xD800 && lead <= 0xDBFF) {
                  var leadSurrogateEnd = state.pos;
                  if (state.eat(0x5C /* \ */) && state.eat(0x75 /* u */) && this.regexp_eatFixedHexDigits(state, 4)) {
                    var trail = state.lastIntValue;
                    if (trail >= 0xDC00 && trail <= 0xDFFF) {
                      state.lastIntValue = (lead - 0xD800) * 0x400 + (trail - 0xDC00) + 0x10000;
                      return true
                    }
                  }
                  state.pos = leadSurrogateEnd;
                  state.lastIntValue = lead;
                }
                return true
              }
              if (
                switchU &&
                state.eat(0x7B /* { */) &&
                this.regexp_eatHexDigits(state) &&
                state.eat(0x7D /* } */) &&
                isValidUnicode(state.lastIntValue)
              ) {
                return true
              }
              if (switchU) {
                state.raise("Invalid unicode escape");
              }
              state.pos = start;
            }

            return false
          };
          function isValidUnicode(ch) {
            return ch >= 0 && ch <= 0x10FFFF
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-IdentityEscape
          pp$1.regexp_eatIdentityEscape = function(state) {
            if (state.switchU) {
              if (this.regexp_eatSyntaxCharacter(state)) {
                return true
              }
              if (state.eat(0x2F /* / */)) {
                state.lastIntValue = 0x2F; /* / */
                return true
              }
              return false
            }

            var ch = state.current();
            if (ch !== 0x63 /* c */ && (!state.switchN || ch !== 0x6B /* k */)) {
              state.lastIntValue = ch;
              state.advance();
              return true
            }

            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-DecimalEscape
          pp$1.regexp_eatDecimalEscape = function(state) {
            state.lastIntValue = 0;
            var ch = state.current();
            if (ch >= 0x31 /* 1 */ && ch <= 0x39 /* 9 */) {
              do {
                state.lastIntValue = 10 * state.lastIntValue + (ch - 0x30 /* 0 */);
                state.advance();
              } while ((ch = state.current()) >= 0x30 /* 0 */ && ch <= 0x39 /* 9 */)
              return true
            }
            return false
          };

          // Return values used by character set parsing methods, needed to
          // forbid negation of sets that can match strings.
          var CharSetNone = 0; // Nothing parsed
          var CharSetOk = 1; // Construct parsed, cannot contain strings
          var CharSetString = 2; // Construct parsed, can contain strings

          // https://www.ecma-international.org/ecma-262/8.0/#prod-CharacterClassEscape
          pp$1.regexp_eatCharacterClassEscape = function(state) {
            var ch = state.current();

            if (isCharacterClassEscape(ch)) {
              state.lastIntValue = -1;
              state.advance();
              return CharSetOk
            }

            var negate = false;
            if (
              state.switchU &&
              this.options.ecmaVersion >= 9 &&
              ((negate = ch === 0x50 /* P */) || ch === 0x70 /* p */)
            ) {
              state.lastIntValue = -1;
              state.advance();
              var result;
              if (
                state.eat(0x7B /* { */) &&
                (result = this.regexp_eatUnicodePropertyValueExpression(state)) &&
                state.eat(0x7D /* } */)
              ) {
                if (negate && result === CharSetString) { state.raise("Invalid property name"); }
                return result
              }
              state.raise("Invalid property name");
            }

            return CharSetNone
          };

          function isCharacterClassEscape(ch) {
            return (
              ch === 0x64 /* d */ ||
              ch === 0x44 /* D */ ||
              ch === 0x73 /* s */ ||
              ch === 0x53 /* S */ ||
              ch === 0x77 /* w */ ||
              ch === 0x57 /* W */
            )
          }

          // UnicodePropertyValueExpression ::
          //   UnicodePropertyName `=` UnicodePropertyValue
          //   LoneUnicodePropertyNameOrValue
          pp$1.regexp_eatUnicodePropertyValueExpression = function(state) {
            var start = state.pos;

            // UnicodePropertyName `=` UnicodePropertyValue
            if (this.regexp_eatUnicodePropertyName(state) && state.eat(0x3D /* = */)) {
              var name = state.lastStringValue;
              if (this.regexp_eatUnicodePropertyValue(state)) {
                var value = state.lastStringValue;
                this.regexp_validateUnicodePropertyNameAndValue(state, name, value);
                return CharSetOk
              }
            }
            state.pos = start;

            // LoneUnicodePropertyNameOrValue
            if (this.regexp_eatLoneUnicodePropertyNameOrValue(state)) {
              var nameOrValue = state.lastStringValue;
              return this.regexp_validateUnicodePropertyNameOrValue(state, nameOrValue)
            }
            return CharSetNone
          };

          pp$1.regexp_validateUnicodePropertyNameAndValue = function(state, name, value) {
            if (!hasOwn(state.unicodeProperties.nonBinary, name))
              { state.raise("Invalid property name"); }
            if (!state.unicodeProperties.nonBinary[name].test(value))
              { state.raise("Invalid property value"); }
          };

          pp$1.regexp_validateUnicodePropertyNameOrValue = function(state, nameOrValue) {
            if (state.unicodeProperties.binary.test(nameOrValue)) { return CharSetOk }
            if (state.switchV && state.unicodeProperties.binaryOfStrings.test(nameOrValue)) { return CharSetString }
            state.raise("Invalid property name");
          };

          // UnicodePropertyName ::
          //   UnicodePropertyNameCharacters
          pp$1.regexp_eatUnicodePropertyName = function(state) {
            var ch = 0;
            state.lastStringValue = "";
            while (isUnicodePropertyNameCharacter(ch = state.current())) {
              state.lastStringValue += codePointToString(ch);
              state.advance();
            }
            return state.lastStringValue !== ""
          };

          function isUnicodePropertyNameCharacter(ch) {
            return isControlLetter(ch) || ch === 0x5F /* _ */
          }

          // UnicodePropertyValue ::
          //   UnicodePropertyValueCharacters
          pp$1.regexp_eatUnicodePropertyValue = function(state) {
            var ch = 0;
            state.lastStringValue = "";
            while (isUnicodePropertyValueCharacter(ch = state.current())) {
              state.lastStringValue += codePointToString(ch);
              state.advance();
            }
            return state.lastStringValue !== ""
          };
          function isUnicodePropertyValueCharacter(ch) {
            return isUnicodePropertyNameCharacter(ch) || isDecimalDigit(ch)
          }

          // LoneUnicodePropertyNameOrValue ::
          //   UnicodePropertyValueCharacters
          pp$1.regexp_eatLoneUnicodePropertyNameOrValue = function(state) {
            return this.regexp_eatUnicodePropertyValue(state)
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-CharacterClass
          pp$1.regexp_eatCharacterClass = function(state) {
            if (state.eat(0x5B /* [ */)) {
              var negate = state.eat(0x5E /* ^ */);
              var result = this.regexp_classContents(state);
              if (!state.eat(0x5D /* ] */))
                { state.raise("Unterminated character class"); }
              if (negate && result === CharSetString)
                { state.raise("Negated character class may contain strings"); }
              return true
            }
            return false
          };

          // https://tc39.es/ecma262/#prod-ClassContents
          // https://www.ecma-international.org/ecma-262/8.0/#prod-ClassRanges
          pp$1.regexp_classContents = function(state) {
            if (state.current() === 0x5D /* ] */) { return CharSetOk }
            if (state.switchV) { return this.regexp_classSetExpression(state) }
            this.regexp_nonEmptyClassRanges(state);
            return CharSetOk
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-NonemptyClassRanges
          // https://www.ecma-international.org/ecma-262/8.0/#prod-NonemptyClassRangesNoDash
          pp$1.regexp_nonEmptyClassRanges = function(state) {
            while (this.regexp_eatClassAtom(state)) {
              var left = state.lastIntValue;
              if (state.eat(0x2D /* - */) && this.regexp_eatClassAtom(state)) {
                var right = state.lastIntValue;
                if (state.switchU && (left === -1 || right === -1)) {
                  state.raise("Invalid character class");
                }
                if (left !== -1 && right !== -1 && left > right) {
                  state.raise("Range out of order in character class");
                }
              }
            }
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-ClassAtom
          // https://www.ecma-international.org/ecma-262/8.0/#prod-ClassAtomNoDash
          pp$1.regexp_eatClassAtom = function(state) {
            var start = state.pos;

            if (state.eat(0x5C /* \ */)) {
              if (this.regexp_eatClassEscape(state)) {
                return true
              }
              if (state.switchU) {
                // Make the same message as V8.
                var ch$1 = state.current();
                if (ch$1 === 0x63 /* c */ || isOctalDigit(ch$1)) {
                  state.raise("Invalid class escape");
                }
                state.raise("Invalid escape");
              }
              state.pos = start;
            }

            var ch = state.current();
            if (ch !== 0x5D /* ] */) {
              state.lastIntValue = ch;
              state.advance();
              return true
            }

            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-ClassEscape
          pp$1.regexp_eatClassEscape = function(state) {
            var start = state.pos;

            if (state.eat(0x62 /* b */)) {
              state.lastIntValue = 0x08; /* <BS> */
              return true
            }

            if (state.switchU && state.eat(0x2D /* - */)) {
              state.lastIntValue = 0x2D; /* - */
              return true
            }

            if (!state.switchU && state.eat(0x63 /* c */)) {
              if (this.regexp_eatClassControlLetter(state)) {
                return true
              }
              state.pos = start;
            }

            return (
              this.regexp_eatCharacterClassEscape(state) ||
              this.regexp_eatCharacterEscape(state)
            )
          };

          // https://tc39.es/ecma262/#prod-ClassSetExpression
          // https://tc39.es/ecma262/#prod-ClassUnion
          // https://tc39.es/ecma262/#prod-ClassIntersection
          // https://tc39.es/ecma262/#prod-ClassSubtraction
          pp$1.regexp_classSetExpression = function(state) {
            var result = CharSetOk, subResult;
            if (this.regexp_eatClassSetRange(state)) ; else if (subResult = this.regexp_eatClassSetOperand(state)) {
              if (subResult === CharSetString) { result = CharSetString; }
              // https://tc39.es/ecma262/#prod-ClassIntersection
              var start = state.pos;
              while (state.eatChars([0x26, 0x26] /* && */)) {
                if (
                  state.current() !== 0x26 /* & */ &&
                  (subResult = this.regexp_eatClassSetOperand(state))
                ) {
                  if (subResult !== CharSetString) { result = CharSetOk; }
                  continue
                }
                state.raise("Invalid character in character class");
              }
              if (start !== state.pos) { return result }
              // https://tc39.es/ecma262/#prod-ClassSubtraction
              while (state.eatChars([0x2D, 0x2D] /* -- */)) {
                if (this.regexp_eatClassSetOperand(state)) { continue }
                state.raise("Invalid character in character class");
              }
              if (start !== state.pos) { return result }
            } else {
              state.raise("Invalid character in character class");
            }
            // https://tc39.es/ecma262/#prod-ClassUnion
            for (;;) {
              if (this.regexp_eatClassSetRange(state)) { continue }
              subResult = this.regexp_eatClassSetOperand(state);
              if (!subResult) { return result }
              if (subResult === CharSetString) { result = CharSetString; }
            }
          };

          // https://tc39.es/ecma262/#prod-ClassSetRange
          pp$1.regexp_eatClassSetRange = function(state) {
            var start = state.pos;
            if (this.regexp_eatClassSetCharacter(state)) {
              var left = state.lastIntValue;
              if (state.eat(0x2D /* - */) && this.regexp_eatClassSetCharacter(state)) {
                var right = state.lastIntValue;
                if (left !== -1 && right !== -1 && left > right) {
                  state.raise("Range out of order in character class");
                }
                return true
              }
              state.pos = start;
            }
            return false
          };

          // https://tc39.es/ecma262/#prod-ClassSetOperand
          pp$1.regexp_eatClassSetOperand = function(state) {
            if (this.regexp_eatClassSetCharacter(state)) { return CharSetOk }
            return this.regexp_eatClassStringDisjunction(state) || this.regexp_eatNestedClass(state)
          };

          // https://tc39.es/ecma262/#prod-NestedClass
          pp$1.regexp_eatNestedClass = function(state) {
            var start = state.pos;
            if (state.eat(0x5B /* [ */)) {
              var negate = state.eat(0x5E /* ^ */);
              var result = this.regexp_classContents(state);
              if (state.eat(0x5D /* ] */)) {
                if (negate && result === CharSetString) {
                  state.raise("Negated character class may contain strings");
                }
                return result
              }
              state.pos = start;
            }
            if (state.eat(0x5C /* \ */)) {
              var result$1 = this.regexp_eatCharacterClassEscape(state);
              if (result$1) {
                return result$1
              }
              state.pos = start;
            }
            return null
          };

          // https://tc39.es/ecma262/#prod-ClassStringDisjunction
          pp$1.regexp_eatClassStringDisjunction = function(state) {
            var start = state.pos;
            if (state.eatChars([0x5C, 0x71] /* \q */)) {
              if (state.eat(0x7B /* { */)) {
                var result = this.regexp_classStringDisjunctionContents(state);
                if (state.eat(0x7D /* } */)) {
                  return result
                }
              } else {
                // Make the same message as V8.
                state.raise("Invalid escape");
              }
              state.pos = start;
            }
            return null
          };

          // https://tc39.es/ecma262/#prod-ClassStringDisjunctionContents
          pp$1.regexp_classStringDisjunctionContents = function(state) {
            var result = this.regexp_classString(state);
            while (state.eat(0x7C /* | */)) {
              if (this.regexp_classString(state) === CharSetString) { result = CharSetString; }
            }
            return result
          };

          // https://tc39.es/ecma262/#prod-ClassString
          // https://tc39.es/ecma262/#prod-NonEmptyClassString
          pp$1.regexp_classString = function(state) {
            var count = 0;
            while (this.regexp_eatClassSetCharacter(state)) { count++; }
            return count === 1 ? CharSetOk : CharSetString
          };

          // https://tc39.es/ecma262/#prod-ClassSetCharacter
          pp$1.regexp_eatClassSetCharacter = function(state) {
            var start = state.pos;
            if (state.eat(0x5C /* \ */)) {
              if (
                this.regexp_eatCharacterEscape(state) ||
                this.regexp_eatClassSetReservedPunctuator(state)
              ) {
                return true
              }
              if (state.eat(0x62 /* b */)) {
                state.lastIntValue = 0x08; /* <BS> */
                return true
              }
              state.pos = start;
              return false
            }
            var ch = state.current();
            if (ch < 0 || ch === state.lookahead() && isClassSetReservedDoublePunctuatorCharacter(ch)) { return false }
            if (isClassSetSyntaxCharacter(ch)) { return false }
            state.advance();
            state.lastIntValue = ch;
            return true
          };

          // https://tc39.es/ecma262/#prod-ClassSetReservedDoublePunctuator
          function isClassSetReservedDoublePunctuatorCharacter(ch) {
            return (
              ch === 0x21 /* ! */ ||
              ch >= 0x23 /* # */ && ch <= 0x26 /* & */ ||
              ch >= 0x2A /* * */ && ch <= 0x2C /* , */ ||
              ch === 0x2E /* . */ ||
              ch >= 0x3A /* : */ && ch <= 0x40 /* @ */ ||
              ch === 0x5E /* ^ */ ||
              ch === 0x60 /* ` */ ||
              ch === 0x7E /* ~ */
            )
          }

          // https://tc39.es/ecma262/#prod-ClassSetSyntaxCharacter
          function isClassSetSyntaxCharacter(ch) {
            return (
              ch === 0x28 /* ( */ ||
              ch === 0x29 /* ) */ ||
              ch === 0x2D /* - */ ||
              ch === 0x2F /* / */ ||
              ch >= 0x5B /* [ */ && ch <= 0x5D /* ] */ ||
              ch >= 0x7B /* { */ && ch <= 0x7D /* } */
            )
          }

          // https://tc39.es/ecma262/#prod-ClassSetReservedPunctuator
          pp$1.regexp_eatClassSetReservedPunctuator = function(state) {
            var ch = state.current();
            if (isClassSetReservedPunctuator(ch)) {
              state.lastIntValue = ch;
              state.advance();
              return true
            }
            return false
          };

          // https://tc39.es/ecma262/#prod-ClassSetReservedPunctuator
          function isClassSetReservedPunctuator(ch) {
            return (
              ch === 0x21 /* ! */ ||
              ch === 0x23 /* # */ ||
              ch === 0x25 /* % */ ||
              ch === 0x26 /* & */ ||
              ch === 0x2C /* , */ ||
              ch === 0x2D /* - */ ||
              ch >= 0x3A /* : */ && ch <= 0x3E /* > */ ||
              ch === 0x40 /* @ */ ||
              ch === 0x60 /* ` */ ||
              ch === 0x7E /* ~ */
            )
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-ClassControlLetter
          pp$1.regexp_eatClassControlLetter = function(state) {
            var ch = state.current();
            if (isDecimalDigit(ch) || ch === 0x5F /* _ */) {
              state.lastIntValue = ch % 0x20;
              state.advance();
              return true
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-HexEscapeSequence
          pp$1.regexp_eatHexEscapeSequence = function(state) {
            var start = state.pos;
            if (state.eat(0x78 /* x */)) {
              if (this.regexp_eatFixedHexDigits(state, 2)) {
                return true
              }
              if (state.switchU) {
                state.raise("Invalid escape");
              }
              state.pos = start;
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-DecimalDigits
          pp$1.regexp_eatDecimalDigits = function(state) {
            var start = state.pos;
            var ch = 0;
            state.lastIntValue = 0;
            while (isDecimalDigit(ch = state.current())) {
              state.lastIntValue = 10 * state.lastIntValue + (ch - 0x30 /* 0 */);
              state.advance();
            }
            return state.pos !== start
          };
          function isDecimalDigit(ch) {
            return ch >= 0x30 /* 0 */ && ch <= 0x39 /* 9 */
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-HexDigits
          pp$1.regexp_eatHexDigits = function(state) {
            var start = state.pos;
            var ch = 0;
            state.lastIntValue = 0;
            while (isHexDigit(ch = state.current())) {
              state.lastIntValue = 16 * state.lastIntValue + hexToInt(ch);
              state.advance();
            }
            return state.pos !== start
          };
          function isHexDigit(ch) {
            return (
              (ch >= 0x30 /* 0 */ && ch <= 0x39 /* 9 */) ||
              (ch >= 0x41 /* A */ && ch <= 0x46 /* F */) ||
              (ch >= 0x61 /* a */ && ch <= 0x66 /* f */)
            )
          }
          function hexToInt(ch) {
            if (ch >= 0x41 /* A */ && ch <= 0x46 /* F */) {
              return 10 + (ch - 0x41 /* A */)
            }
            if (ch >= 0x61 /* a */ && ch <= 0x66 /* f */) {
              return 10 + (ch - 0x61 /* a */)
            }
            return ch - 0x30 /* 0 */
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-annexB-LegacyOctalEscapeSequence
          // Allows only 0-377(octal) i.e. 0-255(decimal).
          pp$1.regexp_eatLegacyOctalEscapeSequence = function(state) {
            if (this.regexp_eatOctalDigit(state)) {
              var n1 = state.lastIntValue;
              if (this.regexp_eatOctalDigit(state)) {
                var n2 = state.lastIntValue;
                if (n1 <= 3 && this.regexp_eatOctalDigit(state)) {
                  state.lastIntValue = n1 * 64 + n2 * 8 + state.lastIntValue;
                } else {
                  state.lastIntValue = n1 * 8 + n2;
                }
              } else {
                state.lastIntValue = n1;
              }
              return true
            }
            return false
          };

          // https://www.ecma-international.org/ecma-262/8.0/#prod-OctalDigit
          pp$1.regexp_eatOctalDigit = function(state) {
            var ch = state.current();
            if (isOctalDigit(ch)) {
              state.lastIntValue = ch - 0x30; /* 0 */
              state.advance();
              return true
            }
            state.lastIntValue = 0;
            return false
          };
          function isOctalDigit(ch) {
            return ch >= 0x30 /* 0 */ && ch <= 0x37 /* 7 */
          }

          // https://www.ecma-international.org/ecma-262/8.0/#prod-Hex4Digits
          // https://www.ecma-international.org/ecma-262/8.0/#prod-HexDigit
          // And HexDigit HexDigit in https://www.ecma-international.org/ecma-262/8.0/#prod-HexEscapeSequence
          pp$1.regexp_eatFixedHexDigits = function(state, length) {
            var start = state.pos;
            state.lastIntValue = 0;
            for (var i = 0; i < length; ++i) {
              var ch = state.current();
              if (!isHexDigit(ch)) {
                state.pos = start;
                return false
              }
              state.lastIntValue = 16 * state.lastIntValue + hexToInt(ch);
              state.advance();
            }
            return true
          };

          // Object type used to represent tokens. Note that normally, tokens
          // simply exist as properties on the parser object. This is only
          // used for the onToken callback and the external tokenizer.

          var Token = function Token(p) {
            this.type = p.type;
            this.value = p.value;
            this.start = p.start;
            this.end = p.end;
            if (p.options.locations)
              { this.loc = new SourceLocation(p, p.startLoc, p.endLoc); }
            if (p.options.ranges)
              { this.range = [p.start, p.end]; }
          };

          // ## Tokenizer

          var pp = Parser.prototype;

          // Move to the next token

          pp.next = function(ignoreEscapeSequenceInKeyword) {
            if (!ignoreEscapeSequenceInKeyword && this.type.keyword && this.containsEsc)
              { this.raiseRecoverable(this.start, "Escape sequence in keyword " + this.type.keyword); }
            if (this.options.onToken)
              { this.options.onToken(new Token(this)); }

            this.lastTokEnd = this.end;
            this.lastTokStart = this.start;
            this.lastTokEndLoc = this.endLoc;
            this.lastTokStartLoc = this.startLoc;
            this.nextToken();
          };

          pp.getToken = function() {
            this.next();
            return new Token(this)
          };

          // If we're in an ES6 environment, make parsers iterable
          if (typeof Symbol !== "undefined")
            { pp[Symbol.iterator] = function() {
              var this$1$1 = this;

              return {
                next: function () {
                  var token = this$1$1.getToken();
                  return {
                    done: token.type === types$1.eof,
                    value: token
                  }
                }
              }
            }; }

          // Toggle strict mode. Re-reads the next number or string to please
          // pedantic tests (`"use strict"; 010;` should fail).

          // Read a single token, updating the parser object's token-related
          // properties.

          pp.nextToken = function() {
            var curContext = this.curContext();
            if (!curContext || !curContext.preserveSpace) { this.skipSpace(); }

            this.start = this.pos;
            if (this.options.locations) { this.startLoc = this.curPosition(); }
            if (this.pos >= this.input.length) { return this.finishToken(types$1.eof) }

            if (curContext.override) { return curContext.override(this) }
            else { this.readToken(this.fullCharCodeAtPos()); }
          };

          pp.readToken = function(code) {
            // Identifier or keyword. '\uXXXX' sequences are allowed in
            // identifiers, so '\' also dispatches to that.
            if (isIdentifierStart(code, this.options.ecmaVersion >= 6) || code === 92 /* '\' */)
              { return this.readWord() }

            return this.getTokenFromCode(code)
          };

          pp.fullCharCodeAtPos = function() {
            var code = this.input.charCodeAt(this.pos);
            if (code <= 0xd7ff || code >= 0xdc00) { return code }
            var next = this.input.charCodeAt(this.pos + 1);
            return next <= 0xdbff || next >= 0xe000 ? code : (code << 10) + next - 0x35fdc00
          };

          pp.skipBlockComment = function() {
            var startLoc = this.options.onComment && this.curPosition();
            var start = this.pos, end = this.input.indexOf("*/", this.pos += 2);
            if (end === -1) { this.raise(this.pos - 2, "Unterminated comment"); }
            this.pos = end + 2;
            if (this.options.locations) {
              for (var nextBreak = (void 0), pos = start; (nextBreak = nextLineBreak(this.input, pos, this.pos)) > -1;) {
                ++this.curLine;
                pos = this.lineStart = nextBreak;
              }
            }
            if (this.options.onComment)
              { this.options.onComment(true, this.input.slice(start + 2, end), start, this.pos,
                                     startLoc, this.curPosition()); }
          };

          pp.skipLineComment = function(startSkip) {
            var start = this.pos;
            var startLoc = this.options.onComment && this.curPosition();
            var ch = this.input.charCodeAt(this.pos += startSkip);
            while (this.pos < this.input.length && !isNewLine(ch)) {
              ch = this.input.charCodeAt(++this.pos);
            }
            if (this.options.onComment)
              { this.options.onComment(false, this.input.slice(start + startSkip, this.pos), start, this.pos,
                                     startLoc, this.curPosition()); }
          };

          // Called at the start of the parse and after every token. Skips
          // whitespace and comments, and.

          pp.skipSpace = function() {
            loop: while (this.pos < this.input.length) {
              var ch = this.input.charCodeAt(this.pos);
              switch (ch) {
              case 32: case 160: // ' '
                ++this.pos;
                break
              case 13:
                if (this.input.charCodeAt(this.pos + 1) === 10) {
                  ++this.pos;
                }
              case 10: case 8232: case 8233:
                ++this.pos;
                if (this.options.locations) {
                  ++this.curLine;
                  this.lineStart = this.pos;
                }
                break
              case 47: // '/'
                switch (this.input.charCodeAt(this.pos + 1)) {
                case 42: // '*'
                  this.skipBlockComment();
                  break
                case 47:
                  this.skipLineComment(2);
                  break
                default:
                  break loop
                }
                break
              default:
                if (ch > 8 && ch < 14 || ch >= 5760 && nonASCIIwhitespace.test(String.fromCharCode(ch))) {
                  ++this.pos;
                } else {
                  break loop
                }
              }
            }
          };

          // Called at the end of every token. Sets `end`, `val`, and
          // maintains `context` and `exprAllowed`, and skips the space after
          // the token, so that the next one's `start` will point at the
          // right position.

          pp.finishToken = function(type, val) {
            this.end = this.pos;
            if (this.options.locations) { this.endLoc = this.curPosition(); }
            var prevType = this.type;
            this.type = type;
            this.value = val;

            this.updateContext(prevType);
          };

          // ### Token reading

          // This is the function that is called to fetch the next token. It
          // is somewhat obscure, because it works in character codes rather
          // than characters, and because operator parsing has been inlined
          // into it.
          //
          // All in the name of speed.
          //
          pp.readToken_dot = function() {
            var next = this.input.charCodeAt(this.pos + 1);
            if (next >= 48 && next <= 57) { return this.readNumber(true) }
            var next2 = this.input.charCodeAt(this.pos + 2);
            if (this.options.ecmaVersion >= 6 && next === 46 && next2 === 46) { // 46 = dot '.'
              this.pos += 3;
              return this.finishToken(types$1.ellipsis)
            } else {
              ++this.pos;
              return this.finishToken(types$1.dot)
            }
          };

          pp.readToken_slash = function() { // '/'
            var next = this.input.charCodeAt(this.pos + 1);
            if (this.exprAllowed) { ++this.pos; return this.readRegexp() }
            if (next === 61) { return this.finishOp(types$1.assign, 2) }
            return this.finishOp(types$1.slash, 1)
          };

          pp.readToken_mult_modulo_exp = function(code) { // '%*'
            var next = this.input.charCodeAt(this.pos + 1);
            var size = 1;
            var tokentype = code === 42 ? types$1.star : types$1.modulo;

            // exponentiation operator ** and **=
            if (this.options.ecmaVersion >= 7 && code === 42 && next === 42) {
              ++size;
              tokentype = types$1.starstar;
              next = this.input.charCodeAt(this.pos + 2);
            }

            if (next === 61) { return this.finishOp(types$1.assign, size + 1) }
            return this.finishOp(tokentype, size)
          };

          pp.readToken_pipe_amp = function(code) { // '|&'
            var next = this.input.charCodeAt(this.pos + 1);
            if (next === code) {
              if (this.options.ecmaVersion >= 12) {
                var next2 = this.input.charCodeAt(this.pos + 2);
                if (next2 === 61) { return this.finishOp(types$1.assign, 3) }
              }
              return this.finishOp(code === 124 ? types$1.logicalOR : types$1.logicalAND, 2)
            }
            if (next === 61) { return this.finishOp(types$1.assign, 2) }
            return this.finishOp(code === 124 ? types$1.bitwiseOR : types$1.bitwiseAND, 1)
          };

          pp.readToken_caret = function() { // '^'
            var next = this.input.charCodeAt(this.pos + 1);
            if (next === 61) { return this.finishOp(types$1.assign, 2) }
            return this.finishOp(types$1.bitwiseXOR, 1)
          };

          pp.readToken_plus_min = function(code) { // '+-'
            var next = this.input.charCodeAt(this.pos + 1);
            if (next === code) {
              if (next === 45 && !this.inModule && this.input.charCodeAt(this.pos + 2) === 62 &&
                  (this.lastTokEnd === 0 || lineBreak.test(this.input.slice(this.lastTokEnd, this.pos)))) {
                // A `-->` line comment
                this.skipLineComment(3);
                this.skipSpace();
                return this.nextToken()
              }
              return this.finishOp(types$1.incDec, 2)
            }
            if (next === 61) { return this.finishOp(types$1.assign, 2) }
            return this.finishOp(types$1.plusMin, 1)
          };

          pp.readToken_lt_gt = function(code) { // '<>'
            var next = this.input.charCodeAt(this.pos + 1);
            var size = 1;
            if (next === code) {
              size = code === 62 && this.input.charCodeAt(this.pos + 2) === 62 ? 3 : 2;
              if (this.input.charCodeAt(this.pos + size) === 61) { return this.finishOp(types$1.assign, size + 1) }
              return this.finishOp(types$1.bitShift, size)
            }
            if (next === 33 && code === 60 && !this.inModule && this.input.charCodeAt(this.pos + 2) === 45 &&
                this.input.charCodeAt(this.pos + 3) === 45) {
              // `<!--`, an XML-style comment that should be interpreted as a line comment
              this.skipLineComment(4);
              this.skipSpace();
              return this.nextToken()
            }
            if (next === 61) { size = 2; }
            return this.finishOp(types$1.relational, size)
          };

          pp.readToken_eq_excl = function(code) { // '=!'
            var next = this.input.charCodeAt(this.pos + 1);
            if (next === 61) { return this.finishOp(types$1.equality, this.input.charCodeAt(this.pos + 2) === 61 ? 3 : 2) }
            if (code === 61 && next === 62 && this.options.ecmaVersion >= 6) { // '=>'
              this.pos += 2;
              return this.finishToken(types$1.arrow)
            }
            return this.finishOp(code === 61 ? types$1.eq : types$1.prefix, 1)
          };

          pp.readToken_question = function() { // '?'
            var ecmaVersion = this.options.ecmaVersion;
            if (ecmaVersion >= 11) {
              var next = this.input.charCodeAt(this.pos + 1);
              if (next === 46) {
                var next2 = this.input.charCodeAt(this.pos + 2);
                if (next2 < 48 || next2 > 57) { return this.finishOp(types$1.questionDot, 2) }
              }
              if (next === 63) {
                if (ecmaVersion >= 12) {
                  var next2$1 = this.input.charCodeAt(this.pos + 2);
                  if (next2$1 === 61) { return this.finishOp(types$1.assign, 3) }
                }
                return this.finishOp(types$1.coalesce, 2)
              }
            }
            return this.finishOp(types$1.question, 1)
          };

          pp.readToken_numberSign = function() { // '#'
            var ecmaVersion = this.options.ecmaVersion;
            var code = 35; // '#'
            if (ecmaVersion >= 13) {
              ++this.pos;
              code = this.fullCharCodeAtPos();
              if (isIdentifierStart(code, true) || code === 92 /* '\' */) {
                return this.finishToken(types$1.privateId, this.readWord1())
              }
            }

            this.raise(this.pos, "Unexpected character '" + codePointToString(code) + "'");
          };

          pp.getTokenFromCode = function(code) {
            switch (code) {
            // The interpretation of a dot depends on whether it is followed
            // by a digit or another two dots.
            case 46: // '.'
              return this.readToken_dot()

            // Punctuation tokens.
            case 40: ++this.pos; return this.finishToken(types$1.parenL)
            case 41: ++this.pos; return this.finishToken(types$1.parenR)
            case 59: ++this.pos; return this.finishToken(types$1.semi)
            case 44: ++this.pos; return this.finishToken(types$1.comma)
            case 91: ++this.pos; return this.finishToken(types$1.bracketL)
            case 93: ++this.pos; return this.finishToken(types$1.bracketR)
            case 123: ++this.pos; return this.finishToken(types$1.braceL)
            case 125: ++this.pos; return this.finishToken(types$1.braceR)
            case 58: ++this.pos; return this.finishToken(types$1.colon)

            case 96: // '`'
              if (this.options.ecmaVersion < 6) { break }
              ++this.pos;
              return this.finishToken(types$1.backQuote)

            case 48: // '0'
              var next = this.input.charCodeAt(this.pos + 1);
              if (next === 120 || next === 88) { return this.readRadixNumber(16) } // '0x', '0X' - hex number
              if (this.options.ecmaVersion >= 6) {
                if (next === 111 || next === 79) { return this.readRadixNumber(8) } // '0o', '0O' - octal number
                if (next === 98 || next === 66) { return this.readRadixNumber(2) } // '0b', '0B' - binary number
              }

            // Anything else beginning with a digit is an integer, octal
            // number, or float.
            case 49: case 50: case 51: case 52: case 53: case 54: case 55: case 56: case 57: // 1-9
              return this.readNumber(false)

            // Quotes produce strings.
            case 34: case 39: // '"', "'"
              return this.readString(code)

            // Operators are parsed inline in tiny state machines. '=' (61) is
            // often referred to. `finishOp` simply skips the amount of
            // characters it is given as second argument, and returns a token
            // of the type given by its first argument.
            case 47: // '/'
              return this.readToken_slash()

            case 37: case 42: // '%*'
              return this.readToken_mult_modulo_exp(code)

            case 124: case 38: // '|&'
              return this.readToken_pipe_amp(code)

            case 94: // '^'
              return this.readToken_caret()

            case 43: case 45: // '+-'
              return this.readToken_plus_min(code)

            case 60: case 62: // '<>'
              return this.readToken_lt_gt(code)

            case 61: case 33: // '=!'
              return this.readToken_eq_excl(code)

            case 63: // '?'
              return this.readToken_question()

            case 126: // '~'
              return this.finishOp(types$1.prefix, 1)

            case 35: // '#'
              return this.readToken_numberSign()
            }

            this.raise(this.pos, "Unexpected character '" + codePointToString(code) + "'");
          };

          pp.finishOp = function(type, size) {
            var str = this.input.slice(this.pos, this.pos + size);
            this.pos += size;
            return this.finishToken(type, str)
          };

          pp.readRegexp = function() {
            var escaped, inClass, start = this.pos;
            for (;;) {
              if (this.pos >= this.input.length) { this.raise(start, "Unterminated regular expression"); }
              var ch = this.input.charAt(this.pos);
              if (lineBreak.test(ch)) { this.raise(start, "Unterminated regular expression"); }
              if (!escaped) {
                if (ch === "[") { inClass = true; }
                else if (ch === "]" && inClass) { inClass = false; }
                else if (ch === "/" && !inClass) { break }
                escaped = ch === "\\";
              } else { escaped = false; }
              ++this.pos;
            }
            var pattern = this.input.slice(start, this.pos);
            ++this.pos;
            var flagsStart = this.pos;
            var flags = this.readWord1();
            if (this.containsEsc) { this.unexpected(flagsStart); }

            // Validate pattern
            var state = this.regexpState || (this.regexpState = new RegExpValidationState(this));
            state.reset(start, pattern, flags);
            this.validateRegExpFlags(state);
            this.validateRegExpPattern(state);

            // Create Literal#value property value.
            var value = null;
            try {
              value = new RegExp(pattern, flags);
            } catch (e) {
              // ESTree requires null if it failed to instantiate RegExp object.
              // https://github.com/estree/estree/blob/a27003adf4fd7bfad44de9cef372a2eacd527b1c/es5.md#regexpliteral
            }

            return this.finishToken(types$1.regexp, {pattern: pattern, flags: flags, value: value})
          };

          // Read an integer in the given radix. Return null if zero digits
          // were read, the integer value otherwise. When `len` is given, this
          // will return `null` unless the integer has exactly `len` digits.

          pp.readInt = function(radix, len, maybeLegacyOctalNumericLiteral) {
            // `len` is used for character escape sequences. In that case, disallow separators.
            var allowSeparators = this.options.ecmaVersion >= 12 && len === undefined;

            // `maybeLegacyOctalNumericLiteral` is true if it doesn't have prefix (0x,0o,0b)
            // and isn't fraction part nor exponent part. In that case, if the first digit
            // is zero then disallow separators.
            var isLegacyOctalNumericLiteral = maybeLegacyOctalNumericLiteral && this.input.charCodeAt(this.pos) === 48;

            var start = this.pos, total = 0, lastCode = 0;
            for (var i = 0, e = len == null ? Infinity : len; i < e; ++i, ++this.pos) {
              var code = this.input.charCodeAt(this.pos), val = (void 0);

              if (allowSeparators && code === 95) {
                if (isLegacyOctalNumericLiteral) { this.raiseRecoverable(this.pos, "Numeric separator is not allowed in legacy octal numeric literals"); }
                if (lastCode === 95) { this.raiseRecoverable(this.pos, "Numeric separator must be exactly one underscore"); }
                if (i === 0) { this.raiseRecoverable(this.pos, "Numeric separator is not allowed at the first of digits"); }
                lastCode = code;
                continue
              }

              if (code >= 97) { val = code - 97 + 10; } // a
              else if (code >= 65) { val = code - 65 + 10; } // A
              else if (code >= 48 && code <= 57) { val = code - 48; } // 0-9
              else { val = Infinity; }
              if (val >= radix) { break }
              lastCode = code;
              total = total * radix + val;
            }

            if (allowSeparators && lastCode === 95) { this.raiseRecoverable(this.pos - 1, "Numeric separator is not allowed at the last of digits"); }
            if (this.pos === start || len != null && this.pos - start !== len) { return null }

            return total
          };

          function stringToNumber(str, isLegacyOctalNumericLiteral) {
            if (isLegacyOctalNumericLiteral) {
              return parseInt(str, 8)
            }

            // `parseFloat(value)` stops parsing at the first numeric separator then returns a wrong value.
            return parseFloat(str.replace(/_/g, ""))
          }

          function stringToBigInt(str) {
            if (typeof BigInt !== "function") {
              return null
            }

            // `BigInt(value)` throws syntax error if the string contains numeric separators.
            return BigInt(str.replace(/_/g, ""))
          }

          pp.readRadixNumber = function(radix) {
            var start = this.pos;
            this.pos += 2; // 0x
            var val = this.readInt(radix);
            if (val == null) { this.raise(this.start + 2, "Expected number in radix " + radix); }
            if (this.options.ecmaVersion >= 11 && this.input.charCodeAt(this.pos) === 110) {
              val = stringToBigInt(this.input.slice(start, this.pos));
              ++this.pos;
            } else if (isIdentifierStart(this.fullCharCodeAtPos())) { this.raise(this.pos, "Identifier directly after number"); }
            return this.finishToken(types$1.num, val)
          };

          // Read an integer, octal integer, or floating-point number.

          pp.readNumber = function(startsWithDot) {
            var start = this.pos;
            if (!startsWithDot && this.readInt(10, undefined, true) === null) { this.raise(start, "Invalid number"); }
            var octal = this.pos - start >= 2 && this.input.charCodeAt(start) === 48;
            if (octal && this.strict) { this.raise(start, "Invalid number"); }
            var next = this.input.charCodeAt(this.pos);
            if (!octal && !startsWithDot && this.options.ecmaVersion >= 11 && next === 110) {
              var val$1 = stringToBigInt(this.input.slice(start, this.pos));
              ++this.pos;
              if (isIdentifierStart(this.fullCharCodeAtPos())) { this.raise(this.pos, "Identifier directly after number"); }
              return this.finishToken(types$1.num, val$1)
            }
            if (octal && /[89]/.test(this.input.slice(start, this.pos))) { octal = false; }
            if (next === 46 && !octal) { // '.'
              ++this.pos;
              this.readInt(10);
              next = this.input.charCodeAt(this.pos);
            }
            if ((next === 69 || next === 101) && !octal) { // 'eE'
              next = this.input.charCodeAt(++this.pos);
              if (next === 43 || next === 45) { ++this.pos; } // '+-'
              if (this.readInt(10) === null) { this.raise(start, "Invalid number"); }
            }
            if (isIdentifierStart(this.fullCharCodeAtPos())) { this.raise(this.pos, "Identifier directly after number"); }

            var val = stringToNumber(this.input.slice(start, this.pos), octal);
            return this.finishToken(types$1.num, val)
          };

          // Read a string value, interpreting backslash-escapes.

          pp.readCodePoint = function() {
            var ch = this.input.charCodeAt(this.pos), code;

            if (ch === 123) { // '{'
              if (this.options.ecmaVersion < 6) { this.unexpected(); }
              var codePos = ++this.pos;
              code = this.readHexChar(this.input.indexOf("}", this.pos) - this.pos);
              ++this.pos;
              if (code > 0x10FFFF) { this.invalidStringToken(codePos, "Code point out of bounds"); }
            } else {
              code = this.readHexChar(4);
            }
            return code
          };

          pp.readString = function(quote) {
            var out = "", chunkStart = ++this.pos;
            for (;;) {
              if (this.pos >= this.input.length) { this.raise(this.start, "Unterminated string constant"); }
              var ch = this.input.charCodeAt(this.pos);
              if (ch === quote) { break }
              if (ch === 92) { // '\'
                out += this.input.slice(chunkStart, this.pos);
                out += this.readEscapedChar(false);
                chunkStart = this.pos;
              } else if (ch === 0x2028 || ch === 0x2029) {
                if (this.options.ecmaVersion < 10) { this.raise(this.start, "Unterminated string constant"); }
                ++this.pos;
                if (this.options.locations) {
                  this.curLine++;
                  this.lineStart = this.pos;
                }
              } else {
                if (isNewLine(ch)) { this.raise(this.start, "Unterminated string constant"); }
                ++this.pos;
              }
            }
            out += this.input.slice(chunkStart, this.pos++);
            return this.finishToken(types$1.string, out)
          };

          // Reads template string tokens.

          var INVALID_TEMPLATE_ESCAPE_ERROR = {};

          pp.tryReadTemplateToken = function() {
            this.inTemplateElement = true;
            try {
              this.readTmplToken();
            } catch (err) {
              if (err === INVALID_TEMPLATE_ESCAPE_ERROR) {
                this.readInvalidTemplateToken();
              } else {
                throw err
              }
            }

            this.inTemplateElement = false;
          };

          pp.invalidStringToken = function(position, message) {
            if (this.inTemplateElement && this.options.ecmaVersion >= 9) {
              throw INVALID_TEMPLATE_ESCAPE_ERROR
            } else {
              this.raise(position, message);
            }
          };

          pp.readTmplToken = function() {
            var out = "", chunkStart = this.pos;
            for (;;) {
              if (this.pos >= this.input.length) { this.raise(this.start, "Unterminated template"); }
              var ch = this.input.charCodeAt(this.pos);
              if (ch === 96 || ch === 36 && this.input.charCodeAt(this.pos + 1) === 123) { // '`', '${'
                if (this.pos === this.start && (this.type === types$1.template || this.type === types$1.invalidTemplate)) {
                  if (ch === 36) {
                    this.pos += 2;
                    return this.finishToken(types$1.dollarBraceL)
                  } else {
                    ++this.pos;
                    return this.finishToken(types$1.backQuote)
                  }
                }
                out += this.input.slice(chunkStart, this.pos);
                return this.finishToken(types$1.template, out)
              }
              if (ch === 92) { // '\'
                out += this.input.slice(chunkStart, this.pos);
                out += this.readEscapedChar(true);
                chunkStart = this.pos;
              } else if (isNewLine(ch)) {
                out += this.input.slice(chunkStart, this.pos);
                ++this.pos;
                switch (ch) {
                case 13:
                  if (this.input.charCodeAt(this.pos) === 10) { ++this.pos; }
                case 10:
                  out += "\n";
                  break
                default:
                  out += String.fromCharCode(ch);
                  break
                }
                if (this.options.locations) {
                  ++this.curLine;
                  this.lineStart = this.pos;
                }
                chunkStart = this.pos;
              } else {
                ++this.pos;
              }
            }
          };

          // Reads a template token to search for the end, without validating any escape sequences
          pp.readInvalidTemplateToken = function() {
            for (; this.pos < this.input.length; this.pos++) {
              switch (this.input[this.pos]) {
              case "\\":
                ++this.pos;
                break

              case "$":
                if (this.input[this.pos + 1] !== "{") { break }
                // fall through
              case "`":
                return this.finishToken(types$1.invalidTemplate, this.input.slice(this.start, this.pos))

              case "\r":
                if (this.input[this.pos + 1] === "\n") { ++this.pos; }
                // fall through
              case "\n": case "\u2028": case "\u2029":
                ++this.curLine;
                this.lineStart = this.pos + 1;
                break
              }
            }
            this.raise(this.start, "Unterminated template");
          };

          // Used to read escaped characters

          pp.readEscapedChar = function(inTemplate) {
            var ch = this.input.charCodeAt(++this.pos);
            ++this.pos;
            switch (ch) {
            case 110: return "\n" // 'n' -> '\n'
            case 114: return "\r" // 'r' -> '\r'
            case 120: return String.fromCharCode(this.readHexChar(2)) // 'x'
            case 117: return codePointToString(this.readCodePoint()) // 'u'
            case 116: return "\t" // 't' -> '\t'
            case 98: return "\b" // 'b' -> '\b'
            case 118: return "\u000b" // 'v' -> '\u000b'
            case 102: return "\f" // 'f' -> '\f'
            case 13: if (this.input.charCodeAt(this.pos) === 10) { ++this.pos; } // '\r\n'
            case 10: // ' \n'
              if (this.options.locations) { this.lineStart = this.pos; ++this.curLine; }
              return ""
            case 56:
            case 57:
              if (this.strict) {
                this.invalidStringToken(
                  this.pos - 1,
                  "Invalid escape sequence"
                );
              }
              if (inTemplate) {
                var codePos = this.pos - 1;

                this.invalidStringToken(
                  codePos,
                  "Invalid escape sequence in template string"
                );
              }
            default:
              if (ch >= 48 && ch <= 55) {
                var octalStr = this.input.substr(this.pos - 1, 3).match(/^[0-7]+/)[0];
                var octal = parseInt(octalStr, 8);
                if (octal > 255) {
                  octalStr = octalStr.slice(0, -1);
                  octal = parseInt(octalStr, 8);
                }
                this.pos += octalStr.length - 1;
                ch = this.input.charCodeAt(this.pos);
                if ((octalStr !== "0" || ch === 56 || ch === 57) && (this.strict || inTemplate)) {
                  this.invalidStringToken(
                    this.pos - 1 - octalStr.length,
                    inTemplate
                      ? "Octal literal in template string"
                      : "Octal literal in strict mode"
                  );
                }
                return String.fromCharCode(octal)
              }
              if (isNewLine(ch)) {
                // Unicode new line characters after \ get removed from output in both
                // template literals and strings
                if (this.options.locations) { this.lineStart = this.pos; ++this.curLine; }
                return ""
              }
              return String.fromCharCode(ch)
            }
          };

          // Used to read character escape sequences ('\x', '\u', '\U').

          pp.readHexChar = function(len) {
            var codePos = this.pos;
            var n = this.readInt(16, len);
            if (n === null) { this.invalidStringToken(codePos, "Bad character escape sequence"); }
            return n
          };

          // Read an identifier, and return it as a string. Sets `this.containsEsc`
          // to whether the word contained a '\u' escape.
          //
          // Incrementally adds only escaped chars, adding other chunks as-is
          // as a micro-optimization.

          pp.readWord1 = function() {
            this.containsEsc = false;
            var word = "", first = true, chunkStart = this.pos;
            var astral = this.options.ecmaVersion >= 6;
            while (this.pos < this.input.length) {
              var ch = this.fullCharCodeAtPos();
              if (isIdentifierChar(ch, astral)) {
                this.pos += ch <= 0xffff ? 1 : 2;
              } else if (ch === 92) { // "\"
                this.containsEsc = true;
                word += this.input.slice(chunkStart, this.pos);
                var escStart = this.pos;
                if (this.input.charCodeAt(++this.pos) !== 117) // "u"
                  { this.invalidStringToken(this.pos, "Expecting Unicode escape sequence \\uXXXX"); }
                ++this.pos;
                var esc = this.readCodePoint();
                if (!(first ? isIdentifierStart : isIdentifierChar)(esc, astral))
                  { this.invalidStringToken(escStart, "Invalid Unicode escape"); }
                word += codePointToString(esc);
                chunkStart = this.pos;
              } else {
                break
              }
              first = false;
            }
            return word + this.input.slice(chunkStart, this.pos)
          };

          // Read an identifier or keyword token. Will check for reserved
          // words when necessary.

          pp.readWord = function() {
            var word = this.readWord1();
            var type = types$1.name;
            if (this.keywords.test(word)) {
              type = keywords[word];
            }
            return this.finishToken(type, word)
          };

          // Acorn is a tiny, fast JavaScript parser written in JavaScript.
          //
          // Acorn was written by Marijn Haverbeke, Ingvar Stepanyan, and
          // various contributors and released under an MIT license.
          //
          // Git repositories for Acorn are available at
          //
          //     http://marijnhaverbeke.nl/git/acorn
          //     https://github.com/acornjs/acorn.git
          //
          // Please use the [github bug tracker][ghbt] to report issues.
          //
          // [ghbt]: https://github.com/acornjs/acorn/issues


          var version = "8.15.0";

          Parser.acorn = {
            Parser: Parser,
            version: version,
            defaultOptions: defaultOptions,
            Position: Position,
            SourceLocation: SourceLocation,
            getLineInfo: getLineInfo,
            Node: Node,
            TokenType: TokenType,
            tokTypes: types$1,
            keywordTypes: keywords,
            TokContext: TokContext,
            tokContexts: types,
            isIdentifierChar: isIdentifierChar,
            isIdentifierStart: isIdentifierStart,
            Token: Token,
            isNewLine: isNewLine,
            lineBreak: lineBreak,
            lineBreakG: lineBreakG,
            nonASCIIwhitespace: nonASCIIwhitespace
          };

          // The main exported interface (under `self.acorn` when in the
          // browser) is a `parse` function that takes a code string and returns
          // an abstract syntax tree as specified by the [ESTree spec][estree].
          //
          // [estree]: https://github.com/estree/estree

          function parse(input, options) {
            return Parser.parse(input, options)
          }

          // This function tries to parse a single expression at a given
          // offset in a string. Useful for parsing mixed-language formats
          // that embed JavaScript expressions.

          function parseExpressionAt(input, pos, options) {
            return Parser.parseExpressionAt(input, pos, options)
          }

          // Acorn is organized as a tokenizer and a recursive-descent parser.
          // The `tokenizer` export provides an interface to the tokenizer.

          function tokenizer(input, options) {
            return Parser.tokenizer(input, options)
          }

          exports.Node = Node;
          exports.Parser = Parser;
          exports.Position = Position;
          exports.SourceLocation = SourceLocation;
          exports.TokContext = TokContext;
          exports.Token = Token;
          exports.TokenType = TokenType;
          exports.defaultOptions = defaultOptions;
          exports.getLineInfo = getLineInfo;
          exports.isIdentifierChar = isIdentifierChar;
          exports.isIdentifierStart = isIdentifierStart;
          exports.isNewLine = isNewLine;
          exports.keywordTypes = keywords;
          exports.lineBreak = lineBreak;
          exports.lineBreakG = lineBreakG;
          exports.nonASCIIwhitespace = nonASCIIwhitespace;
          exports.parse = parse;
          exports.parseExpressionAt = parseExpressionAt;
          exports.tokContexts = types;
          exports.tokTypes = types$1;
          exports.tokenizer = tokenizer;
          exports.version = version;

        })); 
    } (acorn$1, acorn$1.exports));
    return acorn$1.exports;
}

var acornLoose = acornLoose$1.exports;

var hasRequiredAcornLoose;

function requireAcornLoose () {
    if (hasRequiredAcornLoose) return acornLoose$1.exports;
    hasRequiredAcornLoose = 1;
    (function (module, exports) {
        (function (global, factory) {
          factory(exports, requireAcorn()) ;
        })(acornLoose, (function (exports, acorn) {
          var dummyValue = "✖";

          function isDummy(node) { return node.name === dummyValue }

          function noop() {}

          var LooseParser = function LooseParser(input, options) {
            if ( options === void 0 ) options = {};

            this.toks = this.constructor.BaseParser.tokenizer(input, options);
            this.options = this.toks.options;
            this.input = this.toks.input;
            this.tok = this.last = {type: acorn.tokTypes.eof, start: 0, end: 0};
            this.tok.validateRegExpFlags = noop;
            this.tok.validateRegExpPattern = noop;
            if (this.options.locations) {
              var here = this.toks.curPosition();
              this.tok.loc = new acorn.SourceLocation(this.toks, here, here);
            }
            this.ahead = []; // Tokens ahead
            this.context = []; // Indentation contexted
            this.curIndent = 0;
            this.curLineStart = 0;
            this.nextLineStart = this.lineEnd(this.curLineStart) + 1;
            this.inAsync = false;
            this.inGenerator = false;
            this.inFunction = false;
          };

          LooseParser.prototype.startNode = function startNode () {
            return new acorn.Node(this.toks, this.tok.start, this.options.locations ? this.tok.loc.start : null)
          };

          LooseParser.prototype.storeCurrentPos = function storeCurrentPos () {
            return this.options.locations ? [this.tok.start, this.tok.loc.start] : this.tok.start
          };

          LooseParser.prototype.startNodeAt = function startNodeAt (pos) {
            if (this.options.locations) {
              return new acorn.Node(this.toks, pos[0], pos[1])
            } else {
              return new acorn.Node(this.toks, pos)
            }
          };

          LooseParser.prototype.finishNode = function finishNode (node, type) {
            node.type = type;
            node.end = this.last.end;
            if (this.options.locations)
              { node.loc.end = this.last.loc.end; }
            if (this.options.ranges)
              { node.range[1] = this.last.end; }
            return node
          };

          LooseParser.prototype.dummyNode = function dummyNode (type) {
            var dummy = this.startNode();
            dummy.type = type;
            dummy.end = dummy.start;
            if (this.options.locations)
              { dummy.loc.end = dummy.loc.start; }
            if (this.options.ranges)
              { dummy.range[1] = dummy.start; }
            this.last = {type: acorn.tokTypes.name, start: dummy.start, end: dummy.start, loc: dummy.loc};
            return dummy
          };

          LooseParser.prototype.dummyIdent = function dummyIdent () {
            var dummy = this.dummyNode("Identifier");
            dummy.name = dummyValue;
            return dummy
          };

          LooseParser.prototype.dummyString = function dummyString () {
            var dummy = this.dummyNode("Literal");
            dummy.value = dummy.raw = dummyValue;
            return dummy
          };

          LooseParser.prototype.eat = function eat (type) {
            if (this.tok.type === type) {
              this.next();
              return true
            } else {
              return false
            }
          };

          LooseParser.prototype.isContextual = function isContextual (name) {
            return this.tok.type === acorn.tokTypes.name && this.tok.value === name
          };

          LooseParser.prototype.eatContextual = function eatContextual (name) {
            return this.tok.value === name && this.eat(acorn.tokTypes.name)
          };

          LooseParser.prototype.canInsertSemicolon = function canInsertSemicolon () {
            return this.tok.type === acorn.tokTypes.eof || this.tok.type === acorn.tokTypes.braceR ||
              acorn.lineBreak.test(this.input.slice(this.last.end, this.tok.start))
          };

          LooseParser.prototype.semicolon = function semicolon () {
            return this.eat(acorn.tokTypes.semi)
          };

          LooseParser.prototype.expect = function expect (type) {
            if (this.eat(type)) { return true }
            for (var i = 1; i <= 2; i++) {
              if (this.lookAhead(i).type === type) {
                for (var j = 0; j < i; j++) { this.next(); }
                return true
              }
            }
          };

          LooseParser.prototype.pushCx = function pushCx () {
            this.context.push(this.curIndent);
          };

          LooseParser.prototype.popCx = function popCx () {
            this.curIndent = this.context.pop();
          };

          LooseParser.prototype.lineEnd = function lineEnd (pos) {
            while (pos < this.input.length && !acorn.isNewLine(this.input.charCodeAt(pos))) { ++pos; }
            return pos
          };

          LooseParser.prototype.indentationAfter = function indentationAfter (pos) {
            for (var count = 0;; ++pos) {
              var ch = this.input.charCodeAt(pos);
              if (ch === 32) { ++count; }
              else if (ch === 9) { count += this.options.tabSize; }
              else { return count }
            }
          };

          LooseParser.prototype.closes = function closes (closeTok, indent, line, blockHeuristic) {
            if (this.tok.type === closeTok || this.tok.type === acorn.tokTypes.eof) { return true }
            return line !== this.curLineStart && this.curIndent < indent && this.tokenStartsLine() &&
              (!blockHeuristic || this.nextLineStart >= this.input.length ||
               this.indentationAfter(this.nextLineStart) < indent)
          };

          LooseParser.prototype.tokenStartsLine = function tokenStartsLine () {
            for (var p = this.tok.start - 1; p >= this.curLineStart; --p) {
              var ch = this.input.charCodeAt(p);
              if (ch !== 9 && ch !== 32) { return false }
            }
            return true
          };

          LooseParser.prototype.extend = function extend (name, f) {
            this[name] = f(this[name]);
          };

          LooseParser.prototype.parse = function parse () {
            this.next();
            return this.parseTopLevel()
          };

          LooseParser.extend = function extend () {
              var plugins = [], len = arguments.length;
              while ( len-- ) plugins[ len ] = arguments[ len ];

            var cls = this;
            for (var i = 0; i < plugins.length; i++) { cls = plugins[i](cls); }
            return cls
          };

          LooseParser.parse = function parse (input, options) {
            return new this(input, options).parse()
          };

          // Allows plugins to extend the base parser / tokenizer used
          LooseParser.BaseParser = acorn.Parser;

          var lp$2 = LooseParser.prototype;

          function isSpace(ch) {
            return (ch < 14 && ch > 8) || ch === 32 || ch === 160 || acorn.isNewLine(ch)
          }

          lp$2.next = function() {
            this.last = this.tok;
            if (this.ahead.length)
              { this.tok = this.ahead.shift(); }
            else
              { this.tok = this.readToken(); }

            if (this.tok.start >= this.nextLineStart) {
              while (this.tok.start >= this.nextLineStart) {
                this.curLineStart = this.nextLineStart;
                this.nextLineStart = this.lineEnd(this.curLineStart) + 1;
              }
              this.curIndent = this.indentationAfter(this.curLineStart);
            }
          };

          lp$2.readToken = function() {
            for (;;) {
              try {
                this.toks.next();
                if (this.toks.type === acorn.tokTypes.dot &&
                    this.input.substr(this.toks.end, 1) === "." &&
                    this.options.ecmaVersion >= 6) {
                  this.toks.end++;
                  this.toks.type = acorn.tokTypes.ellipsis;
                }
                return new acorn.Token(this.toks)
              } catch (e) {
                if (!(e instanceof SyntaxError)) { throw e }

                // Try to skip some text, based on the error message, and then continue
                var msg = e.message, pos = e.raisedAt, replace = true;
                if (/unterminated/i.test(msg)) {
                  pos = this.lineEnd(e.pos + 1);
                  if (/string/.test(msg)) {
                    replace = {start: e.pos, end: pos, type: acorn.tokTypes.string, value: this.input.slice(e.pos + 1, pos)};
                  } else if (/regular expr/i.test(msg)) {
                    var re = this.input.slice(e.pos, pos);
                    try { re = new RegExp(re); } catch (e$1) { /* ignore compilation error due to new syntax */ }
                    replace = {start: e.pos, end: pos, type: acorn.tokTypes.regexp, value: re};
                  } else if (/template/.test(msg)) {
                    replace = {
                      start: e.pos,
                      end: pos,
                      type: acorn.tokTypes.template,
                      value: this.input.slice(e.pos, pos)
                    };
                  } else {
                    replace = false;
                  }
                } else if (/invalid (unicode|regexp|number)|expecting unicode|octal literal|is reserved|directly after number|expected number in radix|numeric separator/i.test(msg)) {
                  while (pos < this.input.length && !isSpace(this.input.charCodeAt(pos))) { ++pos; }
                } else if (/character escape|expected hexadecimal/i.test(msg)) {
                  while (pos < this.input.length) {
                    var ch = this.input.charCodeAt(pos++);
                    if (ch === 34 || ch === 39 || acorn.isNewLine(ch)) { break }
                  }
                } else if (/unexpected character/i.test(msg)) {
                  pos++;
                  replace = false;
                } else if (/regular expression/i.test(msg)) {
                  replace = true;
                } else {
                  throw e
                }
                this.resetTo(pos);
                if (replace === true) { replace = {start: pos, end: pos, type: acorn.tokTypes.name, value: dummyValue}; }
                if (replace) {
                  if (this.options.locations)
                    { replace.loc = new acorn.SourceLocation(
                      this.toks,
                      acorn.getLineInfo(this.input, replace.start),
                      acorn.getLineInfo(this.input, replace.end)); }
                  return replace
                }
              }
            }
          };

          lp$2.resetTo = function(pos) {
            this.toks.pos = pos;
            this.toks.containsEsc = false;
            var ch = this.input.charAt(pos - 1);
            this.toks.exprAllowed = !ch || /[[{(,;:?/*=+\-~!|&%^<>]/.test(ch) ||
              /[enwfd]/.test(ch) &&
              /\b(case|else|return|throw|new|in|(instance|type)?of|delete|void)$/.test(this.input.slice(pos - 10, pos));

            if (this.options.locations) {
              this.toks.curLine = 1;
              this.toks.lineStart = acorn.lineBreakG.lastIndex = 0;
              var match;
              while ((match = acorn.lineBreakG.exec(this.input)) && match.index < pos) {
                ++this.toks.curLine;
                this.toks.lineStart = match.index + match[0].length;
              }
            }
          };

          lp$2.lookAhead = function(n) {
            while (n > this.ahead.length)
              { this.ahead.push(this.readToken()); }
            return this.ahead[n - 1]
          };

          var lp$1 = LooseParser.prototype;

          lp$1.parseTopLevel = function() {
            var node = this.startNodeAt(this.options.locations ? [0, acorn.getLineInfo(this.input, 0)] : 0);
            node.body = [];
            while (this.tok.type !== acorn.tokTypes.eof) { node.body.push(this.parseStatement()); }
            this.toks.adaptDirectivePrologue(node.body);
            this.last = this.tok;
            node.sourceType = this.options.sourceType === "commonjs" ? "script" : this.options.sourceType;
            return this.finishNode(node, "Program")
          };

          lp$1.parseStatement = function() {
            var starttype = this.tok.type, node = this.startNode(), kind;

            if (this.toks.isLet()) {
              starttype = acorn.tokTypes._var;
              kind = "let";
            }

            switch (starttype) {
            case acorn.tokTypes._break: case acorn.tokTypes._continue:
              this.next();
              var isBreak = starttype === acorn.tokTypes._break;
              if (this.semicolon() || this.canInsertSemicolon()) {
                node.label = null;
              } else {
                node.label = this.tok.type === acorn.tokTypes.name ? this.parseIdent() : null;
                this.semicolon();
              }
              return this.finishNode(node, isBreak ? "BreakStatement" : "ContinueStatement")

            case acorn.tokTypes._debugger:
              this.next();
              this.semicolon();
              return this.finishNode(node, "DebuggerStatement")

            case acorn.tokTypes._do:
              this.next();
              node.body = this.parseStatement();
              node.test = this.eat(acorn.tokTypes._while) ? this.parseParenExpression() : this.dummyIdent();
              this.semicolon();
              return this.finishNode(node, "DoWhileStatement")

            case acorn.tokTypes._for:
              this.next(); // `for` keyword
              var isAwait = this.options.ecmaVersion >= 9 && this.eatContextual("await");

              this.pushCx();
              this.expect(acorn.tokTypes.parenL);
              if (this.tok.type === acorn.tokTypes.semi) { return this.parseFor(node, null) }
              var isLet = this.toks.isLet();
              var isAwaitUsing = this.toks.isAwaitUsing(true);
              var isUsing = !isAwaitUsing && this.toks.isUsing(true);

              if (isLet || this.tok.type === acorn.tokTypes._var || this.tok.type === acorn.tokTypes._const || isUsing || isAwaitUsing) {
                var kind$1 = isLet ? "let" : isUsing ? "using" : isAwaitUsing ? "await using" : this.tok.value;
                var init$1 = this.startNode();
                if (isUsing || isAwaitUsing) {
                  if (isAwaitUsing) { this.next(); }
                  this.parseVar(init$1, true, kind$1);
                } else {
                  init$1 = this.parseVar(init$1, true, kind$1);
                }

                if (init$1.declarations.length === 1 && (this.tok.type === acorn.tokTypes._in || this.isContextual("of"))) {
                  if (this.options.ecmaVersion >= 9 && this.tok.type !== acorn.tokTypes._in) {
                    node.await = isAwait;
                  }
                  return this.parseForIn(node, init$1)
                }
                return this.parseFor(node, init$1)
              }
              var init = this.parseExpression(true);
              if (this.tok.type === acorn.tokTypes._in || this.isContextual("of")) {
                if (this.options.ecmaVersion >= 9 && this.tok.type !== acorn.tokTypes._in) {
                  node.await = isAwait;
                }
                return this.parseForIn(node, this.toAssignable(init))
              }
              return this.parseFor(node, init)

            case acorn.tokTypes._function:
              this.next();
              return this.parseFunction(node, true)

            case acorn.tokTypes._if:
              this.next();
              node.test = this.parseParenExpression();
              node.consequent = this.parseStatement();
              node.alternate = this.eat(acorn.tokTypes._else) ? this.parseStatement() : null;
              return this.finishNode(node, "IfStatement")

            case acorn.tokTypes._return:
              this.next();
              if (this.eat(acorn.tokTypes.semi) || this.canInsertSemicolon()) { node.argument = null; }
              else { node.argument = this.parseExpression(); this.semicolon(); }
              return this.finishNode(node, "ReturnStatement")

            case acorn.tokTypes._switch:
              var blockIndent = this.curIndent, line = this.curLineStart;
              this.next();
              node.discriminant = this.parseParenExpression();
              node.cases = [];
              this.pushCx();
              this.expect(acorn.tokTypes.braceL);

              var cur;
              while (!this.closes(acorn.tokTypes.braceR, blockIndent, line, true)) {
                if (this.tok.type === acorn.tokTypes._case || this.tok.type === acorn.tokTypes._default) {
                  var isCase = this.tok.type === acorn.tokTypes._case;
                  if (cur) { this.finishNode(cur, "SwitchCase"); }
                  node.cases.push(cur = this.startNode());
                  cur.consequent = [];
                  this.next();
                  if (isCase) { cur.test = this.parseExpression(); }
                  else { cur.test = null; }
                  this.expect(acorn.tokTypes.colon);
                } else {
                  if (!cur) {
                    node.cases.push(cur = this.startNode());
                    cur.consequent = [];
                    cur.test = null;
                  }
                  cur.consequent.push(this.parseStatement());
                }
              }
              if (cur) { this.finishNode(cur, "SwitchCase"); }
              this.popCx();
              this.eat(acorn.tokTypes.braceR);
              return this.finishNode(node, "SwitchStatement")

            case acorn.tokTypes._throw:
              this.next();
              node.argument = this.parseExpression();
              this.semicolon();
              return this.finishNode(node, "ThrowStatement")

            case acorn.tokTypes._try:
              this.next();
              node.block = this.parseBlock();
              node.handler = null;
              if (this.tok.type === acorn.tokTypes._catch) {
                var clause = this.startNode();
                this.next();
                if (this.eat(acorn.tokTypes.parenL)) {
                  clause.param = this.toAssignable(this.parseExprAtom(), true);
                  this.expect(acorn.tokTypes.parenR);
                } else {
                  clause.param = null;
                }
                clause.body = this.parseBlock();
                node.handler = this.finishNode(clause, "CatchClause");
              }
              node.finalizer = this.eat(acorn.tokTypes._finally) ? this.parseBlock() : null;
              if (!node.handler && !node.finalizer) { return node.block }
              return this.finishNode(node, "TryStatement")

            case acorn.tokTypes._var:
            case acorn.tokTypes._const:
              return this.parseVar(node, false, kind || this.tok.value)

            case acorn.tokTypes._while:
              this.next();
              node.test = this.parseParenExpression();
              node.body = this.parseStatement();
              return this.finishNode(node, "WhileStatement")

            case acorn.tokTypes._with:
              this.next();
              node.object = this.parseParenExpression();
              node.body = this.parseStatement();
              return this.finishNode(node, "WithStatement")

            case acorn.tokTypes.braceL:
              return this.parseBlock()

            case acorn.tokTypes.semi:
              this.next();
              return this.finishNode(node, "EmptyStatement")

            case acorn.tokTypes._class:
              return this.parseClass(true)

            case acorn.tokTypes._import:
              if (this.options.ecmaVersion > 10) {
                var nextType = this.lookAhead(1).type;
                if (nextType === acorn.tokTypes.parenL || nextType === acorn.tokTypes.dot) {
                  node.expression = this.parseExpression();
                  this.semicolon();
                  return this.finishNode(node, "ExpressionStatement")
                }
              }

              return this.parseImport()

            case acorn.tokTypes._export:
              return this.parseExport()

            default:
              if (this.toks.isAsyncFunction()) {
                this.next();
                this.next();
                return this.parseFunction(node, true, true)
              }

              if (this.toks.isUsing(false)) {
                return this.parseVar(node, false, "using")
              }

              if (this.toks.isAwaitUsing(false)) {
                this.next();
                return this.parseVar(node, false, "await using")
              }

              var expr = this.parseExpression();
              if (isDummy(expr)) {
                this.next();
                if (this.tok.type === acorn.tokTypes.eof) { return this.finishNode(node, "EmptyStatement") }
                return this.parseStatement()
              } else if (starttype === acorn.tokTypes.name && expr.type === "Identifier" && this.eat(acorn.tokTypes.colon)) {
                node.body = this.parseStatement();
                node.label = expr;
                return this.finishNode(node, "LabeledStatement")
              } else {
                node.expression = expr;
                this.semicolon();
                return this.finishNode(node, "ExpressionStatement")
              }
            }
          };

          lp$1.parseBlock = function() {
            var node = this.startNode();
            this.pushCx();
            this.expect(acorn.tokTypes.braceL);
            var blockIndent = this.curIndent, line = this.curLineStart;
            node.body = [];
            while (!this.closes(acorn.tokTypes.braceR, blockIndent, line, true))
              { node.body.push(this.parseStatement()); }
            this.popCx();
            this.eat(acorn.tokTypes.braceR);
            return this.finishNode(node, "BlockStatement")
          };

          lp$1.parseFor = function(node, init) {
            node.init = init;
            node.test = node.update = null;
            if (this.eat(acorn.tokTypes.semi) && this.tok.type !== acorn.tokTypes.semi) { node.test = this.parseExpression(); }
            if (this.eat(acorn.tokTypes.semi) && this.tok.type !== acorn.tokTypes.parenR) { node.update = this.parseExpression(); }
            this.popCx();
            this.expect(acorn.tokTypes.parenR);
            node.body = this.parseStatement();
            return this.finishNode(node, "ForStatement")
          };

          lp$1.parseForIn = function(node, init) {
            var type = this.tok.type === acorn.tokTypes._in ? "ForInStatement" : "ForOfStatement";
            this.next();
            node.left = init;
            node.right = this.parseExpression();
            this.popCx();
            this.expect(acorn.tokTypes.parenR);
            node.body = this.parseStatement();
            return this.finishNode(node, type)
          };

          lp$1.parseVar = function(node, noIn, kind) {
            node.kind = kind;
            this.next();
            node.declarations = [];
            do {
              var decl = this.startNode();
              decl.id = this.options.ecmaVersion >= 6 ? this.toAssignable(this.parseExprAtom(), true) : this.parseIdent();
              decl.init = this.eat(acorn.tokTypes.eq) ? this.parseMaybeAssign(noIn) : null;
              node.declarations.push(this.finishNode(decl, "VariableDeclarator"));
            } while (this.eat(acorn.tokTypes.comma))
            if (!node.declarations.length) {
              var decl$1 = this.startNode();
              decl$1.id = this.dummyIdent();
              node.declarations.push(this.finishNode(decl$1, "VariableDeclarator"));
            }
            if (!noIn) { this.semicolon(); }
            return this.finishNode(node, "VariableDeclaration")
          };

          lp$1.parseClass = function(isStatement) {
            var node = this.startNode();
            this.next();
            if (this.tok.type === acorn.tokTypes.name) { node.id = this.parseIdent(); }
            else if (isStatement === true) { node.id = this.dummyIdent(); }
            else { node.id = null; }
            node.superClass = this.eat(acorn.tokTypes._extends) ? this.parseExpression() : null;
            node.body = this.startNode();
            node.body.body = [];
            this.pushCx();
            var indent = this.curIndent + 1, line = this.curLineStart;
            this.eat(acorn.tokTypes.braceL);
            if (this.curIndent + 1 < indent) { indent = this.curIndent; line = this.curLineStart; }
            while (!this.closes(acorn.tokTypes.braceR, indent, line)) {
              var element = this.parseClassElement();
              if (element) { node.body.body.push(element); }
            }
            this.popCx();
            if (!this.eat(acorn.tokTypes.braceR)) {
              // If there is no closing brace, make the node span to the start
              // of the next token (this is useful for Tern)
              this.last.end = this.tok.start;
              if (this.options.locations) { this.last.loc.end = this.tok.loc.start; }
            }
            this.semicolon();
            this.finishNode(node.body, "ClassBody");
            return this.finishNode(node, isStatement ? "ClassDeclaration" : "ClassExpression")
          };

          lp$1.parseClassElement = function() {
            if (this.eat(acorn.tokTypes.semi)) { return null }

            var ref = this.options;
            var ecmaVersion = ref.ecmaVersion;
            var locations = ref.locations;
            var indent = this.curIndent;
            var line = this.curLineStart;
            var node = this.startNode();
            var keyName = "";
            var isGenerator = false;
            var isAsync = false;
            var kind = "method";
            var isStatic = false;

            if (this.eatContextual("static")) {
              // Parse static init block
              if (ecmaVersion >= 13 && this.eat(acorn.tokTypes.braceL)) {
                this.parseClassStaticBlock(node);
                return node
              }
              if (this.isClassElementNameStart() || this.toks.type === acorn.tokTypes.star) {
                isStatic = true;
              } else {
                keyName = "static";
              }
            }
            node.static = isStatic;
            if (!keyName && ecmaVersion >= 8 && this.eatContextual("async")) {
              if ((this.isClassElementNameStart() || this.toks.type === acorn.tokTypes.star) && !this.canInsertSemicolon()) {
                isAsync = true;
              } else {
                keyName = "async";
              }
            }
            if (!keyName) {
              isGenerator = this.eat(acorn.tokTypes.star);
              var lastValue = this.toks.value;
              if (this.eatContextual("get") || this.eatContextual("set")) {
                if (this.isClassElementNameStart()) {
                  kind = lastValue;
                } else {
                  keyName = lastValue;
                }
              }
            }

            // Parse element name
            if (keyName) {
              // 'async', 'get', 'set', or 'static' were not a keyword contextually.
              // The last token is any of those. Make it the element name.
              node.computed = false;
              node.key = this.startNodeAt(locations ? [this.toks.lastTokStart, this.toks.lastTokStartLoc] : this.toks.lastTokStart);
              node.key.name = keyName;
              this.finishNode(node.key, "Identifier");
            } else {
              this.parseClassElementName(node);

              // From https://github.com/acornjs/acorn/blob/7deba41118d6384a2c498c61176b3cf434f69590/acorn-loose/src/statement.js#L291
              // Skip broken stuff.
              if (isDummy(node.key)) {
                if (isDummy(this.parseMaybeAssign())) { this.next(); }
                this.eat(acorn.tokTypes.comma);
                return null
              }
            }

            // Parse element value
            if (ecmaVersion < 13 || this.toks.type === acorn.tokTypes.parenL || kind !== "method" || isGenerator || isAsync) {
              // Method
              var isConstructor =
                !node.computed &&
                !node.static &&
                !isGenerator &&
                !isAsync &&
                kind === "method" && (
                  node.key.type === "Identifier" && node.key.name === "constructor" ||
                  node.key.type === "Literal" && node.key.value === "constructor"
                );
              node.kind = isConstructor ? "constructor" : kind;
              node.value = this.parseMethod(isGenerator, isAsync);
              this.finishNode(node, "MethodDefinition");
            } else {
              // Field
              if (this.eat(acorn.tokTypes.eq)) {
                if (this.curLineStart !== line && this.curIndent <= indent && this.tokenStartsLine()) {
                  // Estimated the next line is the next class element by indentations.
                  node.value = null;
                } else {
                  var oldInAsync = this.inAsync;
                  var oldInGenerator = this.inGenerator;
                  this.inAsync = false;
                  this.inGenerator = false;
                  node.value = this.parseMaybeAssign();
                  this.inAsync = oldInAsync;
                  this.inGenerator = oldInGenerator;
                }
              } else {
                node.value = null;
              }
              this.semicolon();
              this.finishNode(node, "PropertyDefinition");
            }

            return node
          };

          lp$1.parseClassStaticBlock = function(node) {
            var blockIndent = this.curIndent, line = this.curLineStart;
            node.body = [];
            this.pushCx();
            while (!this.closes(acorn.tokTypes.braceR, blockIndent, line, true))
              { node.body.push(this.parseStatement()); }
            this.popCx();
            this.eat(acorn.tokTypes.braceR);

            return this.finishNode(node, "StaticBlock")
          };

          lp$1.isClassElementNameStart = function() {
            return this.toks.isClassElementNameStart()
          };

          lp$1.parseClassElementName = function(element) {
            if (this.toks.type === acorn.tokTypes.privateId) {
              element.computed = false;
              element.key = this.parsePrivateIdent();
            } else {
              this.parsePropertyName(element);
            }
          };

          lp$1.parseFunction = function(node, isStatement, isAsync) {
            var oldInAsync = this.inAsync, oldInGenerator = this.inGenerator, oldInFunction = this.inFunction;
            this.initFunction(node);
            if (this.options.ecmaVersion >= 6) {
              node.generator = this.eat(acorn.tokTypes.star);
            }
            if (this.options.ecmaVersion >= 8) {
              node.async = !!isAsync;
            }
            if (this.tok.type === acorn.tokTypes.name) { node.id = this.parseIdent(); }
            else if (isStatement === true) { node.id = this.dummyIdent(); }
            this.inAsync = node.async;
            this.inGenerator = node.generator;
            this.inFunction = true;
            node.params = this.parseFunctionParams();
            node.body = this.parseBlock();
            this.toks.adaptDirectivePrologue(node.body.body);
            this.inAsync = oldInAsync;
            this.inGenerator = oldInGenerator;
            this.inFunction = oldInFunction;
            return this.finishNode(node, isStatement ? "FunctionDeclaration" : "FunctionExpression")
          };

          lp$1.parseExport = function() {
            var node = this.startNode();
            this.next();
            if (this.eat(acorn.tokTypes.star)) {
              if (this.options.ecmaVersion >= 11) {
                if (this.eatContextual("as")) {
                  node.exported = this.parseExprAtom();
                } else {
                  node.exported = null;
                }
              }
              node.source = this.eatContextual("from") ? this.parseExprAtom() : this.dummyString();
              if (this.options.ecmaVersion >= 16)
                { node.attributes = this.parseWithClause(); }
              this.semicolon();
              return this.finishNode(node, "ExportAllDeclaration")
            }
            if (this.eat(acorn.tokTypes._default)) {
              // export default (function foo() {}) // This is FunctionExpression.
              var isAsync;
              if (this.tok.type === acorn.tokTypes._function || (isAsync = this.toks.isAsyncFunction())) {
                var fNode = this.startNode();
                this.next();
                if (isAsync) { this.next(); }
                node.declaration = this.parseFunction(fNode, "nullableID", isAsync);
              } else if (this.tok.type === acorn.tokTypes._class) {
                node.declaration = this.parseClass("nullableID");
              } else {
                node.declaration = this.parseMaybeAssign();
                this.semicolon();
              }
              return this.finishNode(node, "ExportDefaultDeclaration")
            }
            if (this.tok.type.keyword || this.toks.isLet() || this.toks.isAsyncFunction()) {
              node.declaration = this.parseStatement();
              node.specifiers = [];
              node.source = null;
            } else {
              node.declaration = null;
              node.specifiers = this.parseExportSpecifierList();
              node.source = this.eatContextual("from") ? this.parseExprAtom() : null;
              if (this.options.ecmaVersion >= 16)
                { node.attributes = this.parseWithClause(); }
              this.semicolon();
            }
            return this.finishNode(node, "ExportNamedDeclaration")
          };

          lp$1.parseImport = function() {
            var node = this.startNode();
            this.next();
            if (this.tok.type === acorn.tokTypes.string) {
              node.specifiers = [];
              node.source = this.parseExprAtom();
            } else {
              var elt;
              if (this.tok.type === acorn.tokTypes.name && this.tok.value !== "from") {
                elt = this.startNode();
                elt.local = this.parseIdent();
                this.finishNode(elt, "ImportDefaultSpecifier");
                this.eat(acorn.tokTypes.comma);
              }
              node.specifiers = this.parseImportSpecifiers();
              node.source = this.eatContextual("from") && this.tok.type === acorn.tokTypes.string ? this.parseExprAtom() : this.dummyString();
              if (elt) { node.specifiers.unshift(elt); }
            }
            if (this.options.ecmaVersion >= 16)
              { node.attributes = this.parseWithClause(); }
            this.semicolon();
            return this.finishNode(node, "ImportDeclaration")
          };

          lp$1.parseImportSpecifiers = function() {
            var elts = [];
            if (this.tok.type === acorn.tokTypes.star) {
              var elt = this.startNode();
              this.next();
              elt.local = this.eatContextual("as") ? this.parseIdent() : this.dummyIdent();
              elts.push(this.finishNode(elt, "ImportNamespaceSpecifier"));
            } else {
              var indent = this.curIndent, line = this.curLineStart, continuedLine = this.nextLineStart;
              this.pushCx();
              this.eat(acorn.tokTypes.braceL);
              if (this.curLineStart > continuedLine) { continuedLine = this.curLineStart; }
              while (!this.closes(acorn.tokTypes.braceR, indent + (this.curLineStart <= continuedLine ? 1 : 0), line)) {
                var elt$1 = this.startNode();
                if (this.eat(acorn.tokTypes.star)) {
                  elt$1.local = this.eatContextual("as") ? this.parseModuleExportName() : this.dummyIdent();
                  this.finishNode(elt$1, "ImportNamespaceSpecifier");
                } else {
                  if (this.isContextual("from")) { break }
                  elt$1.imported = this.parseModuleExportName();
                  if (isDummy(elt$1.imported)) { break }
                  elt$1.local = this.eatContextual("as") ? this.parseModuleExportName() : elt$1.imported;
                  this.finishNode(elt$1, "ImportSpecifier");
                }
                elts.push(elt$1);
                this.eat(acorn.tokTypes.comma);
              }
              this.eat(acorn.tokTypes.braceR);
              this.popCx();
            }
            return elts
          };

          lp$1.parseWithClause = function() {
            var nodes = [];
            if (!this.eat(acorn.tokTypes._with)) {
              return nodes
            }

            var indent = this.curIndent, line = this.curLineStart, continuedLine = this.nextLineStart;
            this.pushCx();
            this.eat(acorn.tokTypes.braceL);
            if (this.curLineStart > continuedLine) { continuedLine = this.curLineStart; }
            while (!this.closes(acorn.tokTypes.braceR, indent + (this.curLineStart <= continuedLine ? 1 : 0), line)) {
              var attr = this.startNode();
              attr.key = this.tok.type === acorn.tokTypes.string ? this.parseExprAtom() : this.parseIdent();
              if (this.eat(acorn.tokTypes.colon)) {
                if (this.tok.type === acorn.tokTypes.string)
                  { attr.value = this.parseExprAtom(); }
                else { attr.value = this.dummyString(); }
              } else {
                if (isDummy(attr.key)) { break }
                if (this.tok.type === acorn.tokTypes.string)
                  { attr.value = this.parseExprAtom(); }
                else { break }
              }
              nodes.push(this.finishNode(attr, "ImportAttribute"));
              this.eat(acorn.tokTypes.comma);
            }
            this.eat(acorn.tokTypes.braceR);
            this.popCx();
            return nodes
          };

          lp$1.parseExportSpecifierList = function() {
            var elts = [];
            var indent = this.curIndent, line = this.curLineStart, continuedLine = this.nextLineStart;
            this.pushCx();
            this.eat(acorn.tokTypes.braceL);
            if (this.curLineStart > continuedLine) { continuedLine = this.curLineStart; }
            while (!this.closes(acorn.tokTypes.braceR, indent + (this.curLineStart <= continuedLine ? 1 : 0), line)) {
              if (this.isContextual("from")) { break }
              var elt = this.startNode();
              elt.local = this.parseModuleExportName();
              if (isDummy(elt.local)) { break }
              elt.exported = this.eatContextual("as") ? this.parseModuleExportName() : elt.local;
              this.finishNode(elt, "ExportSpecifier");
              elts.push(elt);
              this.eat(acorn.tokTypes.comma);
            }
            this.eat(acorn.tokTypes.braceR);
            this.popCx();
            return elts
          };

          lp$1.parseModuleExportName = function() {
            return this.options.ecmaVersion >= 13 && this.tok.type === acorn.tokTypes.string
              ? this.parseExprAtom()
              : this.parseIdent()
          };

          var lp = LooseParser.prototype;

          lp.checkLVal = function(expr) {
            if (!expr) { return expr }
            switch (expr.type) {
            case "Identifier":
            case "MemberExpression":
              return expr

            case "ParenthesizedExpression":
              expr.expression = this.checkLVal(expr.expression);
              return expr

            default:
              return this.dummyIdent()
            }
          };

          lp.parseExpression = function(noIn) {
            var start = this.storeCurrentPos();
            var expr = this.parseMaybeAssign(noIn);
            if (this.tok.type === acorn.tokTypes.comma) {
              var node = this.startNodeAt(start);
              node.expressions = [expr];
              while (this.eat(acorn.tokTypes.comma)) { node.expressions.push(this.parseMaybeAssign(noIn)); }
              return this.finishNode(node, "SequenceExpression")
            }
            return expr
          };

          lp.parseParenExpression = function() {
            this.pushCx();
            this.expect(acorn.tokTypes.parenL);
            var val = this.parseExpression();
            this.popCx();
            this.expect(acorn.tokTypes.parenR);
            return val
          };

          lp.parseMaybeAssign = function(noIn) {
            // `yield` should be an identifier reference if it's not in generator functions.
            if (this.inGenerator && this.toks.isContextual("yield")) {
              var node = this.startNode();
              this.next();
              if (this.semicolon() || this.canInsertSemicolon() || (this.tok.type !== acorn.tokTypes.star && !this.tok.type.startsExpr)) {
                node.delegate = false;
                node.argument = null;
              } else {
                node.delegate = this.eat(acorn.tokTypes.star);
                node.argument = this.parseMaybeAssign();
              }
              return this.finishNode(node, "YieldExpression")
            }

            var start = this.storeCurrentPos();
            var left = this.parseMaybeConditional(noIn);
            if (this.tok.type.isAssign) {
              var node$1 = this.startNodeAt(start);
              node$1.operator = this.tok.value;
              node$1.left = this.tok.type === acorn.tokTypes.eq ? this.toAssignable(left) : this.checkLVal(left);
              this.next();
              node$1.right = this.parseMaybeAssign(noIn);
              return this.finishNode(node$1, "AssignmentExpression")
            }
            return left
          };

          lp.parseMaybeConditional = function(noIn) {
            var start = this.storeCurrentPos();
            var expr = this.parseExprOps(noIn);
            if (this.eat(acorn.tokTypes.question)) {
              var node = this.startNodeAt(start);
              node.test = expr;
              node.consequent = this.parseMaybeAssign();
              node.alternate = this.expect(acorn.tokTypes.colon) ? this.parseMaybeAssign(noIn) : this.dummyIdent();
              return this.finishNode(node, "ConditionalExpression")
            }
            return expr
          };

          lp.parseExprOps = function(noIn) {
            var start = this.storeCurrentPos();
            var indent = this.curIndent, line = this.curLineStart;
            return this.parseExprOp(this.parseMaybeUnary(false), start, -1, noIn, indent, line)
          };

          lp.parseExprOp = function(left, start, minPrec, noIn, indent, line) {
            if (this.curLineStart !== line && this.curIndent < indent && this.tokenStartsLine()) { return left }
            var prec = this.tok.type.binop;
            if (prec != null && (!noIn || this.tok.type !== acorn.tokTypes._in)) {
              if (prec > minPrec) {
                var node = this.startNodeAt(start);
                node.left = left;
                node.operator = this.tok.value;
                this.next();
                if (this.curLineStart !== line && this.curIndent < indent && this.tokenStartsLine()) {
                  node.right = this.dummyIdent();
                } else {
                  var rightStart = this.storeCurrentPos();
                  node.right = this.parseExprOp(this.parseMaybeUnary(false), rightStart, prec, noIn, indent, line);
                }
                this.finishNode(node, /&&|\|\||\?\?/.test(node.operator) ? "LogicalExpression" : "BinaryExpression");
                return this.parseExprOp(node, start, minPrec, noIn, indent, line)
              }
            }
            return left
          };

          lp.parseMaybeUnary = function(sawUnary) {
            var start = this.storeCurrentPos(), expr;
            if (this.options.ecmaVersion >= 8 && this.toks.isContextual("await") &&
                (this.inAsync || (this.toks.inModule && this.options.ecmaVersion >= 13) ||
                 (!this.inFunction && this.options.allowAwaitOutsideFunction))) {
              expr = this.parseAwait();
              sawUnary = true;
            } else if (this.tok.type.prefix) {
              var node = this.startNode(), update = this.tok.type === acorn.tokTypes.incDec;
              if (!update) { sawUnary = true; }
              node.operator = this.tok.value;
              node.prefix = true;
              this.next();
              node.argument = this.parseMaybeUnary(true);
              if (update) { node.argument = this.checkLVal(node.argument); }
              expr = this.finishNode(node, update ? "UpdateExpression" : "UnaryExpression");
            } else if (this.tok.type === acorn.tokTypes.ellipsis) {
              var node$1 = this.startNode();
              this.next();
              node$1.argument = this.parseMaybeUnary(sawUnary);
              expr = this.finishNode(node$1, "SpreadElement");
            } else if (!sawUnary && this.tok.type === acorn.tokTypes.privateId) {
              expr = this.parsePrivateIdent();
            } else {
              expr = this.parseExprSubscripts();
              while (this.tok.type.postfix && !this.canInsertSemicolon()) {
                var node$2 = this.startNodeAt(start);
                node$2.operator = this.tok.value;
                node$2.prefix = false;
                node$2.argument = this.checkLVal(expr);
                this.next();
                expr = this.finishNode(node$2, "UpdateExpression");
              }
            }

            if (!sawUnary && this.eat(acorn.tokTypes.starstar)) {
              var node$3 = this.startNodeAt(start);
              node$3.operator = "**";
              node$3.left = expr;
              node$3.right = this.parseMaybeUnary(false);
              return this.finishNode(node$3, "BinaryExpression")
            }

            return expr
          };

          lp.parseExprSubscripts = function() {
            var start = this.storeCurrentPos();
            return this.parseSubscripts(this.parseExprAtom(), start, false, this.curIndent, this.curLineStart)
          };

          lp.parseSubscripts = function(base, start, noCalls, startIndent, line) {
            var optionalSupported = this.options.ecmaVersion >= 11;
            var optionalChained = false;
            for (;;) {
              if (this.curLineStart !== line && this.curIndent <= startIndent && this.tokenStartsLine()) {
                if (this.tok.type === acorn.tokTypes.dot && this.curIndent === startIndent)
                  { --startIndent; }
                else
                  { break }
              }

              var maybeAsyncArrow = base.type === "Identifier" && base.name === "async" && !this.canInsertSemicolon();
              var optional = optionalSupported && this.eat(acorn.tokTypes.questionDot);
              if (optional) {
                optionalChained = true;
              }

              if ((optional && this.tok.type !== acorn.tokTypes.parenL && this.tok.type !== acorn.tokTypes.bracketL && this.tok.type !== acorn.tokTypes.backQuote) || this.eat(acorn.tokTypes.dot)) {
                var node = this.startNodeAt(start);
                node.object = base;
                if (this.curLineStart !== line && this.curIndent <= startIndent && this.tokenStartsLine())
                  { node.property = this.dummyIdent(); }
                else
                  { node.property = this.parsePropertyAccessor() || this.dummyIdent(); }
                node.computed = false;
                if (optionalSupported) {
                  node.optional = optional;
                }
                base = this.finishNode(node, "MemberExpression");
              } else if (this.tok.type === acorn.tokTypes.bracketL) {
                this.pushCx();
                this.next();
                var node$1 = this.startNodeAt(start);
                node$1.object = base;
                node$1.property = this.parseExpression();
                node$1.computed = true;
                if (optionalSupported) {
                  node$1.optional = optional;
                }
                this.popCx();
                this.expect(acorn.tokTypes.bracketR);
                base = this.finishNode(node$1, "MemberExpression");
              } else if (!noCalls && this.tok.type === acorn.tokTypes.parenL) {
                var exprList = this.parseExprList(acorn.tokTypes.parenR);
                if (maybeAsyncArrow && this.eat(acorn.tokTypes.arrow))
                  { return this.parseArrowExpression(this.startNodeAt(start), exprList, true) }
                var node$2 = this.startNodeAt(start);
                node$2.callee = base;
                node$2.arguments = exprList;
                if (optionalSupported) {
                  node$2.optional = optional;
                }
                base = this.finishNode(node$2, "CallExpression");
              } else if (this.tok.type === acorn.tokTypes.backQuote) {
                var node$3 = this.startNodeAt(start);
                node$3.tag = base;
                node$3.quasi = this.parseTemplate();
                base = this.finishNode(node$3, "TaggedTemplateExpression");
              } else {
                break
              }
            }

            if (optionalChained) {
              var chainNode = this.startNodeAt(start);
              chainNode.expression = base;
              base = this.finishNode(chainNode, "ChainExpression");
            }
            return base
          };

          lp.parseExprAtom = function() {
            var node;
            switch (this.tok.type) {
            case acorn.tokTypes._this:
            case acorn.tokTypes._super:
              var type = this.tok.type === acorn.tokTypes._this ? "ThisExpression" : "Super";
              node = this.startNode();
              this.next();
              return this.finishNode(node, type)

            case acorn.tokTypes.name:
              var start = this.storeCurrentPos();
              var id = this.parseIdent();
              var isAsync = false;
              if (id.name === "async" && !this.canInsertSemicolon()) {
                if (this.eat(acorn.tokTypes._function)) {
                  this.toks.overrideContext(acorn.tokContexts.f_expr);
                  return this.parseFunction(this.startNodeAt(start), false, true)
                }
                if (this.tok.type === acorn.tokTypes.name) {
                  id = this.parseIdent();
                  isAsync = true;
                }
              }
              return this.eat(acorn.tokTypes.arrow) ? this.parseArrowExpression(this.startNodeAt(start), [id], isAsync) : id

            case acorn.tokTypes.regexp:
              node = this.startNode();
              var val = this.tok.value;
              node.regex = {pattern: val.pattern, flags: val.flags};
              node.value = val.value;
              node.raw = this.input.slice(this.tok.start, this.tok.end);
              this.next();
              return this.finishNode(node, "Literal")

            case acorn.tokTypes.num: case acorn.tokTypes.string:
              node = this.startNode();
              node.value = this.tok.value;
              node.raw = this.input.slice(this.tok.start, this.tok.end);
              if (this.tok.type === acorn.tokTypes.num && node.raw.charCodeAt(node.raw.length - 1) === 110)
                { node.bigint = node.value != null ? node.value.toString() : node.raw.slice(0, -1).replace(/_/g, ""); }
              this.next();
              return this.finishNode(node, "Literal")

            case acorn.tokTypes._null: case acorn.tokTypes._true: case acorn.tokTypes._false:
              node = this.startNode();
              node.value = this.tok.type === acorn.tokTypes._null ? null : this.tok.type === acorn.tokTypes._true;
              node.raw = this.tok.type.keyword;
              this.next();
              return this.finishNode(node, "Literal")

            case acorn.tokTypes.parenL:
              var parenStart = this.storeCurrentPos();
              this.next();
              var inner = this.parseExpression();
              this.expect(acorn.tokTypes.parenR);
              if (this.eat(acorn.tokTypes.arrow)) {
                // (a,)=>a // SequenceExpression makes dummy in the last hole. Drop the dummy.
                var params = inner.expressions || [inner];
                if (params.length && isDummy(params[params.length - 1]))
                  { params.pop(); }
                return this.parseArrowExpression(this.startNodeAt(parenStart), params)
              }
              if (this.options.preserveParens) {
                var par = this.startNodeAt(parenStart);
                par.expression = inner;
                inner = this.finishNode(par, "ParenthesizedExpression");
              }
              return inner

            case acorn.tokTypes.bracketL:
              node = this.startNode();
              node.elements = this.parseExprList(acorn.tokTypes.bracketR, true);
              return this.finishNode(node, "ArrayExpression")

            case acorn.tokTypes.braceL:
              this.toks.overrideContext(acorn.tokContexts.b_expr);
              return this.parseObj()

            case acorn.tokTypes._class:
              return this.parseClass(false)

            case acorn.tokTypes._function:
              node = this.startNode();
              this.next();
              return this.parseFunction(node, false)

            case acorn.tokTypes._new:
              return this.parseNew()

            case acorn.tokTypes.backQuote:
              return this.parseTemplate()

            case acorn.tokTypes._import:
              if (this.options.ecmaVersion >= 11) {
                return this.parseExprImport()
              } else {
                return this.dummyIdent()
              }

            default:
              return this.dummyIdent()
            }
          };

          lp.parseExprImport = function() {
            var node = this.startNode();
            var meta = this.parseIdent(true);
            switch (this.tok.type) {
            case acorn.tokTypes.parenL:
              return this.parseDynamicImport(node)
            case acorn.tokTypes.dot:
              node.meta = meta;
              return this.parseImportMeta(node)
            default:
              node.name = "import";
              return this.finishNode(node, "Identifier")
            }
          };

          lp.parseDynamicImport = function(node) {
            var list = this.parseExprList(acorn.tokTypes.parenR);
            node.source = list[0] || this.dummyString();
            node.options = list[1] || null;
            return this.finishNode(node, "ImportExpression")
          };

          lp.parseImportMeta = function(node) {
            this.next(); // skip '.'
            node.property = this.parseIdent(true);
            return this.finishNode(node, "MetaProperty")
          };

          lp.parseNew = function() {
            var node = this.startNode(), startIndent = this.curIndent, line = this.curLineStart;
            var meta = this.parseIdent(true);
            if (this.options.ecmaVersion >= 6 && this.eat(acorn.tokTypes.dot)) {
              node.meta = meta;
              node.property = this.parseIdent(true);
              return this.finishNode(node, "MetaProperty")
            }
            var start = this.storeCurrentPos();
            node.callee = this.parseSubscripts(this.parseExprAtom(), start, true, startIndent, line);
            if (this.tok.type === acorn.tokTypes.parenL) {
              node.arguments = this.parseExprList(acorn.tokTypes.parenR);
            } else {
              node.arguments = [];
            }
            return this.finishNode(node, "NewExpression")
          };

          lp.parseTemplateElement = function() {
            var elem = this.startNode();

            // The loose parser accepts invalid unicode escapes even in untagged templates.
            if (this.tok.type === acorn.tokTypes.invalidTemplate) {
              elem.value = {
                raw: this.tok.value,
                cooked: null
              };
            } else {
              elem.value = {
                raw: this.input.slice(this.tok.start, this.tok.end).replace(/\r\n?/g, "\n"),
                cooked: this.tok.value
              };
            }
            this.next();
            elem.tail = this.tok.type === acorn.tokTypes.backQuote;
            return this.finishNode(elem, "TemplateElement")
          };

          lp.parseTemplate = function() {
            var node = this.startNode();
            this.next();
            node.expressions = [];
            var curElt = this.parseTemplateElement();
            node.quasis = [curElt];
            while (!curElt.tail) {
              this.next();
              node.expressions.push(this.parseExpression());
              if (this.expect(acorn.tokTypes.braceR)) {
                curElt = this.parseTemplateElement();
              } else {
                curElt = this.startNode();
                curElt.value = {cooked: "", raw: ""};
                curElt.tail = true;
                this.finishNode(curElt, "TemplateElement");
              }
              node.quasis.push(curElt);
            }
            this.expect(acorn.tokTypes.backQuote);
            return this.finishNode(node, "TemplateLiteral")
          };

          lp.parseObj = function() {
            var node = this.startNode();
            node.properties = [];
            this.pushCx();
            var indent = this.curIndent + 1, line = this.curLineStart;
            this.eat(acorn.tokTypes.braceL);
            if (this.curIndent + 1 < indent) { indent = this.curIndent; line = this.curLineStart; }
            while (!this.closes(acorn.tokTypes.braceR, indent, line)) {
              var prop = this.startNode(), isGenerator = (void 0), isAsync = (void 0), start = (void 0);
              if (this.options.ecmaVersion >= 9 && this.eat(acorn.tokTypes.ellipsis)) {
                prop.argument = this.parseMaybeAssign();
                node.properties.push(this.finishNode(prop, "SpreadElement"));
                this.eat(acorn.tokTypes.comma);
                continue
              }
              if (this.options.ecmaVersion >= 6) {
                start = this.storeCurrentPos();
                prop.method = false;
                prop.shorthand = false;
                isGenerator = this.eat(acorn.tokTypes.star);
              }
              this.parsePropertyName(prop);
              if (this.toks.isAsyncProp(prop)) {
                isAsync = true;
                isGenerator = this.options.ecmaVersion >= 9 && this.eat(acorn.tokTypes.star);
                this.parsePropertyName(prop);
              } else {
                isAsync = false;
              }
              if (isDummy(prop.key)) { if (isDummy(this.parseMaybeAssign())) { this.next(); } this.eat(acorn.tokTypes.comma); continue }
              if (this.eat(acorn.tokTypes.colon)) {
                prop.kind = "init";
                prop.value = this.parseMaybeAssign();
              } else if (this.options.ecmaVersion >= 6 && (this.tok.type === acorn.tokTypes.parenL || this.tok.type === acorn.tokTypes.braceL)) {
                prop.kind = "init";
                prop.method = true;
                prop.value = this.parseMethod(isGenerator, isAsync);
              } else if (this.options.ecmaVersion >= 5 && prop.key.type === "Identifier" &&
                         !prop.computed && (prop.key.name === "get" || prop.key.name === "set") &&
                         (this.tok.type !== acorn.tokTypes.comma && this.tok.type !== acorn.tokTypes.braceR && this.tok.type !== acorn.tokTypes.eq)) {
                prop.kind = prop.key.name;
                this.parsePropertyName(prop);
                prop.value = this.parseMethod(false);
              } else {
                prop.kind = "init";
                if (this.options.ecmaVersion >= 6) {
                  if (this.eat(acorn.tokTypes.eq)) {
                    var assign = this.startNodeAt(start);
                    assign.operator = "=";
                    assign.left = prop.key;
                    assign.right = this.parseMaybeAssign();
                    prop.value = this.finishNode(assign, "AssignmentExpression");
                  } else {
                    prop.value = prop.key;
                  }
                } else {
                  prop.value = this.dummyIdent();
                }
                prop.shorthand = true;
              }
              node.properties.push(this.finishNode(prop, "Property"));
              this.eat(acorn.tokTypes.comma);
            }
            this.popCx();
            if (!this.eat(acorn.tokTypes.braceR)) {
              // If there is no closing brace, make the node span to the start
              // of the next token (this is useful for Tern)
              this.last.end = this.tok.start;
              if (this.options.locations) { this.last.loc.end = this.tok.loc.start; }
            }
            return this.finishNode(node, "ObjectExpression")
          };

          lp.parsePropertyName = function(prop) {
            if (this.options.ecmaVersion >= 6) {
              if (this.eat(acorn.tokTypes.bracketL)) {
                prop.computed = true;
                prop.key = this.parseExpression();
                this.expect(acorn.tokTypes.bracketR);
                return
              } else {
                prop.computed = false;
              }
            }
            var key = (this.tok.type === acorn.tokTypes.num || this.tok.type === acorn.tokTypes.string) ? this.parseExprAtom() : this.parseIdent();
            prop.key = key || this.dummyIdent();
          };

          lp.parsePropertyAccessor = function() {
            if (this.tok.type === acorn.tokTypes.name || this.tok.type.keyword) { return this.parseIdent() }
            if (this.tok.type === acorn.tokTypes.privateId) { return this.parsePrivateIdent() }
          };

          lp.parseIdent = function() {
            var name = this.tok.type === acorn.tokTypes.name ? this.tok.value : this.tok.type.keyword;
            if (!name) { return this.dummyIdent() }
            if (this.tok.type.keyword) { this.toks.type = acorn.tokTypes.name; }
            var node = this.startNode();
            this.next();
            node.name = name;
            return this.finishNode(node, "Identifier")
          };

          lp.parsePrivateIdent = function() {
            var node = this.startNode();
            node.name = this.tok.value;
            this.next();
            return this.finishNode(node, "PrivateIdentifier")
          };

          lp.initFunction = function(node) {
            node.id = null;
            node.params = [];
            if (this.options.ecmaVersion >= 6) {
              node.generator = false;
              node.expression = false;
            }
            if (this.options.ecmaVersion >= 8)
              { node.async = false; }
          };

          // Convert existing expression atom to assignable pattern
          // if possible.

          lp.toAssignable = function(node, binding) {
            if (!node || node.type === "Identifier" || (node.type === "MemberExpression" && !binding)) ; else if (node.type === "ParenthesizedExpression") {
              this.toAssignable(node.expression, binding);
            } else if (this.options.ecmaVersion < 6) {
              return this.dummyIdent()
            } else if (node.type === "ObjectExpression") {
              node.type = "ObjectPattern";
              for (var i = 0, list = node.properties; i < list.length; i += 1)
                {
                var prop = list[i];

                this.toAssignable(prop, binding);
              }
            } else if (node.type === "ArrayExpression") {
              node.type = "ArrayPattern";
              this.toAssignableList(node.elements, binding);
            } else if (node.type === "Property") {
              this.toAssignable(node.value, binding);
            } else if (node.type === "SpreadElement") {
              node.type = "RestElement";
              this.toAssignable(node.argument, binding);
            } else if (node.type === "AssignmentExpression") {
              node.type = "AssignmentPattern";
              delete node.operator;
            } else {
              return this.dummyIdent()
            }
            return node
          };

          lp.toAssignableList = function(exprList, binding) {
            for (var i = 0, list = exprList; i < list.length; i += 1)
              {
              var expr = list[i];

              this.toAssignable(expr, binding);
            }
            return exprList
          };

          lp.parseFunctionParams = function(params) {
            params = this.parseExprList(acorn.tokTypes.parenR);
            return this.toAssignableList(params, true)
          };

          lp.parseMethod = function(isGenerator, isAsync) {
            var node = this.startNode(), oldInAsync = this.inAsync, oldInGenerator = this.inGenerator, oldInFunction = this.inFunction;
            this.initFunction(node);
            if (this.options.ecmaVersion >= 6)
              { node.generator = !!isGenerator; }
            if (this.options.ecmaVersion >= 8)
              { node.async = !!isAsync; }
            this.inAsync = node.async;
            this.inGenerator = node.generator;
            this.inFunction = true;
            node.params = this.parseFunctionParams();
            node.body = this.parseBlock();
            this.toks.adaptDirectivePrologue(node.body.body);
            this.inAsync = oldInAsync;
            this.inGenerator = oldInGenerator;
            this.inFunction = oldInFunction;
            return this.finishNode(node, "FunctionExpression")
          };

          lp.parseArrowExpression = function(node, params, isAsync) {
            var oldInAsync = this.inAsync, oldInGenerator = this.inGenerator, oldInFunction = this.inFunction;
            this.initFunction(node);
            if (this.options.ecmaVersion >= 8)
              { node.async = !!isAsync; }
            this.inAsync = node.async;
            this.inGenerator = false;
            this.inFunction = true;
            node.params = this.toAssignableList(params, true);
            node.expression = this.tok.type !== acorn.tokTypes.braceL;
            if (node.expression) {
              node.body = this.parseMaybeAssign();
            } else {
              node.body = this.parseBlock();
              this.toks.adaptDirectivePrologue(node.body.body);
            }
            this.inAsync = oldInAsync;
            this.inGenerator = oldInGenerator;
            this.inFunction = oldInFunction;
            return this.finishNode(node, "ArrowFunctionExpression")
          };

          lp.parseExprList = function(close, allowEmpty) {
            this.pushCx();
            var indent = this.curIndent, line = this.curLineStart, elts = [];
            this.next(); // Opening bracket
            while (!this.closes(close, indent + 1, line)) {
              if (this.eat(acorn.tokTypes.comma)) {
                elts.push(allowEmpty ? null : this.dummyIdent());
                continue
              }
              var elt = this.parseMaybeAssign();
              if (isDummy(elt)) {
                if (this.closes(close, indent, line)) { break }
                this.next();
              } else {
                elts.push(elt);
              }
              this.eat(acorn.tokTypes.comma);
            }
            this.popCx();
            if (!this.eat(close)) {
              // If there is no closing brace, make the node span to the start
              // of the next token (this is useful for Tern)
              this.last.end = this.tok.start;
              if (this.options.locations) { this.last.loc.end = this.tok.loc.start; }
            }
            return elts
          };

          lp.parseAwait = function() {
            var node = this.startNode();
            this.next();
            node.argument = this.parseMaybeUnary();
            return this.finishNode(node, "AwaitExpression")
          };

          // Acorn: Loose parser
          //
          // This module provides an alternative parser that exposes that same
          // interface as the main module's `parse` function, but will try to
          // parse anything as JavaScript, repairing syntax error the best it
          // can. There are circumstances in which it will raise an error and
          // give up, but they are very rare. The resulting AST will be a mostly
          // valid JavaScript AST (as per the [ESTree spec][estree], except
          // that:
          //
          // - Return outside functions is allowed
          //
          // - Label consistency (no conflicts, break only to existing labels)
          //   is not enforced.
          //
          // - Bogus Identifier nodes with a name of `"✖"` are inserted whenever
          //   the parser got too confused to return anything meaningful.
          //
          // [estree]: https://github.com/estree/estree
          //
          // The expected use for this is to *first* try `acorn.parse`, and only
          // if that fails switch to the loose parser. The loose parser might
          // parse badly indented code incorrectly, so **don't** use it as your
          // default parser.
          //
          // Quite a lot of acorn.js is duplicated here. The alternative was to
          // add a *lot* of extra cruft to that file, making it less readable
          // and slower. Copying and editing the code allowed me to make
          // invasive changes and simplifications without creating a complicated
          // tangle.


          acorn.defaultOptions.tabSize = 4;

          function parse(input, options) {
            return LooseParser.parse(input, options)
          }

          exports.LooseParser = LooseParser;
          exports.isDummy = isDummy;
          exports.parse = parse;

        })); 
    } (acornLoose$1, acornLoose$1.exports));
    return acornLoose$1.exports;
}

var hasRequiredAst;
function requireAst() {
  if (hasRequiredAst) return ast;
  hasRequiredAst = 1;
  var __importDefault = ast && ast.__importDefault || function(mod) {
    return mod && mod.__esModule ? mod : { "default": mod };
  };
  Object.defineProperty(ast, "__esModule", { value: true });
  ast.parseAcornLoose = parseAcornLoose;
  ast.parseAcorn = parseAcorn;
  ast.parseOXC = parseOXC;
  ast.parseBabel = parseBabel;
  ast.sanitisePosition = sanitisePosition;
  ast.parseAST = parseAST;
  const oxc_parser_1 = __importDefault(require$$0);
  const acorn_loose_1 = requireAcornLoose();
  const acorn_1 = requireAcorn();
  const compiler_sfc_1 = require$$1;
  function parseAcornLoose(source) {
    const ast2 = (0, acorn_loose_1.parse)(source, {
      ecmaVersion: "latest",
      allowAwaitOutsideFunction: true,
      sourceType: "module"
    });
    return ast2;
  }
  function parseAcorn(source) {
    const ast2 = (0, acorn_1.parse)(source, {
      ecmaVersion: "latest"
    });
    return ast2;
  }
  function parseOXC(source, options) {
    return oxc_parser_1.default.parseSync("index.ts", source, options);
  }
  function parseBabel(source, options) {
    return (0, compiler_sfc_1.babelParse)(source, options);
  }
  function sanitisePosition(source) {
    return source.replace(/[^\x00-\x7F]/g, "*");
  }
  function langFilename(filename) {
    const ext = filename.split(".").pop();
    switch (ext) {
      case "tsx":
      case "jsx":
      case "ts":
      case "js":
        return ext;
    }
    throw new Error("Unknown extension: " + ext);
  }
  function parseAST(source, sourceFilename = "index.ts") {
    const normaliseSource = source;
    let ast2;
    try {
      const result = oxc_parser_1.default.parseSync(sourceFilename, normaliseSource, {
        lang: langFilename(sourceFilename)
      });
      if (result.errors.length) {
        throw result.errors;
      } else {
        ast2 = result.program;
      }
    } catch (e) {
      console.error("oxc parser failed :(", e, sourceFilename);
      ast2 = parseAcornLoose(normaliseSource);
    }
    return ast2;
  }
  return ast;
}

var hasRequiredUtils$2;
function requireUtils$2() {
  if (hasRequiredUtils$2) return utils$2;
  hasRequiredUtils$2 = 1;
  Object.defineProperty(utils$2, "__esModule", { value: true });
  utils$2.retrieveBindings = retrieveBindings;
  utils$2.getASTBindings = getASTBindings;
  utils$2.createTemplateTypeMap = createTemplateTypeMap;
  utils$2.templateItemsToMap = templateItemsToMap;
  const compiler_core_1 = require$$0$1;
  const compiler_sfc_1 = require$$1;
  const node_js_1 = requireNode();
  const ast_js_1 = requireAst();
  const Keywords = "break,case,catch,class,const,continue,debugger,default,delete,do,else,export,extends,false,finally,for,function,if,import,in,instanceof,new,null,return,super,switch,this,throw,true,try,typeof,var,void,while,with,await".split(",");
  const KeywordsInStrict = ["let", "static", "yield"];
  const Literals = "undefined,null,true,false".split(",");
  const BindingIgnoreArray = [...Keywords, ...KeywordsInStrict, ...Literals];
  const BidningIgnoreSet = new Set(BindingIgnoreArray);
  function retrieveBindings(exp, context, directive = null) {
    const bindings = [];
    if (exp.type !== compiler_core_1.NodeTypes.SIMPLE_EXPRESSION) {
      return bindings;
    }
    if (exp.isStatic || exp.ast === null) {
      const name = exp.content;
      bindings.push({
        type: "Binding",
        node: exp,
        name,
        parent: null,
        ignore: context.ignoredIdentifiers.includes(name) || exp.isStatic,
        directive,
        exp
      });
    } else if (exp.ast) {
      bindings.push(...getASTBindings(exp.ast, context, exp));
    } else {
      const ast = (0, ast_js_1.parseAcornLoose)(exp.content);
      if (ast) {
        bindings.push(...getASTBindings(ast, context, exp));
      } else {
        bindings.push({
          type: "Binding",
          node: exp,
          context,
          value: exp.content,
          invalid: true,
          ignore: false,
          name: void 0,
          parent: null,
          exp
        });
      }
    }
    return bindings;
  }
  function getASTBindings(ast, context, exp) {
    const bindings = [];
    const isAcorn = "type" in ast && ast.type === "Program";
    (0, compiler_sfc_1.walk)(ast, {
      enter(n, parent) {
        var _a;
        const ignoredIdentifiers = (
          // @ts-expect-error
          n._ignoredIdentifiers = [
            // @ts-expect-error
            ...(parent == null ? void 0 : parent._ignoredIdentifiers) || context.ignoredIdentifiers
          ]
        );
        switch (n.type) {
          case "FunctionDeclaration":
          case "FunctionExpression": {
            if (n.id) {
              ignoredIdentifiers.push(n.id.name);
            }
          }
          case "ArrowFunctionExpression": {
            const params = n.params;
            params.forEach((param) => {
              if (param.type === "Identifier") {
                ignoredIdentifiers.push(param.name);
              }
            });
            const pN = exp ? (0, node_js_1.patchBabelNodeLoc)(n, exp) : n;
            const bN = exp ? (0, node_js_1.patchBabelNodeLoc)(n.body, exp) : n;
            bindings.push({
              type: "Function",
              // @ts-expect-error not correct type
              node: pN,
              // @ts-expect-error not correct type
              body: bN,
              context
            });
            break;
          }
          case "Identifier": {
            const name = n.name;
            if (parent && ("property" in parent && parent.property === n || "key" in parent && parent.key === n)) {
              this.skip();
              return;
            }
            if (isAcorn && name === "\u2716") {
              this.skip();
              return;
            }
            const pNode = exp ? isAcorn ? (
              // @ts-expect-error not correct type
              (0, node_js_1.patchAcornNodeLoc)(n, exp)
            ) : (0, node_js_1.patchBabelNodeLoc)(n, exp) : n;
            bindings.push({
              type: "Binding",
              // @ts-expect-error not correct type
              node: pNode,
              name,
              // @ts-expect-error not correct type
              parent,
              directive: null,
              ignore: ignoredIdentifiers.includes(name) || BidningIgnoreSet.has(name),
              exp
            });
            break;
          }
          default: {
            if (n.type.endsWith("Literal")) {
              const pNode = exp && !isAcorn ? (0, node_js_1.patchBabelNodeLoc)(n, exp) : n;
              let content = "";
              let value = void 0;
              if ("value" in n) {
                content = `${n.value}`;
                value = n.value;
              }
              bindings.push({
                type: "Literal",
                content,
                value,
                // @ts-expect-error
                node: pNode
              });
            }
            if ("id" in n) {
              if (((_a = n.id) == null ? void 0 : _a.type) === "Identifier") {
                ignoredIdentifiers.push(n.id.name);
              }
            }
          }
        }
      },
      leave(n) {
        delete n._ignoredIdentifiers;
      }
    });
    return bindings;
  }
  function createTemplateTypeMap() {
    return {
      [
        "Condition"
        /* TemplateTypes.Condition */
      ]: [],
      [
        "Loop"
        /* TemplateTypes.Loop */
      ]: [],
      [
        "Element"
        /* TemplateTypes.Element */
      ]: [],
      [
        "Prop"
        /* TemplateTypes.Prop */
      ]: [],
      [
        "Binding"
        /* TemplateTypes.Binding */
      ]: [],
      [
        "SlotRender"
        /* TemplateTypes.SlotRender */
      ]: [],
      [
        "SlotDeclaration"
        /* TemplateTypes.SlotDeclaration */
      ]: [],
      [
        "Comment"
        /* TemplateTypes.Comment */
      ]: [],
      [
        "Text"
        /* TemplateTypes.Text */
      ]: [],
      [
        "Directive"
        /* TemplateTypes.Directive */
      ]: [],
      [
        "Interpolation"
        /* TemplateTypes.Interpolation */
      ]: [],
      [
        "Function"
        /* TemplateTypes.Function */
      ]: [],
      [
        "Literal"
        /* TemplateTypes.Literal */
      ]: []
    };
  }
  function templateItemsToMap(items) {
    const map = createTemplateTypeMap();
    for (const item of items) {
      map[item.type].push(item);
    }
    return map;
  }
  return utils$2;
}

var hasRequiredSetup$1;
function requireSetup$1() {
  if (hasRequiredSetup$1) return setup;
  hasRequiredSetup$1 = 1;
  Object.defineProperty(setup, "__esModule", { value: true });
  setup.handleSetupNode = handleSetupNode;
  setup.createSetupContext = createSetupContext;
  const utils_1 = requireUtils$2();
  const compiler_sfc_1 = require$$1;
  function handleSetupNode(pnode, shallowCb, isOptions = false) {
    const items = [];
    let isAsync = false;
    let trackDeclarations = true;
    (0, compiler_sfc_1.walk)(pnode, {
      enter(node, parent) {
        var _a;
        if (parent === pnode && shallowCb) {
          const r = shallowCb(node);
          if (Array.isArray(r)) {
            items.push(...r);
          }
        }
        switch (node.type) {
          case "ImportDeclaration":
          case "FunctionDeclaration":
          case "FunctionExpression":
          case "ArrowFunctionExpression":
            this.skip();
            return;
          case "AwaitExpression": {
            isAsync = true;
            items.push({
              type: "Async",
              isAsync: true,
              node
            });
            break;
          }
          case "CallExpression": {
            items.push({
              type: "FunctionCall",
              node,
              parent,
              name: node.callee.type === "Identifier" ? node.callee.name : node.callee.type === "MemberExpression" ? (_a = node.callee.property) == null ? void 0 : _a.name : ""
            });
            this.skip();
            break;
          }
          case "VariableDeclaration": {
            if (!trackDeclarations) {
              this.skip();
              break;
            }
            const declarations = node.declarations.flatMap((x) => {
              var _a2;
              const bindings = (0, utils_1.getASTBindings)(x.id, {
                ignoredIdentifiers: []
              });
              if (((_a2 = x.init) == null ? void 0 : _a2.type) === "AwaitExpression") {
                isAsync = true;
                items.push({
                  type: "Async",
                  isAsync: true,
                  node: x.init
                });
              }
              return bindings.filter((x2) => {
                var _a3;
                return x2.type === "Binding" && !((_a3 = x2.parent) == null ? void 0 : _a3.type.startsWith("TS"));
              }).map((b) => {
                return {
                  type: "Declaration",
                  node: b.node,
                  name: b.name,
                  declarator: node,
                  parent: x,
                  rest: false
                };
              });
            });
            this.skip();
            items.push(...declarations);
            break;
          }
          case "ExportDefaultDeclaration": {
            items.push({
              type: "Error",
              node,
              message: "EXPORT_DEFAULT_SETUP",
              loc: null
            });
            this.skip();
            break;
          }
          case "ReturnStatement": {
            if (isOptions) {
              return [];
            }
            items.push({
              type: "Error",
              node,
              message: "NO_RETURN_IN_SETUP",
              loc: null
            });
            this.skip();
            break;
          }
          // if any variable is declared in a block or something it shouldn't be tracked
          case "ForInStatement":
          case "ForOfStatement":
          case "ForStatement":
          case "WhileStatement":
          case "DoWhileStatement":
          case "IfStatement":
          case "SwitchStatement":
          case "TryStatement":
          case "BlockStatement": {
            trackDeclarations = false;
            break;
          }
        }
      },
      leave(node, parent) {
        switch (node.type) {
          case "ForInStatement":
          case "ForOfStatement":
          case "ForStatement":
          case "WhileStatement":
          case "DoWhileStatement":
          case "IfStatement":
          case "SwitchStatement":
          case "TryStatement":
          case "BlockStatement": {
            trackDeclarations = true;
            break;
          }
        }
      }
    });
    return {
      items,
      isAsync
    };
  }
  function createSetupContext(opts) {
    let trackDeclarations = true;
    let isAsync = false;
    let ignore = false;
    const ignoreNodeTypes = /* @__PURE__ */ new Set([
      "ImportDeclaration",
      "FunctionDeclaration",
      "FunctionExpression",
      "ArrowFunctionExpression",
      "CallExpression",
      "VariableDeclaration",
      "ExportDefaultDeclaration",
      "ReturnStatement"
    ]);
    function visit(node, parent, key) {
      if (ignore) {
        return;
      }
      switch (node.type) {
        case "AwaitExpression": {
          isAsync = true;
          return {
            type: "Async",
            isAsync: true,
            node
          };
        }
        case "CallExpression": {
          return {
            type: "FunctionCall",
            node,
            parent,
            name: node.callee.type === "Identifier" ? node.callee.name : node.callee.type === "MemberExpression" ? node.callee.property.type === "Identifier" ? node.callee.property.name : "" : ""
          };
        }
        case "VariableDeclaration": {
          if (!trackDeclarations) {
            return;
          }
          const declarations = node.declarations.flatMap((x) => {
            var _a;
            const bindings = (0, utils_1.getASTBindings)(x.id, {
              ignoredIdentifiers: []
            });
            const items = bindings.filter((x2) => {
              var _a2;
              return x2.type === "Binding" && !((_a2 = x2.parent) == null ? void 0 : _a2.type.startsWith("TS"));
            }).map((b) => {
              return {
                type: "Declaration",
                node: b.node,
                name: b.name,
                declarator: node,
                parent: x,
                rest: false
              };
            });
            if (((_a = x.init) == null ? void 0 : _a.type) === "AwaitExpression") {
              isAsync = true;
              items.unshift({
                type: "Async",
                isAsync: true,
                node: x.init
              });
            }
            return items;
          });
          return declarations;
        }
        case "ExportDefaultDeclaration": {
          return {
            type: "Error",
            node,
            message: "EXPORT_DEFAULT_SETUP",
            loc: null
          };
        }
        case "ReturnStatement": {
          return {
            type: "Error",
            node,
            message: "NO_RETURN_IN_SETUP",
            loc: null
          };
        }
        // if any variable is declared in a block or something it shouldn't be tracked
        case "ForInStatement":
        case "ForOfStatement":
        case "ForStatement":
        case "WhileStatement":
        case "DoWhileStatement":
        case "IfStatement":
        case "SwitchStatement":
        case "TryStatement":
        case "BlockStatement": {
          trackDeclarations = false;
          return;
        }
      }
      if (ignoreNodeTypes.has(node.type)) {
        ignore = true;
      }
    }
    function leave(node, parent, key) {
      if (ignoreNodeTypes.has(node.type)) {
        ignore = false;
      }
      switch (node.type) {
        case "ForInStatement":
        case "ForOfStatement":
        case "ForStatement":
        case "WhileStatement":
        case "DoWhileStatement":
        case "IfStatement":
        case "SwitchStatement":
        case "TryStatement":
        case "BlockStatement": {
          trackDeclarations = true;
          break;
        }
      }
    }
    return {
      visit,
      leave,
      get isAsync() {
        return isAsync;
      }
    };
  }
  return setup;
}

var hasRequiredSetup;
function requireSetup() {
  if (hasRequiredSetup) return setup$1;
  hasRequiredSetup = 1;
  (function(exports) {
    var __createBinding = setup$1 && setup$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = setup$1 && setup$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireSetup$1(), exports);
  })(setup$1);
  return setup$1;
}

var hasRequiredOptions$1;
function requireOptions$1() {
  if (hasRequiredOptions$1) return options;
  hasRequiredOptions$1 = 1;
  Object.defineProperty(options, "__esModule", { value: true });
  options.handleOptionsNode = handleOptionsNode;
  options.createOptionsContext = createOptionsContext;
  const setup_1 = requireSetup();
  function handleOptionsNode(node) {
    switch (node.type) {
      case "ExportDefaultDeclaration": {
        let objectExpression = null;
        switch (node.declaration.type) {
          case "CallExpression": {
            if (node.declaration.arguments[0].type === "ObjectExpression") {
              objectExpression = node.declaration.arguments[0];
            }
            break;
          }
          case "ObjectExpression": {
            objectExpression = node.declaration;
            break;
          }
        }
        if (objectExpression) {
          return handleObjectDeclaration(objectExpression);
        }
        return [];
      }
      default:
        return [];
    }
  }
  function handleObjectDeclaration(node) {
    const items = [];
    let isAsync = false;
    for (const property of node.properties) {
      switch (property.type) {
        case "SpreadElement": {
          break;
        }
        case "Property": {
          const key = property.key;
          const value = property.value;
          if (key.type === "Identifier") {
            switch (key.name) {
              case "setup": {
                if (value.type === "FunctionExpression") {
                  isAsync = value.async;
                  if (value.body) {
                    const result = (0, setup_1.handleSetupNode)(value.body, void 0, true);
                    if (Array.isArray(result)) {
                      items.push(...result);
                    } else {
                      items.push(...result.items);
                    }
                  }
                }
                break;
              }
            }
          }
        }
      }
    }
    return { items, isAsync };
  }
  function createOptionsContext(opts) {
    let track = false;
    let objectExpression = null;
    let setupFunction = null;
    function visit(node, parent, key) {
      if (node.type === "ExportDefaultDeclaration") {
        track = true;
        return {
          type: "DefaultExport",
          node
        };
      }
      if (!track)
        return;
      if (setupFunction) {
        return opts.setupCtx.visit(node, parent, key);
      }
      switch (node.type) {
        case "ObjectExpression": {
          if (objectExpression)
            return;
          objectExpression = node;
          break;
        }
        case "CallExpression": {
          if (!objectExpression) {
            if (node.arguments[0].type === "ObjectExpression") {
              objectExpression = node.arguments[0];
            }
          }
          return;
        }
        case "FunctionExpression": {
          if (!setupFunction && key === "setup") {
            setupFunction = node;
          }
          return;
        }
      }
    }
    function leave(node, parent, key) {
      switch (node.type) {
        case "ExportDefaultDeclaration": {
          track = false;
          break;
        }
        case "FunctionExpression": {
          if (node === setupFunction) {
            setupFunction = null;
          }
          break;
        }
      }
      if (setupFunction) {
        opts.setupCtx.leave(node, parent, key);
      }
    }
    return {
      visit,
      leave
    };
  }
  return options;
}

var hasRequiredOptions;
function requireOptions() {
  if (hasRequiredOptions) return options$1;
  hasRequiredOptions = 1;
  (function(exports) {
    var __createBinding = options$1 && options$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = options$1 && options$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireOptions$1(), exports);
  })(options$1);
  return options$1;
}

var hasRequiredScript$3;
function requireScript$3() {
  if (hasRequiredScript$3) return script$3;
  hasRequiredScript$3 = 1;
  Object.defineProperty(script$3, "__esModule", { value: true });
  script$3.parseScript = parseScript;
  script$3.parseScriptBetter = parseScriptBetter;
  const walk_1 = requireWalk();
  const index_js_1 = requireShared();
  const index_js_2 = requireOptions();
  const index_js_3 = requireSetup();
  function parseScript(ast, attrs) {
    let isAsync = false;
    const items = [];
    const isSetup = !!attrs.setup;
    attrs.lang || "js";
    if (isSetup) {
      return (0, index_js_3.handleSetupNode)(ast, index_js_1.handleShared);
    } else {
      (0, walk_1.shallowWalk)(ast, (node) => {
        const shared = (0, index_js_1.handleShared)(node);
        if (shared) {
          items.push(...shared);
        }
        const result = (0, index_js_2.handleOptionsNode)(node);
        if (Array.isArray(result)) {
          items.push(...result);
        } else {
          isAsync = result.isAsync;
          items.push(...result.items);
        }
      });
    }
    return {
      isAsync,
      items
    };
  }
  function parseScriptBetter(ast, attrs) {
    const lang = (typeof attrs.lang === "string" ? attrs.lang : "js") || "js";
    const isSetup = !!attrs.setup;
    const setupCtx = (0, index_js_3.createSetupContext)({ lang });
    const sharedCtx = (0, index_js_1.createSharedContext)({ lang });
    const optionsCtx = (0, index_js_2.createOptionsContext)({ lang, setupCtx });
    const visitorCtx = isSetup ? setupCtx : optionsCtx;
    const items = [];
    function addResult(result) {
      if (!result)
        return;
      if (Array.isArray(result)) {
        items.push(...result);
      } else {
        items.push(result);
      }
    }
    (0, walk_1.deepWalk)(ast, (node, parent) => {
      const shared = sharedCtx.visit(node, parent);
      const visit = visitorCtx.visit(node, parent);
      addResult(shared);
      addResult(visit);
    }, (node, parent, key) => {
      visitorCtx.leave(node, parent, key);
      sharedCtx.leave(node, parent, key);
    });
    return {
      isAsync: setupCtx.isAsync,
      items
    };
  }
  return script$3;
}

var template$5 = {};

var comment$2 = {};

var hasRequiredComment$2;
function requireComment$2() {
  if (hasRequiredComment$2) return comment$2;
  hasRequiredComment$2 = 1;
  Object.defineProperty(comment$2, "__esModule", { value: true });
  comment$2.handleComment = handleComment;
  const compiler_core_1 = require$$0$1;
  function handleComment(node) {
    if (node.type !== compiler_core_1.NodeTypes.COMMENT) {
      return null;
    }
    return {
      type: "Comment",
      content: node.content,
      node
    };
  }
  return comment$2;
}

var interpolation$2 = {};

var hasRequiredInterpolation$2;
function requireInterpolation$2() {
  if (hasRequiredInterpolation$2) return interpolation$2;
  hasRequiredInterpolation$2 = 1;
  Object.defineProperty(interpolation$2, "__esModule", { value: true });
  interpolation$2.handleInterpolation = handleInterpolation;
  const compiler_core_1 = require$$0$1;
  const utils_1 = requireUtils$2();
  function handleInterpolation(node, context) {
    if (node.type !== compiler_core_1.NodeTypes.INTERPOLATION) {
      return null;
    }
    const interpolation2 = {
      type: "Interpolation",
      node
    };
    return [interpolation2, ...(0, utils_1.retrieveBindings)(node.content, context)];
  }
  return interpolation$2;
}

var element$2 = {};

var conditions$1 = {};

var conditions = {};

var hasRequiredConditions$1;
function requireConditions$1() {
  if (hasRequiredConditions$1) return conditions;
  hasRequiredConditions$1 = 1;
  Object.defineProperty(conditions, "__esModule", { value: true });
  conditions.handleConditions = handleConditions;
  const compiler_core_1 = require$$0$1;
  const utils_1 = requireUtils$2();
  function handleConditions(node, parent, parentContext) {
    if (node.type !== compiler_core_1.NodeTypes.ELEMENT) {
      return null;
    }
    const prop = node.props.find((prop2) => prop2.type === compiler_core_1.NodeTypes.DIRECTIVE && (prop2.name === "if" || prop2.name === "else-if" || prop2.name === "else"));
    if (!prop) {
      return null;
    }
    const items = [];
    const bindings = [];
    const siblings = [];
    if (prop.name !== "if") {
      const children = "children" in parent ? parent.children : [];
      for (let i = 0; i < children.length; i++) {
        const element = children[i];
        if (element === node) {
          break;
        }
        const condition2 = handleConditions(element, parent, parentContext);
        if (condition2) {
          siblings.push(condition2.condition);
        }
      }
    }
    const condition = {
      type: "Condition",
      node: prop,
      bindings,
      element: node,
      parent,
      context: parentContext,
      siblings
    };
    const context = {
      ...parentContext,
      conditions: [...parentContext.conditions, condition]
    };
    items.push(condition);
    if (prop.exp) {
      const all = (0, utils_1.retrieveBindings)(prop.exp, context);
      bindings.push(...all.filter(
        (x) => x.type === "Binding"
        /* TemplateTypes.Binding */
      ));
      items.push(...all);
    }
    return {
      items,
      condition,
      context
    };
  }
  return conditions;
}

var hasRequiredConditions;
function requireConditions() {
  if (hasRequiredConditions) return conditions$1;
  hasRequiredConditions = 1;
  (function(exports) {
    var __createBinding = conditions$1 && conditions$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = conditions$1 && conditions$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireConditions$1(), exports);
  })(conditions$1);
  return conditions$1;
}

var loops$1 = {};

var loops = {};

var hasRequiredLoops$1;
function requireLoops$1() {
  if (hasRequiredLoops$1) return loops;
  hasRequiredLoops$1 = 1;
  Object.defineProperty(loops, "__esModule", { value: true });
  loops.handleLoopProp = handleLoopProp;
  const compiler_core_1 = require$$0$1;
  const utils_1 = requireUtils$2();
  function handleLoopProp(node, parent, parentContext) {
    if (node.type !== compiler_core_1.NodeTypes.ELEMENT) {
      return null;
    }
    const forProp = node.props.find((x) => x.type === compiler_core_1.NodeTypes.DIRECTIVE && "forParseResult" in x);
    if (!forProp) {
      return null;
    }
    const sourceBindings = forProp.forParseResult.source ? (0, utils_1.retrieveBindings)(forProp.forParseResult.source, parentContext) : [];
    const keyBindings = forProp.forParseResult.key ? (0, utils_1.retrieveBindings)(forProp.forParseResult.key, parentContext) : [];
    const valueBindings = forProp.forParseResult.value ? (0, utils_1.retrieveBindings)(forProp.forParseResult.value, parentContext) : [];
    const indexBindings = forProp.forParseResult.index ? (0, utils_1.retrieveBindings)(forProp.forParseResult.index, parentContext) : [];
    const items = [];
    const toAddIgnoredIdentifiers = [
      ...valueBindings.filter(
        (x) => x.type === "Binding"
        /* TemplateTypes.Binding */
      ).map((x) => x.name),
      ...keyBindings.filter(
        (x) => x.type === "Binding"
        /* TemplateTypes.Binding */
      ).map((x) => x.name),
      ...indexBindings.filter(
        (x) => x.type === "Binding"
        /* TemplateTypes.Binding */
      ).map((x) => x.name)
    ];
    const context = toAddIgnoredIdentifiers.length > 0 ? {
      ...parentContext,
      ignoredIdentifiers: [
        ...parentContext.ignoredIdentifiers,
        ...toAddIgnoredIdentifiers
      ],
      inFor: true
    } : {
      ...parentContext,
      inFor: true
    };
    const loop = {
      type: "Loop",
      node: forProp,
      element: node,
      parent,
      context: {
        ...parentContext
        // blockDirection: "Right",
      }
    };
    items.push(loop);
    items.push(...sourceBindings);
    return {
      items,
      loop,
      context
    };
  }
  return loops;
}

var hasRequiredLoops;
function requireLoops() {
  if (hasRequiredLoops) return loops$1;
  hasRequiredLoops = 1;
  (function(exports) {
    var __createBinding = loops$1 && loops$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = loops$1 && loops$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireLoops$1(), exports);
  })(loops$1);
  return loops$1;
}

var props$1 = {};

var props = {};

var hasRequiredProps$1;
function requireProps$1() {
  if (hasRequiredProps$1) return props;
  hasRequiredProps$1 = 1;
  Object.defineProperty(props, "__esModule", { value: true });
  props.handleProps = handleProps;
  props.propToTemplateProp = propToTemplateProp;
  const compiler_core_1 = require$$0$1;
  const utils_1 = requireUtils$2();
  function handleProps(node, context) {
    var _a, _b;
    if (node.type !== compiler_core_1.NodeTypes.ELEMENT) {
      return null;
    }
    const items = [];
    const toNormalise = {
      classes: [],
      styles: []
    };
    for (const prop of node.props) {
      switch (prop.name) {
        case "style":
          toNormalise.styles.push(prop);
          break;
        case "class":
          toNormalise.classes.push(prop);
          break;
        case "on":
        case "bind": {
          if (prop.type === compiler_core_1.NodeTypes.DIRECTIVE) {
            if (((_a = prop.arg) == null ? void 0 : _a.content) === "class") {
              toNormalise.classes.push(prop);
              break;
            }
            if (((_b = prop.arg) == null ? void 0 : _b.content) === "style") {
              toNormalise.styles.push(prop);
              break;
            }
            items.push(...propToTemplateProp(prop, node, context));
          }
          break;
        }
        case "if":
        case "else-if":
        case "else":
        case "for":
        case "slot": {
          if (prop.type === compiler_core_1.NodeTypes.DIRECTIVE) {
            break;
          }
        }
        default:
          if (prop.name === "is" && node.tag === "component") {
            break;
          }
          items.push(...propToTemplateProp(prop, node, context));
      }
    }
    if (toNormalise.classes.length > 0) {
      const props2 = [];
      const bindings = [];
      for (let i = 0; i < toNormalise.classes.length; i++) {
        const [p, ...b] = propToTemplateProp(toNormalise.classes[i], node, context);
        props2.push(p);
        bindings.push(...b);
      }
      items.push({
        type: "Prop",
        node: null,
        name: "class",
        props: props2,
        context
      });
      items.push(...bindings);
    }
    if (toNormalise.styles.length > 0) {
      const props2 = [];
      const bindings = [];
      for (let i = 0; i < toNormalise.styles.length; i++) {
        const [p, ...b] = propToTemplateProp(toNormalise.styles[i], node, context);
        props2.push(p);
        bindings.push(...b);
      }
      items.push({
        type: "Prop",
        node: null,
        name: "style",
        props: props2,
        context
      });
      items.push(...bindings);
    }
    const tagEnd = (node.isSelfClosing ? node.loc.end.offset : node.children.length > 0 ? node.children[node.children.length - 1].loc.end.offset : node.loc.end.offset) - node.loc.start.offset;
    const hasPre = node.loc.source.slice(0, tagEnd).indexOf("v-pre");
    if (hasPre !== -1) {
      items.push({
        type: "Prop",
        name: "pre",
        arg: null,
        exp: null,
        static: true,
        node: {
          loc: {
            start: { offset: hasPre + node.loc.start.offset },
            end: { offset: hasPre + 5 + node.loc.start.offset }
          }
        },
        context
      });
    }
    return items;
  }
  const BuiltInDirectivesAsProps = /* @__PURE__ */ new Set([
    "bind",
    "on",
    "text",
    "html",
    "show",
    "pre",
    "once",
    "memo",
    "cloak"
  ]);
  function propToTemplateProp(prop, element, context) {
    var _a;
    if (prop.type === compiler_core_1.NodeTypes.ATTRIBUTE) {
      return [
        {
          type: "Prop",
          node: prop,
          name: prop.name,
          value: ((_a = prop.value) == null ? void 0 : _a.content) ?? null,
          static: true,
          event: false,
          element,
          items: []
        }
      ];
    } else if (BuiltInDirectivesAsProps.has(prop.name)) {
      const nameBinding = prop.arg ? (0, utils_1.retrieveBindings)(prop.arg, context, prop) : [];
      const valueBinding = prop.exp ? (0, utils_1.retrieveBindings)(prop.exp, context) : [];
      if (!prop.exp) {
        nameBinding.filter(
          (x) => x.type === "Binding"
          /* TemplateTypes.Binding */
        ).forEach((x) => {
          x.ignore = false;
          x.skip = true;
        });
      }
      return [
        {
          type: "Prop",
          node: prop,
          arg: prop.arg ? nameBinding.filter(
            (x) => x.type === "Binding"
            /* TemplateTypes.Binding */
          ) : null,
          exp: prop.exp ? valueBinding.filter(
            (x) => x.type === "Binding"
            /* TemplateTypes.Binding */
          ) : null,
          static: false,
          event: prop.name === "on",
          name: prop.name,
          context,
          element,
          items: [...nameBinding, ...valueBinding]
        },
        ...nameBinding,
        ...valueBinding
      ];
    } else {
      const nameBinding = prop.arg ? (0, utils_1.retrieveBindings)(prop.arg, context) : [];
      const valueBinding = prop.exp ? (0, utils_1.retrieveBindings)(prop.exp, context) : [];
      return [
        {
          type: "Directive",
          node: prop,
          name: prop.name,
          arg: prop.arg ? nameBinding.filter(
            (x) => x.type === "Binding"
            /* TemplateTypes.Binding */
          ) : null,
          exp: prop.exp ? valueBinding.filter(
            (x) => x.type === "Binding"
            /* TemplateTypes.Binding */
          ) : null,
          static: false,
          context,
          element,
          items: [...nameBinding, ...valueBinding]
        },
        ...nameBinding,
        ...valueBinding
      ];
    }
  }
  return props;
}

var hasRequiredProps;
function requireProps() {
  if (hasRequiredProps) return props$1;
  hasRequiredProps = 1;
  (function(exports) {
    var __createBinding = props$1 && props$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = props$1 && props$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireProps$1(), exports);
  })(props$1);
  return props$1;
}

var slots$1 = {};

var slots = {};

var hasRequiredSlots$1;
function requireSlots$1() {
  if (hasRequiredSlots$1) return slots;
  hasRequiredSlots$1 = 1;
  Object.defineProperty(slots, "__esModule", { value: true });
  slots.handleSlotDeclaration = handleSlotDeclaration;
  slots.handleSlotProp = handleSlotProp;
  const compiler_core_1 = require$$0$1;
  const props_1 = requireProps();
  function handleSlotDeclaration(node, parent, parentContext) {
    if (node.type !== compiler_core_1.NodeTypes.ELEMENT) {
      return null;
    }
    if (node.tagType !== compiler_core_1.ElementTypes.SLOT) {
      return null;
    }
    const items = [];
    const propItems = (0, props_1.handleProps)(node, parentContext) ?? [];
    const props = propItems.filter(
      (x) => x.type === "Prop"
      /* TemplateTypes.Prop */
    );
    const name = propItems.find((prop) => prop.name === "name") ?? null;
    const slot = {
      type: "SlotDeclaration",
      node,
      name,
      props,
      parent
    };
    const context = {
      ...parentContext,
      ignoredIdentifiers: [...parentContext.ignoredIdentifiers]
    };
    items.push(slot);
    items.push(...propItems);
    return {
      slot,
      context,
      items
    };
  }
  function handleSlotProp(node, parent, parentContext, condition) {
    var _a;
    if (node.type !== compiler_core_1.NodeTypes.ELEMENT) {
      return null;
    }
    const propDirective = node.props.find((x) => x.type === compiler_core_1.NodeTypes.DIRECTIVE && x.name === "slot");
    if (!propDirective) {
      return null;
    }
    const [prop] = (0, props_1.propToTemplateProp)(propDirective, node, parentContext);
    const ignoredIdentifiers = ((_a = prop.exp) == null ? void 0 : _a.map((x) => x.type === "Binding" && x.name)) ?? [];
    const context = ignoredIdentifiers.length > 0 ? {
      ...parentContext,
      ignoredIdentifiers: [
        ...parentContext.ignoredIdentifiers,
        ...ignoredIdentifiers
      ]
    } : parentContext;
    const slot = {
      type: "SlotRender",
      prop,
      parent: node.type === compiler_core_1.NodeTypes.ELEMENT && node.tag !== "template" ? null : parent,
      element: node,
      name: prop.arg,
      context,
      condition: condition ?? null
    };
    const items = [slot];
    if (prop.type === "Directive") {
      if (prop.arg && prop.arg.length) {
        const b = prop.arg.filter((x) => x.type === "Binding" && !x.ignore);
        items.push(...b);
      }
    }
    return {
      slot,
      items,
      context
    };
  }
  return slots;
}

var hasRequiredSlots;
function requireSlots() {
  if (hasRequiredSlots) return slots$1;
  hasRequiredSlots = 1;
  (function(exports) {
    var __createBinding = slots$1 && slots$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = slots$1 && slots$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireSlots$1(), exports);
  })(slots$1);
  return slots$1;
}

var hasRequiredElement$2;
function requireElement$2() {
  if (hasRequiredElement$2) return element$2;
  hasRequiredElement$2 = 1;
  Object.defineProperty(element$2, "__esModule", { value: true });
  element$2.handleElement = handleElement;
  const compiler_core_1 = require$$0$1;
  const conditions_1 = requireConditions();
  const loops_1 = requireLoops();
  const props_1 = requireProps();
  const slots_1 = requireSlots();
  function handleElement(node, parent, parentContext) {
    if (node.type !== compiler_core_1.NodeTypes.ELEMENT) {
      return null;
    }
    let context = parentContext;
    const conditions = (0, conditions_1.handleConditions)(node, parent, context);
    if (conditions) {
      context = conditions.context;
    }
    const loop = (0, loops_1.handleLoopProp)(node, parent, context);
    if (loop) {
      context = loop.context;
    }
    const propBindings = (0, props_1.handleProps)(node, context);
    const props = (propBindings == null ? void 0 : propBindings.filter((x) => x.type === "Prop")) ?? [];
    const slot = (0, slots_1.handleSlotDeclaration)(node, parent, context);
    const propSlot = (0, slots_1.handleSlotProp)(node, parent, context, conditions == null ? void 0 : conditions.condition);
    if (propSlot) {
      context = propSlot.context;
    }
    const element2 = {
      type: "Element",
      tag: node.tag,
      node,
      parent,
      ref: (props == null ? void 0 : props.find((x) => {
        if (x.name === "ref") {
          return true;
        }
        const node2 = x.node;
        if ((node2 == null ? void 0 : node2.type) === compiler_core_1.NodeTypes.DIRECTIVE) {
          return node2.rawName === ":ref";
        }
        return false;
      })) ?? null,
      props: props ?? [],
      condition: (conditions == null ? void 0 : conditions.condition) ?? null,
      loop: (loop == null ? void 0 : loop.loop) ?? null,
      slot: (slot == null ? void 0 : slot.slot) ?? (propSlot == null ? void 0 : propSlot.slot) ?? null,
      context
    };
    const items = [
      ...(conditions == null ? void 0 : conditions.items) ?? [],
      ...(loop == null ? void 0 : loop.items) ?? [],
      ...node.tagType === compiler_core_1.ElementTypes.COMPONENT ? [
        {
          type: "Binding",
          name: node.tag.split(".")[0],
          node,
          isComponent: true,
          directive: null,
          exp: null,
          parent: null
        }
      ] : [],
      element2,
      ...propBindings ?? [],
      ...(propSlot == null ? void 0 : propSlot.items) ?? [],
      ...[slot == null ? void 0 : slot.slot].filter((x) => x)
    ];
    return {
      element: element2,
      context,
      items,
      conditions,
      loop,
      props,
      slot
    };
  }
  return element$2;
}

var text$2 = {};

var hasRequiredText$2;
function requireText$2() {
  if (hasRequiredText$2) return text$2;
  hasRequiredText$2 = 1;
  Object.defineProperty(text$2, "__esModule", { value: true });
  text$2.handleText = handleText;
  const compiler_core_1 = require$$0$1;
  function handleText(node) {
    if (node.type !== compiler_core_1.NodeTypes.TEXT) {
      return null;
    }
    return {
      type: "Text",
      content: node.content,
      node
    };
  }
  return text$2;
}

var hasRequiredTemplate$5;
function requireTemplate$5() {
  if (hasRequiredTemplate$5) return template$5;
  hasRequiredTemplate$5 = 1;
  Object.defineProperty(template$5, "__esModule", { value: true });
  template$5.parseTemplate = parseTemplate;
  const compiler_core_1 = require$$0$1;
  const index_js_1 = requireWalk();
  const comment_js_1 = requireComment$2();
  const interpolation_js_1 = requireInterpolation$2();
  const element_js_1 = requireElement$2();
  const text_js_1 = requireText$2();
  function parseTemplate(ast, source, ignoredIdentifiers = []) {
    const items = [];
    (0, index_js_1.templateWalk)(ast, {
      enter(node, parent, context) {
        let list = null;
        let overrideContext = null;
        switch (node.type) {
          case compiler_core_1.NodeTypes.COMMENT: {
            const comment = (0, comment_js_1.handleComment)(node);
            if (comment) {
              list = [comment];
            }
            break;
          }
          case compiler_core_1.NodeTypes.INTERPOLATION: {
            list = (0, interpolation_js_1.handleInterpolation)(node, context);
            break;
          }
          case compiler_core_1.NodeTypes.TEXT: {
            const t = (0, text_js_1.handleText)(node);
            if (t) {
              list = [t];
            }
            break;
          }
          case compiler_core_1.NodeTypes.ELEMENT: {
            const e = (0, element_js_1.handleElement)(node, parent, context);
            if (e) {
              list = e.items;
              overrideContext = e.context;
            }
            break;
          }
        }
        items.push(...list ?? []);
        if (overrideContext) {
          return overrideContext;
        }
      }
    }, {
      conditions: [],
      inFor: false,
      ignoredIdentifiers
    });
    return {
      root: ast,
      source,
      items
    };
  }
  return template$5;
}

var generic$1 = {};

var generic = {};

var hasRequiredGeneric$1;
function requireGeneric$1() {
  if (hasRequiredGeneric$1) return generic;
  hasRequiredGeneric$1 = 1;
  Object.defineProperty(generic, "__esModule", { value: true });
  generic.parseGeneric = parseGeneric;
  const ast_1 = requireAst();
  function parseGeneric(genericStr, offset = 0, prefix = "__VERTER__TS__") {
    var _a;
    if (!genericStr)
      return null;
    const genericCode = `type __GENERIC__<${genericStr}> = {};`;
    const ast = (0, ast_1.parseOXC)(genericCode, { lang: "ts", sourceType: "script" });
    const body = ast.program.body[0];
    if (!body || !("typeParameters" in body) || !body.typeParameters) {
      return null;
    }
    const params = ((_a = body == null ? void 0 : body.typeParameters) == null ? void 0 : _a.params) ?? [];
    const items = params.map((param, index) => ({
      name: param.name.name,
      content: genericCode.slice(param.start, param.end),
      constraint: param.constraint ? genericCode.slice(param.constraint.start, param.constraint.end) : void 0,
      default: param.default ? genericCode.slice(param.default.start, param.default.end) : void 0,
      index
    }));
    function getGenericComponentName(name) {
      return prefix + name;
    }
    function replaceComponentNameUsage(name, content) {
      const regex = new RegExp(`\\b${name}\\b`, "g");
      return content.replace(regex, getGenericComponentName(name));
    }
    function sanitiseGenericNames(content) {
      if (!content)
        return content ?? "";
      return names ? names.reduce((prev, cur) => {
        return replaceComponentNameUsage(cur, prev);
      }, content) : content;
    }
    const names = items.map((x) => x.name);
    const sanitisedNames = names.map(sanitiseGenericNames);
    const declaration = items.map((x) => {
      const name = getGenericComponentName(x.name);
      const constraint = sanitiseGenericNames(x.constraint);
      const defaultType = sanitiseGenericNames(x.default);
      return [
        name,
        constraint ? `extends ${constraint}` : void 0,
        `= ${defaultType || "any"}`
      ].filter(Boolean).join(" ");
    }).join(", ");
    return {
      source: genericStr,
      names,
      sanitisedNames,
      declaration,
      position: {
        start: offset,
        end: offset + genericStr.length
      }
    };
  }
  return generic;
}

var hasRequiredGeneric;
function requireGeneric() {
  if (hasRequiredGeneric) return generic$1;
  hasRequiredGeneric = 1;
  (function(exports) {
    var __createBinding = generic$1 && generic$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = generic$1 && generic$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireGeneric$1(), exports);
  })(generic$1);
  return generic$1;
}

var hasRequiredParser$1;
function requireParser$1() {
  if (hasRequiredParser$1) return parser;
  hasRequiredParser$1 = 1;
  Object.defineProperty(parser, "__esModule", { value: true });
  parser.parser = parser$1;
  const compiler_sfc_1 = require$$1;
  const index_js_1 = requireUtils$3();
  const script_js_1 = requireScript$3();
  const ast_js_1 = requireAst();
  const template_js_1 = requireTemplate$5();
  const index_js_2 = requireGeneric();
  function parser$1(source, filename = "temp.vue", options = {}) {
    {
      const noHTMLCommentSource = (0, index_js_1.cleanHTMLComments)(source);
      if (noHTMLCommentSource.indexOf("</script>") === -1 && noHTMLCommentSource.indexOf("<script ") === -1 && noHTMLCommentSource.indexOf("<script>") === -1) {
        source += `
<script></script>
`;
      }
    }
    const s = new compiler_sfc_1.MagicString(source);
    let generic = null;
    let isAsync = false;
    let isTS = false;
    let isSetup = false;
    const sfcParse = (0, compiler_sfc_1.parse)(source, {
      ...options,
      filename,
      // sourceMap: true,
      ignoreEmpty: false,
      templateParseOptions: {
        prefixIdentifiers: true,
        expressionPlugins: ["typescript"]
      }
    });
    const blocks = (0, index_js_1.extractBlocksFromDescriptor)(sfcParse.descriptor).sort((a, b) => {
      if (a.tag.type === "template")
        return 1;
      if (b.tag.type === "template")
        return -1;
      return 0;
    }).map((x) => {
      var _a;
      const languageId = (0, index_js_1.findBlockLanguage)(x);
      switch (languageId) {
        case "vue":
          const ast = (0, template_js_1.parseTemplate)(x.block.ast, x.block.content, generic == null ? void 0 : generic.names);
          return {
            type: "template",
            lang: languageId,
            block: x,
            result: ast
          };
        case "ts":
        case "tsx": {
          isTS = true;
          if (x.tag.attributes.generic && x.tag.attributes.generic.value) {
            generic = (0, index_js_2.parseGeneric)(x.tag.attributes.generic.value.content, x.tag.attributes.generic.value.start);
          }
        }
        case "js":
        case "jsx": {
          const prepend = "".padStart(x.block.loc.start.offset, " ");
          const content = prepend + x.block.content;
          const ast2 = (0, ast_js_1.parseAST)(content, filename + "." + languageId);
          const r = (0, script_js_1.parseScriptBetter)(ast2, x.block.attrs);
          isAsync = r.isAsync;
          isSetup = !!(((_a = x.block) == null ? void 0 : _a.setup) ?? false);
          return {
            type: "script",
            lang: languageId,
            block: x,
            result: r,
            isMain: x.block === (sfcParse.descriptor.scriptSetup || sfcParse.descriptor.script)
          };
        }
        default:
          return {
            type: x.tag.type,
            lang: languageId,
            block: x,
            result: null
          };
      }
    });
    return {
      filename,
      s,
      generic,
      isAsync,
      isTS,
      isSetup,
      blocks
    };
  }
  return parser;
}

var types$5 = {};

var hasRequiredTypes$5;
function requireTypes$5() {
  if (hasRequiredTypes$5) return types$5;
  hasRequiredTypes$5 = 1;
  Object.defineProperty(types$5, "__esModule", { value: true });
  return types$5;
}

var types$4 = {};

var hasRequiredTypes$4;
function requireTypes$4() {
  if (hasRequiredTypes$4) return types$4;
  hasRequiredTypes$4 = 1;
  Object.defineProperty(types$4, "__esModule", { value: true });
  return types$4;
}

var script$2 = {};

var types$3 = {};

var hasRequiredTypes$3;
function requireTypes$3() {
  if (hasRequiredTypes$3) return types$3;
  hasRequiredTypes$3 = 1;
  Object.defineProperty(types$3, "__esModule", { value: true });
  types$3.ScriptTypes = void 0;
  var ScriptTypes;
  (function(ScriptTypes2) {
    ScriptTypes2["Binding"] = "Binding";
    ScriptTypes2["Import"] = "Import";
    ScriptTypes2["FunctionCall"] = "FunctionCall";
    ScriptTypes2["Declaration"] = "Declaration";
    ScriptTypes2["Async"] = "Async";
    ScriptTypes2["Export"] = "Export";
    ScriptTypes2["DefaultExport"] = "DefaultExport";
    ScriptTypes2["TypeAssertion"] = "TypeAssertion";
    ScriptTypes2["Error"] = "Error";
    ScriptTypes2["Warning"] = "Warning";
  })(ScriptTypes || (types$3.ScriptTypes = ScriptTypes = {}));
  return types$3;
}

var hasRequiredScript$2;
function requireScript$2() {
  if (hasRequiredScript$2) return script$2;
  hasRequiredScript$2 = 1;
  (function(exports) {
    var __createBinding = script$2 && script$2.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = script$2 && script$2.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireScript$3(), exports);
    __exportStar(requireTypes$3(), exports);
  })(script$2);
  return script$2;
}

var template$4 = {};

var types$2 = {};

var hasRequiredTypes$2;
function requireTypes$2() {
  if (hasRequiredTypes$2) return types$2;
  hasRequiredTypes$2 = 1;
  Object.defineProperty(types$2, "__esModule", { value: true });
  types$2.TemplateTypes = void 0;
  var TemplateTypes;
  (function(TemplateTypes2) {
    TemplateTypes2["Binding"] = "Binding";
    TemplateTypes2["Comment"] = "Comment";
    TemplateTypes2["Text"] = "Text";
    TemplateTypes2["Interpolation"] = "Interpolation";
    TemplateTypes2["Prop"] = "Prop";
    TemplateTypes2["Element"] = "Element";
    TemplateTypes2["Directive"] = "Directive";
    TemplateTypes2["SlotRender"] = "SlotRender";
    TemplateTypes2["SlotDeclaration"] = "SlotDeclaration";
    TemplateTypes2["Condition"] = "Condition";
    TemplateTypes2["Loop"] = "Loop";
    TemplateTypes2["Function"] = "Function";
    TemplateTypes2["Literal"] = "Literal";
  })(TemplateTypes || (types$2.TemplateTypes = TemplateTypes = {}));
  return types$2;
}

var hasRequiredTemplate$4;
function requireTemplate$4() {
  if (hasRequiredTemplate$4) return template$4;
  hasRequiredTemplate$4 = 1;
  (function(exports) {
    var __createBinding = template$4 && template$4.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = template$4 && template$4.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireTemplate$5(), exports);
    __exportStar(requireTypes$2(), exports);
  })(template$4);
  return template$4;
}

var hasRequiredParser;
function requireParser() {
  if (hasRequiredParser) return parser$1;
  hasRequiredParser = 1;
  (function(exports) {
    var __createBinding = parser$1 && parser$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = parser$1 && parser$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireParser$1(), exports);
    __exportStar(requireTypes$5(), exports);
    __exportStar(requireTypes$4(), exports);
    __exportStar(requireScript$2(), exports);
    __exportStar(requireTemplate$4(), exports);
  })(parser$1);
  return parser$1;
}

var process$1 = {};

var script$1 = {};

var script = {};

var utils$1 = {};

var hasRequiredUtils$1;
function requireUtils$1() {
  if (hasRequiredUtils$1) return utils$1;
  hasRequiredUtils$1 = 1;
  Object.defineProperty(utils$1, "__esModule", { value: true });
  utils$1.defaultPrefix = defaultPrefix;
  utils$1.handleHelpers = handleHelpers;
  utils$1.generateImport = generateImport;
  function defaultPrefix(str) {
    return "___VERTER___" + str;
  }
  function retriveImportFromHelpers(source) {
    const VERTER_IMPORTS_KEY = "__VERTER_IMPORTS__";
    const startStr = `/* ${VERTER_IMPORTS_KEY}`;
    const endStr = `/${VERTER_IMPORTS_KEY} */`;
    const start = source.indexOf(startStr);
    if (start === -1)
      return [];
    const end = source.indexOf(endStr, start);
    if (end === -1) {
      throw new Error(endStr + ": not found");
    }
    const content = source.slice(start + startStr.length, end);
    const items = JSON.parse(content);
    return items;
  }
  function handleHelpers(source) {
    const VERTER_START = "__VERTER__START__";
    const imports = retriveImportFromHelpers(source);
    const content = source.slice(source.indexOf(VERTER_START) + VERTER_START.length).trim();
    function withPrefix(prefix) {
      return {
        content: content.replaceAll("$V_", prefix).replaceAll("\nexport ", "\n"),
        imports: imports.map((i) => ({
          ...i,
          items: i.items.map((i2) => ({
            ...i2,
            alias: i2.alias ? i2.alias.replaceAll("$V_", prefix) : void 0
          }))
        }))
      };
    }
    return {
      content,
      imports,
      withPrefix
    };
  }
  function generateImport(items) {
    const grouped = {};
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (!grouped[item.from]) {
        grouped[item.from] = [];
      }
      const list = grouped[item.from];
      if (item.asType) {
        list.push(...item.items.map((i2) => ({ ...i2, type: true })));
      } else {
        list.push(...item.items);
      }
    }
    const imports = [];
    for (const [key, value] of Object.entries(grouped)) {
      const added = /* @__PURE__ */ new Set();
      const toAdd = [];
      for (const item of value) {
        const name = item.alias ?? item.name;
        if (added.has(name)) {
          continue;
        }
        toAdd.push(item);
        added.add(name);
      }
      imports.push(`import { ${toAdd.map((i) => (i.type ? `type ` : "") + i.name + (i.alias ? ` as ${i.alias}` : "")).join(", ")} } from "${key}";`);
    }
    return imports.join("\n");
  }
  return utils$1;
}

var hasRequiredScript$1;
function requireScript$1() {
  if (hasRequiredScript$1) return script;
  hasRequiredScript$1 = 1;
  Object.defineProperty(script, "__esModule", { value: true });
  script.processScript = processScript;
  const utils_1 = requireUtils$1();
  function processScript(items, plugins, _context) {
    const context = {
      generic: null,
      isAsync: false,
      isTS: false,
      items: [],
      prefix: utils_1.defaultPrefix,
      isSetup: false,
      templateBindings: [],
      handledAttributes: /* @__PURE__ */ new Set(),
      ..._context
    };
    const s = context.override ? context.s : context.s.clone();
    const pluginsByType = {
      [
        "Async"
        /* ScriptTypes.Async */
      ]: [],
      [
        "Binding"
        /* ScriptTypes.Binding */
      ]: [],
      [
        "Declaration"
        /* ScriptTypes.Declaration */
      ]: [],
      [
        "Export"
        /* ScriptTypes.Export */
      ]: [],
      [
        "DefaultExport"
        /* ScriptTypes.DefaultExport */
      ]: [],
      [
        "FunctionCall"
        /* ScriptTypes.FunctionCall */
      ]: [],
      [
        "Import"
        /* ScriptTypes.Import */
      ]: [],
      [
        "TypeAssertion"
        /* ScriptTypes.TypeAssertion */
      ]: [],
      [
        "Error"
        /* ScriptTypes.Error */
      ]: [],
      [
        "Warning"
        /* ScriptTypes.Warning */
      ]: []
    };
    const PLUGIN_TYPES = Object.keys(pluginsByType);
    const prePlugins = [];
    const postPlugins = [];
    [...plugins].sort((a, b) => {
      if (a.enforce === "pre" && b.enforce === "post") {
        return -1;
      }
      if (a.enforce === "post" && b.enforce === "pre") {
        return 1;
      }
      if (a.enforce === "pre") {
        return -1;
      }
      if (a.enforce === "post") {
        return 1;
      }
      if (b.enforce === "pre") {
        return 1;
      }
      if (b.enforce === "post") {
        return -1;
      }
      return 0;
    }).forEach((x) => {
      for (const [key, value] of Object.entries(x)) {
        if (typeof value !== "function")
          continue;
        switch (key) {
          case "pre": {
            prePlugins.push(value.bind(x));
            break;
          }
          case "post": {
            postPlugins.push(value.bind(x));
            break;
          }
          case "transform": {
            PLUGIN_TYPES.forEach((type) => {
              pluginsByType[type].push(value.bind(x));
            });
            break;
          }
          default: {
            if (key.startsWith("transform")) {
              const type = key.slice(9);
              pluginsByType[type].push(value.bind(x));
            }
          }
        }
      }
    });
    for (const plugin of prePlugins) {
      plugin(s, context);
    }
    for (const item of items) {
      for (const plugin of pluginsByType[item.type]) {
        plugin(item, s, context);
      }
    }
    for (const plugin of postPlugins) {
      plugin(s, context);
    }
    return {
      context,
      s,
      result: s.toString()
    };
  }
  return script;
}

var builders$1 = {};

var bundle$1 = {};

var bundle = {};

var bundler = {};

var hasRequiredBundler;
function requireBundler() {
  if (hasRequiredBundler) return bundler;
  hasRequiredBundler = 1;
  Object.defineProperty(bundler, "__esModule", { value: true });
  bundler.BundlerHelper = void 0;
  const utils_1 = requireUtils$1();
  bundler.BundlerHelper = (0, utils_1.handleHelpers)(`/* __VERTER_IMPORTS__
  [
    {
      "from": "vue",
      "asType": true,
      "items": [
        { "name": "DefineProps", "alias": "$V_DefineProps" }
      ]
    }
  ]
  /__VERTER_IMPORTS__ */
  
  // __VERTER__START__
  
  export type $V_PartialUndefined<T> = {
    [P in keyof T]: undefined extends T[P] ? P : never;
  }[keyof T] extends infer U extends keyof T
    ? Omit<T, U> & Partial<Pick<T, U>>
    : T;
  
  export type $V_ProcessProps<T> = T extends $V_DefineProps<infer U, infer BKeys>
    ? $V_PartialUndefined<U>
    : $V_PartialUndefined<T>;`);
  return bundler;
}

var main$1 = {};

var main = {};

var plugins$1 = {};

var attributes$1 = {};

var attributes = {};

var types$1 = {};

var hasRequiredTypes$1;
function requireTypes$1() {
  if (hasRequiredTypes$1) return types$1;
  hasRequiredTypes$1 = 1;
  Object.defineProperty(types$1, "__esModule", { value: true });
  types$1.definePlugin = definePlugin;
  function definePlugin(plugin) {
    return plugin;
  }
  return types$1;
}

var hasRequiredAttributes$1;
function requireAttributes$1() {
  if (hasRequiredAttributes$1) return attributes;
  hasRequiredAttributes$1 = 1;
  Object.defineProperty(attributes, "__esModule", { value: true });
  attributes.AttributesPlugin = void 0;
  const types_1 = requireTypes$1();
  attributes.AttributesPlugin = (0, types_1.definePlugin)({
    name: "VerterAttributes",
    pre(s, ctx) {
      var _a;
      const tag = ctx.block.block.tag;
      const isTS = ctx.block.lang.startsWith("ts");
      const generic = ctx.generic;
      const attribute = tag.attributes.attributes;
      if (!attribute || !attribute.value)
        return;
      (_a = ctx.handledAttributes) == null ? void 0 : _a.add("attributes");
      const prefix = ctx.prefix("");
      if (isTS) {
        s.prependRight(attribute.start, `;type ${prefix}`);
        if (generic) {
          s.prependRight(attribute.key.end, `<${generic.source}>`);
        }
        s.remove(attribute.value.start - 1, attribute.value.start);
        s.overwrite(attribute.value.end, attribute.value.end + 1, ";");
        s.move(attribute.start, attribute.end, tag.pos.close.end);
      } else {
        const moveTo = tag.pos.close.end;
        s.prependLeft(moveTo, `/** @typedef `);
        if (attribute.value) {
          s.overwrite(attribute.value.start - 1, attribute.value.start, "{");
          s.overwrite(attribute.value.end, attribute.value.end + 1, "}");
          s.move(attribute.value.start - 1, attribute.value.end + 1, moveTo);
          s.remove(attribute.key.end, attribute.value.start - 1);
        } else {
          s.prependLeft(moveTo, `{}`);
        }
        s.prependRight(attribute.key.start, `${prefix}`);
        s.prependLeft(attribute.key.end, `*/`);
        s.move(attribute.key.start, attribute.key.end, moveTo);
      }
    }
  });
  return attributes;
}

var hasRequiredAttributes;
function requireAttributes() {
  if (hasRequiredAttributes) return attributes$1;
  hasRequiredAttributes = 1;
  (function(exports) {
    var __createBinding = attributes$1 && attributes$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = attributes$1 && attributes$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireAttributes$1(), exports);
  })(attributes$1);
  return attributes$1;
}

var binding$3 = {};

var binding$2 = {};

var hasRequiredBinding$3;
function requireBinding$3() {
  if (hasRequiredBinding$3) return binding$2;
  hasRequiredBinding$3 = 1;
  Object.defineProperty(binding$2, "__esModule", { value: true });
  binding$2.BindingPlugin = void 0;
  const types_1 = requireTypes$1();
  binding$2.BindingPlugin = (0, types_1.definePlugin)({
    name: "VerterBinding",
    // add known bindings
    transformDeclaration(item, _, ctx) {
      if (!item.name)
        return;
      ctx.items.push({
        type: "binding",
        name: item.name,
        originalName: item.name,
        item,
        node: item.node
      });
    },
    transformBinding(item, _, ctx) {
      if (!item.name)
        return;
      ctx.items.push({
        type: "binding",
        name: item.name,
        originalName: item.name,
        item,
        node: item.node
      });
    }
  });
  return binding$2;
}

var hasRequiredBinding$2;
function requireBinding$2() {
  if (hasRequiredBinding$2) return binding$3;
  hasRequiredBinding$2 = 1;
  (function(exports) {
    var __createBinding = binding$3 && binding$3.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = binding$3 && binding$3.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireBinding$3(), exports);
  })(binding$3);
  return binding$3;
}

var fullContext$1 = {};

var fullContext = {};

var utils = {};

var hasRequiredUtils;
function requireUtils() {
  if (hasRequiredUtils) return utils;
  hasRequiredUtils = 1;
  Object.defineProperty(utils, "__esModule", { value: true });
  utils.generateTypeDeclaration = generateTypeDeclaration;
  utils.generateTypeString = generateTypeString;
  function generateTypeDeclaration(name, content, generic, typescript) {
    return typescript ? `;export type ${name}${generic ? `<${generic}>` : ""}=${content};` : `
/** @typedef {${content}} ${name} */
/** @type {${name}} */
export const ${name} = null;
`;
  }
  function generateTypeString(name, info, ctx) {
    const isTS = ctx.block.lang.startsWith("ts");
    const isAsync = ctx.isAsync;
    const generic = ctx.generic;
    const content = `${info.isFunction ? "ReturnType<" : ""}${info.isType ? "" : "typeof "}${info.from}${generic ? `<${generic.names.join(",")}>` : ""}${info.isFunction ? ">" : ""}${isAsync && info.isFunction || info.key ? ` ${isAsync && info.isFunction ? "extends Promise<infer R" : ""}${info.key ? ` extends { ${info.key}: infer K }` : ""}${isAsync && info.isFunction ? ">" : ""}?${info.key ? "K" : "R"}:never` : ""}`;
    return generateTypeDeclaration(name, content, generic == null ? void 0 : generic.source, isTS);
  }
  return utils;
}

var hasRequiredFullContext$1;
function requireFullContext$1() {
  if (hasRequiredFullContext$1) return fullContext;
  hasRequiredFullContext$1 = 1;
  Object.defineProperty(fullContext, "__esModule", { value: true });
  fullContext.FullContextPlugin = void 0;
  const types_1 = requireTypes$1();
  const utils_1 = requireUtils();
  fullContext.FullContextPlugin = (0, types_1.definePlugin)({
    name: "VerterFullContext",
    enforce: "post",
    pre(s, ctx) {
      const importItem = ctx.isTS ? { name: "UnwrapRef", alias: ctx.prefix("UnwrapRef") } : { name: "unref", alias: ctx.prefix("unref") };
      ctx.items.push({
        type: "import",
        from: "vue",
        asType: ctx.isTS,
        items: [importItem]
      });
    },
    post(s, ctx) {
      const isTS = ctx.block.lang === "ts";
      const isAsync = ctx.isAsync;
      const fullContext2 = ctx.prefix("FullContext");
      const unref = ctx.prefix("unref");
      const unwrapRef = ctx.prefix("UnwrapRef");
      const bindings = ctx.items.filter((x) => x.type === "binding" && x.item.node);
      const names = /* @__PURE__ */ new Set();
      const content = /* @__PURE__ */ new Set();
      const source = s.original;
      for (const b of bindings) {
        switch (b.item.type) {
          case "Declaration": {
            const name = b.item.name;
            const node = b.item.declarator;
            if (name) {
              names.add(name);
              content.add(source.slice(node.start, node.end));
            }
          }
        }
      }
      const typeStr = (0, utils_1.generateTypeString)(fullContext2, {
        from: `${fullContext2}FN`,
        isFunction: true
      }, ctx);
      const str = `;${isAsync ? "async " : ""}function ${fullContext2}FN${ctx.generic ? `<${ctx.generic.source}>` : ""}() {${[...content].join("\n")};return{${[...names].map((x) => `${x}${isTS ? `: {} as ${unwrapRef}<typeof ${x}>` : `: ${unref}(${x})"`}`).join(",")}}};${typeStr}`;
      s.prependRight(ctx.block.block.tag.pos.close.end, str);
    }
  });
  return fullContext;
}

var hasRequiredFullContext;
function requireFullContext() {
  if (hasRequiredFullContext) return fullContext$1;
  hasRequiredFullContext = 1;
  (function(exports) {
    var __createBinding = fullContext$1 && fullContext$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = fullContext$1 && fullContext$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireFullContext$1(), exports);
  })(fullContext$1);
  return fullContext$1;
}

var scriptDefault$1 = {};

var scriptDefault = {};

var hasRequiredScriptDefault$1;
function requireScriptDefault$1() {
  if (hasRequiredScriptDefault$1) return scriptDefault;
  hasRequiredScriptDefault$1 = 1;
  Object.defineProperty(scriptDefault, "__esModule", { value: true });
  scriptDefault.ScriptDefaultPlugin = void 0;
  const types_1 = requireTypes$1();
  scriptDefault.ScriptDefaultPlugin = (0, types_1.definePlugin)({
    name: "VerterScriptDefault",
    // enforce: "pre",
    post(s, ctx) {
      var _a;
      ctx.block.lang === "ts";
      const isSetup = ctx.isSetup;
      ctx.isAsync;
      ctx.block.block.tag;
      const name = ctx.prefix("default_Component");
      const defineComponent = ctx.prefix("defineComponent");
      if (isSetup) {
        ctx.items.push({
          type: "import",
          from: "vue",
          items: [
            {
              name: "defineComponent",
              alias: defineComponent
            }
          ]
        });
        const optionsMacro = ctx.items.find(
          (x) => x.type === "options"
          /* ProcessItemType.Options */
        );
        let options = "{}";
        if (optionsMacro) {
          const start = optionsMacro.expression.start;
          const end = optionsMacro.expression.end;
          options = s.slice(start, end);
        }
        s.append(`
;export const ${name}=${defineComponent}(${options});`);
      } else {
        const componentExport = (_a = ctx.block.result) == null ? void 0 : _a.items.find((x) => x.type === "DefaultExport");
        if (!componentExport) {
          ctx.items.push({
            type: "import",
            from: "vue",
            items: [
              {
                name: "defineComponent",
                alias: defineComponent
              }
            ]
          });
          s.append(`;export const ${name}=${defineComponent}({});`);
          return;
        }
        const defaultStartPos = componentExport.node.start + 7;
        const defaultEndPos = componentExport.node.declaration.start;
        s.overwrite(defaultStartPos, defaultEndPos, `const ${name}=`);
        switch (componentExport.node.declaration.type) {
          // if does not have a wrapper
          case "ObjectExpression": {
            ctx.items.push({
              type: "import",
              from: "vue",
              items: [
                {
                  name: "defineComponent",
                  alias: defineComponent
                }
              ]
            });
            s.appendRight(componentExport.node.declaration.start, `${defineComponent}(`);
            s.appendLeft(componentExport.node.declaration.end, ");");
            return;
          }
        }
      }
    }
  });
  return scriptDefault;
}

var hasRequiredScriptDefault;
function requireScriptDefault() {
  if (hasRequiredScriptDefault) return scriptDefault$1;
  hasRequiredScriptDefault = 1;
  (function(exports) {
    var __createBinding = scriptDefault$1 && scriptDefault$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = scriptDefault$1 && scriptDefault$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireScriptDefault$1(), exports);
  })(scriptDefault$1);
  return scriptDefault$1;
}

var imports$1 = {};

var imports = {};

var hasRequiredImports$1;
function requireImports$1() {
  if (hasRequiredImports$1) return imports;
  hasRequiredImports$1 = 1;
  Object.defineProperty(imports, "__esModule", { value: true });
  imports.ImportsPlugin = void 0;
  const utils_1 = requireUtils$1();
  const types_1 = requireTypes$1();
  imports.ImportsPlugin = (0, types_1.definePlugin)({
    name: "VerterImports",
    enforce: "post",
    post(s, ctx) {
      const imports2 = ctx.items.filter(
        (x) => x.type === "import"
        /* ProcessItemType.Import */
      );
      if (imports2.length === 0)
        return;
      const importStr = (0, utils_1.generateImport)(imports2);
      s.prepend(importStr);
    },
    transformImport(item, s) {
      s.move(item.node.start, item.node.end, 0);
    }
  });
  return imports;
}

var hasRequiredImports;
function requireImports() {
  if (hasRequiredImports) return imports$1;
  hasRequiredImports = 1;
  (function(exports) {
    var __createBinding = imports$1 && imports$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = imports$1 && imports$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireImports$1(), exports);
  })(imports$1);
  return imports$1;
}

var macros$1 = {};

var macros = {};

var hasRequiredMacros$1;
function requireMacros$1() {
  if (hasRequiredMacros$1) return macros;
  hasRequiredMacros$1 = 1;
  Object.defineProperty(macros, "__esModule", { value: true });
  macros.MacrosPlugin = void 0;
  const types_1 = requireTypes$1();
  const utils_1 = requireUtils();
  const Macros = /* @__PURE__ */ new Set([
    "defineProps",
    "defineEmits",
    "defineExpose",
    "defineOptions",
    "defineModel",
    "defineSlots",
    "withDefaults"
    // useSlots()/useAttrs()
  ]);
  const HelperLocation = "$verter/options.helper.ts";
  const MacroDependencies = /* @__PURE__ */ new Map([
    [
      "defineEmits",
      ["UnionToIntersection", "EmitMapToProps", "OverloadParameters"]
    ],
    ["defineModel", ["ModelToProps", "UnionToIntersection", "ModelToEmits"]],
    ["defineOptions", ["DefineOptions"]]
  ]);
  macros.MacrosPlugin = (0, types_1.definePlugin)({
    name: "VerterMacro",
    hasWithDefaults: false,
    pre(s, ctx) {
      this.hasWithDefaults = false;
    },
    post(s, ctx) {
      var _a;
      const isTS = ctx.block.lang.startsWith("ts");
      const macroBindinds = ctx.items.filter(
        (x) => x.type === "macro-binding"
        /* ProcessItemType.MacroBinding */
      );
      for (const macro of Macros) {
        if (macro === "withDefaults")
          continue;
        if (macro === "defineOptions") {
          continue;
        }
        const name = ctx.prefix(macro);
        const itemMacro = macro === "defineModel" ? ctx.items.find(
          (x) => x.type === "define-model"
          /* ProcessItemType.DefineModel */
        ) : macroBindinds.find((x) => x.macro === macro);
        const TemplateBinding = ctx.prefix("TemplateBinding");
        if (itemMacro) {
          const str = (0, utils_1.generateTypeString)(name, {
            from: TemplateBinding,
            key: name,
            isType: true
          }, ctx);
          s.append(str);
        } else {
          const str = (0, utils_1.generateTypeDeclaration)(name, "{}", (_a = ctx.generic) == null ? void 0 : _a.source, isTS);
          s.append(str);
        }
      }
    },
    transformDeclaration(item, s, ctx) {
      var _a;
      if (item.parent.type === "VariableDeclarator" && ((_a = item.parent.init) == null ? void 0 : _a.type) === "CallExpression") {
        if (item.parent.init.callee.type === "Identifier") {
          const macroName = item.parent.init.callee.name;
          if (!Macros.has(macroName) || macroName === "defineOptions") {
            return;
          }
          addMacroDependencies(macroName, ctx);
          let varName = item.parent.id.type === "Identifier" ? item.parent.id.name : "";
          if (ctx.isSetup) {
            if (macroName === "defineModel") {
              ctx.items.push({
                type: "define-model",
                varName,
                name: getModelName(item.parent.init),
                node: item.parent
              });
            } else if (macroName == "withDefaults") {
              const defineProps = item.parent.init.arguments[0];
              const pName = ctx.prefix("Props");
              if (defineProps.type === "CallExpression" && defineProps.callee.type === "Identifier" && defineProps.callee.name === "defineProps") {
                addMacroDependencies("defineProps", ctx);
                ctx.items.push({
                  type: "macro-binding",
                  name: pName,
                  macro: "defineProps",
                  node: defineProps
                });
              }
              s.appendLeft(item.declarator.start, `const ${pName}=`);
              s.appendLeft(defineProps.end, ";");
              s.appendRight(defineProps.end, pName);
              s.move(defineProps.start, defineProps.end, item.declarator.start);
              this.hasWithDefaults = true;
            } else {
              if (macroName === "defineProps" && this.hasWithDefaults) {
                return;
              }
              ctx.items.push({
                type: "macro-binding",
                name: varName,
                macro: macroName,
                node: item.parent
              });
            }
          } else {
            ctx.items.push({
              type: "warning",
              message: "MACRO_NOT_IN_SETUP",
              node: item.node,
              start: item.node.start,
              end: item.node.end
            });
          }
        }
      }
    },
    transformFunctionCall(item, s, ctx) {
      if (!Macros.has(item.name) || ctx.items.some((x) => (x.type === "macro-binding" || x.type === "define-model") && // check if is inside another macro
      x.node.start <= item.node.start && x.node.end >= item.node.end)) {
        return;
      }
      const macroName = item.name;
      addMacroDependencies(macroName, ctx);
      let varName = "";
      if (item.name === "defineModel") {
        const modelName = getModelName(item.node);
        const accessor = ctx.prefix("models");
        varName = `${accessor}_${modelName}`;
      } else if (macroName === "withDefaults") {
        if (this.hasWithDefaults) {
          return;
        }
        if (ctx.isSetup) {
          const defineProps = item.node.arguments[0];
          const pName = ctx.prefix("Props");
          if (defineProps.type === "CallExpression" && defineProps.callee.type === "Identifier" && defineProps.callee.name === "defineProps") {
            addMacroDependencies("defineProps", ctx);
            ctx.items.push({
              type: "macro-binding",
              name: pName,
              macro: "defineProps",
              node: defineProps
            });
          }
          s.appendLeft(item.node.start, `const ${pName}=`);
          s.appendLeft(defineProps.end, ";");
          s.appendRight(defineProps.end, pName);
          s.move(defineProps.start, defineProps.end, item.node.start);
          this.hasWithDefaults = true;
          return;
        }
      } else if (macroName === "defineOptions") {
        return handleDefineOptions(item.node, ctx);
      } else if (macroName === "defineProps" && this.hasWithDefaults) {
        return;
      } else {
        varName = ctx.prefix(macroName.replace("define", ""));
      }
      if (ctx.isSetup) {
        s.prependLeft(item.node.start, `const ${varName}=`);
        if (macroName === "defineModel") {
          ctx.items.push({
            type: "define-model",
            varName,
            name: getModelName(item.node),
            node: item.node
          });
        } else {
          ctx.items.push({
            type: "macro-binding",
            name: varName,
            macro: macroName,
            node: item.node
          });
        }
      } else {
        ctx.items.push({
          type: "warning",
          message: "MACRO_NOT_IN_SETUP",
          node: item.node,
          start: item.node.start,
          end: item.node.end
        });
      }
    }
  });
  function addMacroDependencies(macroName, ctx) {
    if (!ctx.block.lang.startsWith("ts"))
      return;
    const dependencies = MacroDependencies.get(macroName);
    if (dependencies) {
      ctx.items.push({
        type: "import",
        asType: true,
        from: HelperLocation,
        items: dependencies.map((dep) => ({ name: ctx.prefix(dep) }))
      });
    }
  }
  function handleDefineOptions(node, ctx) {
    if (node.arguments.length > 0) {
      const [arg] = node.arguments;
      switch (arg.type) {
        case "Identifier":
        case "ObjectExpression": {
          ctx.items.push({
            type: "options",
            node,
            expression: arg
          });
          break;
        }
        default: {
          ctx.items.push({
            type: "warning",
            message: "INVALID_DEFINE_OPTIONS",
            node,
            start: node.start,
            end: node.end
          });
        }
      }
      if (node.arguments.length > 1) {
        ctx.items.push({
          type: "warning",
          message: "INVALID_DEFINE_OPTIONS",
          node,
          start: arg.end,
          end: node.end
        });
      }
      return;
    } else {
      ctx.items.push({
        type: "warning",
        message: "INVALID_DEFINE_OPTIONS",
        node,
        start: node.start,
        end: node.end
      });
    }
  }
  function getModelName(node) {
    var _a;
    const nameArg = node.arguments[0];
    const modelName = (nameArg == null ? void 0 : nameArg.type) === "Literal" ? ((_a = nameArg.value) == null ? void 0 : _a.toString()) ?? "modelValue" : "modelValue";
    return modelName;
  }
  return macros;
}

var hasRequiredMacros;
function requireMacros() {
  if (hasRequiredMacros) return macros$1;
  hasRequiredMacros = 1;
  (function(exports) {
    var __createBinding = macros$1 && macros$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = macros$1 && macros$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireMacros$1(), exports);
  })(macros$1);
  return macros$1;
}

var scriptBlock$1 = {};

var scriptBlock = {};

var hasRequiredScriptBlock$1;
function requireScriptBlock$1() {
  if (hasRequiredScriptBlock$1) return scriptBlock;
  hasRequiredScriptBlock$1 = 1;
  Object.defineProperty(scriptBlock, "__esModule", { value: true });
  scriptBlock.ScriptBlockPlugin = void 0;
  const types_1 = requireTypes$1();
  scriptBlock.ScriptBlockPlugin = (0, types_1.definePlugin)({
    name: "VerterScriptBlock",
    // clean the script tag
    pre(s, ctx) {
      const tag = ctx.block.block.tag;
      if (ctx.isSetup) {
        s.overwrite(tag.pos.open.start, tag.pos.open.start + 1, `${ctx.isAsync ? "async " : ""}function `);
        s.appendRight(tag.pos.open.start, ";");
        s.overwrite(tag.pos.open.end - 1, tag.pos.open.end, "(){");
        s.update(tag.pos.close.start, tag.pos.close.end, "");
        s.prependRight(tag.pos.close.start, "}");
      } else {
        s.overwrite(tag.pos.open.start, tag.pos.open.end, "");
        s.overwrite(tag.pos.close.start, tag.pos.close.end, "");
      }
    },
    post(s, ctx) {
      var _a;
      const notMainScripts = ctx.blocks.filter((block) => block.type === "script" && block.isMain === false);
      if (notMainScripts.length) {
        for (const script of notMainScripts) {
          const start = script.block.tag.pos.open.start;
          const end = script.block.tag.pos.close.end;
          s.move(start, end, 0);
          s.remove(script.block.tag.pos.open.start, script.block.tag.pos.open.end);
          s.remove(script.block.tag.pos.close.start, script.block.tag.pos.close.end);
        }
      }
      const attributes = ctx.block.block.tag.attributes;
      for (const key in attributes) {
        if ((_a = ctx.handledAttributes) == null ? void 0 : _a.has(key))
          continue;
        const attr = attributes[key];
        if (key === "generic") {
          if (attr.value) {
            s.remove(attr.start, attr.value.start - 1);
            s.overwrite(attr.value.start - 1, attr.value.start, "<");
            s.overwrite(attr.value.end, attr.end, ">");
            continue;
          }
        }
        s.remove(attr.start, attr.end);
      }
    }
  });
  return scriptBlock;
}

var hasRequiredScriptBlock;
function requireScriptBlock() {
  if (hasRequiredScriptBlock) return scriptBlock$1;
  hasRequiredScriptBlock = 1;
  (function(exports) {
    var __createBinding = scriptBlock$1 && scriptBlock$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = scriptBlock$1 && scriptBlock$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireScriptBlock$1(), exports);
  })(scriptBlock$1);
  return scriptBlock$1;
}

var templateBinding$1 = {};

var templateBinding = {};

var hasRequiredTemplateBinding$1;
function requireTemplateBinding$1() {
  if (hasRequiredTemplateBinding$1) return templateBinding;
  hasRequiredTemplateBinding$1 = 1;
  Object.defineProperty(templateBinding, "__esModule", { value: true });
  templateBinding.TemplateBindingPlugin = void 0;
  const types_1 = requireTypes$1();
  const utils_1 = requireUtils();
  templateBinding.TemplateBindingPlugin = (0, types_1.definePlugin)({
    name: "VerterTemplateBinding",
    enforce: "post",
    post(s, ctx) {
      const isTS = ctx.block.lang === "ts";
      ctx.isAsync;
      const tag = ctx.block.block.tag;
      const name = ctx.prefix("TemplateBinding");
      if (!ctx.isSetup) {
        const declaration = `function ${name}FN(){return {}}`;
        const typeStr2 = (0, utils_1.generateTypeString)(name, {
          from: `${name}FN`,
          isFunction: true
        }, ctx);
        s.prependRight(tag.pos.close.end, [declaration, typeStr2].join(";"));
        return;
      }
      const bindings = /* @__PURE__ */ new Map();
      for (const item of ctx.items) {
        switch (item.type) {
          case "binding": {
            bindings.set(item.name, item.node);
            break;
          }
        }
      }
      ctx.prefix("models");
      const unref = ctx.prefix("unref");
      const unwrapRef = ctx.prefix("UnwrapRef");
      const macroBindings = ctx.items.filter(
        (x) => x.type === "macro-binding"
        /* ProcessItemType.MacroBinding */
      ).reduce((acc, x) => {
        const n = ctx.prefix(x.macro === "withDefaults" ? "defineProps" : x.macro);
        acc[n] = x.name;
        return acc;
      }, {});
      const usedBindings = ctx.templateBindings.map((x) => {
        if (!x.name)
          return;
        const b = bindings.get(x.name);
        if (!b)
          return;
        return {
          name: x.name,
          start: b.start,
          end: b.end
        };
      }).filter((x) => !!x);
      const defineModels = ctx.items.filter(
        (x) => x.type === "define-model"
        /* ProcessItemType.DefineModel */
      );
      s.prependRight(tag.pos.close.start, `;return{${usedBindings.map((x) => `${x.name}/*${x.start},${x.end}*/: ${isTS ? `${x.name} as unknown as ${unwrapRef}<typeof ${x.name}>` : `${unref}(${x.name})`}`).concat(
        Object.entries(macroBindings).map(([k, x]) => `${k}:${`${isTS ? `${x} as typeof ${x}` : ""}`}`)
        // Object.entries(macroBindings).map(([k, v]) =>
        //   v.map((x) => `${k}:${k}`)
        // )
      ).concat([
        // defineModel regular props
        `${ctx.prefix("defineModel")}:{${defineModels.map((x) => (
          // TODO this should be either pointing to the variable or to the function itself
          `${x.name}/*${x.node.start},${x.node.end}*/: ${isTS ? `${x.varName} as typeof ${x.varName}` : x.varName}`
        )).join(",")}}`
      ]).join(",")}}`);
      if (!isTS) {
        s.prependLeft(tag.pos.open.start, `/** @returns {{${usedBindings.map((x) => `${x.name}:${unwrapRef}<typeof ${x.name}>`).join(",")}}} */`);
      }
      const typeStr = (0, utils_1.generateTypeString)(name, {
        from: `${name}FN`,
        isFunction: true
      }, ctx);
      s.prependRight(tag.pos.close.end, typeStr);
      s.overwrite(tag.pos.open.start + 1, tag.pos.content.start, `${name}FN`);
    }
  });
  return templateBinding;
}

var hasRequiredTemplateBinding;
function requireTemplateBinding() {
  if (hasRequiredTemplateBinding) return templateBinding$1;
  hasRequiredTemplateBinding = 1;
  (function(exports) {
    var __createBinding = templateBinding$1 && templateBinding$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = templateBinding$1 && templateBinding$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireTemplateBinding$1(), exports);
  })(templateBinding$1);
  return templateBinding$1;
}

var templateRef$1 = {};

var templateRef = {};

var hasRequiredTemplateRef$1;
function requireTemplateRef$1() {
  if (hasRequiredTemplateRef$1) return templateRef;
  hasRequiredTemplateRef$1 = 1;
  Object.defineProperty(templateRef, "__esModule", { value: true });
  templateRef.TemplateRefPlugin = void 0;
  const types_1 = requireTypes$1();
  const vue_1 = require$$1$2;
  const shared_1 = require$$1$1;
  const NormalisedComponentsAccessor = "NormalisedComponents";
  templateRef.TemplateRefPlugin = (0, types_1.definePlugin)({
    name: "VerterTemplateRef",
    transformDeclaration(item, s, ctx) {
      var _a;
      if (item.parent.type === "VariableDeclarator" && ((_a = item.parent.init) == null ? void 0 : _a.type) === "CallExpression") {
        if (item.parent.init.callee.type === "Identifier") {
          const macroName = item.parent.init.callee.name;
          if (macroName !== "useTemplateRef") {
            return;
          }
          handleExpression(item.parent.init, s, ctx);
        }
      }
    },
    transformFunctionCall(item, s, ctx) {
      if (item.name !== "useTemplateRef")
        return;
      handleExpression(item.node, s, ctx);
    }
  });
  function handleExpression(node, s, ctx) {
    var _a, _b, _c, _d, _e, _f, _g, _h, _i, _j, _k, _l;
    if (node.typeArguments !== null)
      return;
    const templateItems = (_b = (_a = ctx.blocks.find((x) => x.type === "template")) == null ? void 0 : _a.result) == null ? void 0 : _b.items.filter((x) => x.type === "Element");
    if (!templateItems || !templateItems.length) {
      return;
    }
    const nameArg = (_c = node.arguments) == null ? void 0 : _c[0];
    const name = nameArg ? nameArg.type === "Literal" ? s.original.slice(nameArg.start + 1, nameArg.end - 1) : s.original.slice(nameArg.start, nameArg.end) : "";
    const possibleTypes = [];
    const possibleNames = [];
    for (const item of templateItems) {
      if (!item.ref)
        continue;
      const ref = item.ref;
      const rawRefName = "arg" in ref ? (
        // @ts-expect-error
        ((_e = (_d = ref.node.exp) == null ? void 0 : _d.ast) == null ? void 0 : _e.type) && // @ts-expect-error
        ((_h = (_g = (_f = ref.node.exp) == null ? void 0 : _f.ast) == null ? void 0 : _g.type) == null ? void 0 : _h.indexOf("Function")) !== -1 ? "" : `typeof ${(_k = (_j = (_i = ref.exp) == null ? void 0 : _i[0]) == null ? void 0 : _j.exp) == null ? void 0 : _k.content}`
      ) : (
        // @ts-expect-error
        `"${ref.value}"`
      );
      if (!rawRefName)
        continue;
      possibleNames.push(rawRefName);
      const refName = rawRefName.startsWith("typeof ") ? rawRefName.slice(7) : rawRefName;
      if (!name || name === refName) {
        const tag = resolveTagNameType(item, ctx);
        if (Array.isArray(tag)) {
          possibleTypes.push(...tag);
        } else if (tag) {
          possibleTypes.push(tag);
        }
      } else if ((_l = ctx.block.result) == null ? void 0 : _l.items) {
        for (const scriptItem of ctx.block.result.items) {
          if (scriptItem.type !== "Declaration")
            continue;
          let declarationValue = retrieveDeclarationStringValue(scriptItem);
          if (!declarationValue) {
            if (refName.indexOf(".")) {
              declarationValue = retrieveDeclarationStringValueFromObject(scriptItem, refName);
            }
          }
          if (declarationValue === name && (~refName.indexOf(".") || scriptItem.name === refName)) {
            const tag = resolveTagNameType(item, ctx);
            if (Array.isArray(tag)) {
              possibleTypes.push(...tag);
            } else if (tag) {
              possibleTypes.push(tag);
            }
          }
        }
      }
    }
    if (possibleNames.length === 0)
      return;
    const types = possibleTypes.join("|");
    const names = possibleNames.join("|");
    const isTS = ctx.block.lang.startsWith("ts");
    if (isTS) {
      s.prependLeft(node.callee.end, `<${types || "unknown"},${names}>`);
    } else {
      s.prependLeft(node.callee.start, `/**@type{typeof import('vue').useTemplateRef<${types || "unknown"},${names}>}*/(`);
      s.prependLeft(node.callee.end, ")");
    }
  }
  function resolveTagNameType(item, ctx) {
    var _a;
    if (item.tag === "component") {
      const propIs = (_a = item.props) == null ? void 0 : _a.find((x) => {
        var _a2, _b;
        return x.name === "is" || x.type === "Prop" && "arg" in x && ((_b = (_a2 = x.arg) == null ? void 0 : _a2[0]) == null ? void 0 : _b.name) === "is";
      });
      if (!propIs)
        return;
      if (propIs.static && propIs.value) {
        return resolveTagNameForTag(propIs.value, ctx);
      }
      const directive = propIs.node;
      const exp = directive.exp;
      if (!exp)
        return;
      if (exp.ast) {
        const leafs = findAstConditionsLeafs(exp.ast);
        return leafs.filter((x) => x.type === "StringLiteral").map((x) => resolveTagNameForTag(x.value, ctx));
      } else {
        return `typeof ${exp.content}`;
      }
    }
    return resolveTagNameForTag(item.tag, ctx);
  }
  function resolveTagNameForTag(tag, ctx) {
    var _a, _b;
    const Normalised = ctx.prefix(NormalisedComponentsAccessor);
    if ((0, shared_1.isHTMLTag)(tag)) {
      return `HTML${(0, vue_1.capitalize)(tag)}Element`;
    }
    if (~tag.indexOf(".")) {
      const newTag = tag.split(".").map((x) => `["${x}"]`).join("");
      return `${Normalised}${newTag}`;
    }
    const camel = (0, vue_1.camelize)(tag);
    const camelCapitalised = (0, vue_1.capitalize)(camel);
    const found = /* @__PURE__ */ new Set();
    if ((_a = ctx.block.result) == null ? void 0 : _a.items) {
      for (const item of (_b = ctx.block.result) == null ? void 0 : _b.items) {
        let names = void 0;
        if (item.type === "Binding") {
          names = [item.name];
        } else if (item.type === "Import") {
          names = item.bindings.map((x) => x.name);
        } else if (item.type === "Declaration") {
          const vname = retrieveDeclarationStringValue(item);
          if (vname) {
            names = [vname];
          }
        }
        if (!names)
          continue;
        names.forEach((name) => {
          if (name === tag) {
            found.add(name);
          }
          if (name === camel) {
            found.add(name);
          }
          if (name === camelCapitalised) {
            found.add(name);
          }
        });
      }
    }
    let t = "";
    if (found.has(tag)) {
      t = tag;
    } else if (found.has(camel)) {
      t = camel;
    } else if (found.has(camelCapitalised)) {
      t = camelCapitalised;
    }
    if (!t)
      return `${Normalised}["${tag}"]`;
    return `${Normalised}["${t}"]`;
  }
  function retrieveDeclarationStringValue(item) {
    var _a;
    if (item.parent.type !== "VariableDeclarator")
      return;
    const init = item.parent.init;
    if (!init)
      return;
    if (init.type === "Literal") {
      return (_a = init.value) == null ? void 0 : _a.toString();
    }
    if (init.type === "TemplateLiteral") {
      return init.quasis[0].value.raw;
    }
    return;
  }
  function retrieveDeclarationStringValueFromObject(item, path) {
    var _a, _b, _c;
    if (item.parent.type !== "VariableDeclarator")
      return void 0;
    const init = item.parent.init;
    if (!init || init.type !== "ObjectExpression")
      return void 0;
    const parts = path.split(".");
    if (item.name !== parts.shift())
      return void 0;
    let object = init;
    while (parts.length) {
      const part = parts.shift();
      if (!part)
        break;
      const property = object.properties.find((x) => x.type === "Property" && x.key.type === "Identifier" && x.key.name === part);
      if (!property)
        break;
      if (parts.length === 0) {
        return property.value.type === "Literal" ? (_a = property.value.value) == null ? void 0 : _a.toString() : "expression" in property.value && typeof property.value.expression === "object" && "value" in property.value.expression ? (_c = (_b = property.value.expression) == null ? void 0 : _b.value) == null ? void 0 : _c.toString() : "";
      }
    }
    return void 0;
  }
  function findAstConditionsLeafs(node) {
    const leafs = [];
    const queue = [node];
    while (queue.length) {
      const n = queue.shift();
      if (!n)
        continue;
      if (n.type === "ConditionalExpression") {
        queue.push(n.consequent, n.alternate);
      } else if (n.type === "LogicalExpression") {
        queue.push(n.left, n.right);
      } else if (n.type === "BinaryExpression") {
        queue.push(n.left, n.right);
      } else {
        leafs.push(n);
      }
    }
    return leafs;
  }
  return templateRef;
}

var hasRequiredTemplateRef;
function requireTemplateRef() {
  if (hasRequiredTemplateRef) return templateRef$1;
  hasRequiredTemplateRef = 1;
  (function(exports) {
    var __createBinding = templateRef$1 && templateRef$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = templateRef$1 && templateRef$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireTemplateRef$1(), exports);
  })(templateRef$1);
  return templateRef$1;
}

var sfcCleaner$3 = {};

var sfcCleaner$2 = {};

var hasRequiredSfcCleaner$3;
function requireSfcCleaner$3() {
  if (hasRequiredSfcCleaner$3) return sfcCleaner$2;
  hasRequiredSfcCleaner$3 = 1;
  Object.defineProperty(sfcCleaner$2, "__esModule", { value: true });
  sfcCleaner$2.SFCCleanerPlugin = void 0;
  const types_1 = requireTypes$1();
  sfcCleaner$2.SFCCleanerPlugin = (0, types_1.definePlugin)({
    name: "VerterSFCCleaner",
    post(s, ctx) {
      ctx.blocks.forEach((block) => {
        if (block === ctx.block) {
          return;
        }
        const content = s.original.slice(block.block.tag.pos.open.start, block.block.tag.pos.close.end);
        const lines = content.split("\n");
        let lineOffset = block.block.tag.pos.open.start;
        for (const l of lines) {
          s.appendLeft(lineOffset, "// ");
          lineOffset += l.length + 1;
        }
      });
    }
  });
  return sfcCleaner$2;
}

var hasRequiredSfcCleaner$2;
function requireSfcCleaner$2() {
  if (hasRequiredSfcCleaner$2) return sfcCleaner$3;
  hasRequiredSfcCleaner$2 = 1;
  (function(exports) {
    var __createBinding = sfcCleaner$3 && sfcCleaner$3.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = sfcCleaner$3 && sfcCleaner$3.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireSfcCleaner$3(), exports);
  })(sfcCleaner$3);
  return sfcCleaner$3;
}

var resolvers$1 = {};

var resolvers = {};

var hasRequiredResolvers$1;
function requireResolvers$1() {
  if (hasRequiredResolvers$1) return resolvers;
  hasRequiredResolvers$1 = 1;
  Object.defineProperty(resolvers, "__esModule", { value: true });
  resolvers.ScriptResolversPlugin = void 0;
  const types_1 = requireTypes$1();
  const utils_1 = requireUtils();
  resolvers.ScriptResolversPlugin = (0, types_1.definePlugin)({
    name: "VerterResolvers",
    // clean the script tag
    post(s, ctx) {
      var _a, _b;
      ctx.block.block.tag;
      ctx.isTS;
      const genericNames = ctx.generic ? `<${ctx.generic.names.join(",")}>` : "";
      const hasModel = ctx.items.some(
        (x) => x.type === "define-model"
        /* ProcessItemType.DefineModel */
      );
      const definePropsName = ctx.prefix("defineProps");
      const defineEmitsName = ctx.prefix("defineEmits");
      const defineModelName = ctx.prefix("defineModel");
      const resolvePropsName = ctx.prefix("resolveProps");
      const resolveEmitsName = ctx.prefix("resolveEmits");
      const modelToProp = `{ readonly [K in keyof ${defineModelName}]: ${defineModelName}${genericNames}[K] extends { value: infer V } ? V : never }`;
      const modelToEmits = `{[K in keyof ${defineModelName}]?: ${defineModelName}${genericNames}[K] extends { value: infer V } ? (event: \`update:\${K}\`,value: V) => void : never }[keyof ${defineModelName}]`;
      const emitsToProps = `(${resolveEmitsName}${genericNames} extends (...args: infer Args extends any[]) => void ? {
        [K in Args[0] as \`on\${Capitalize<Args[0]>}\`]?: (...args: Args extends [e: infer E, ...args: infer P]
                ? K extends E
                ? P
                : never
                : never) => any
} : {})`;
      const resolveProps = [
        hasModel ? `Omit<${definePropsName}${genericNames}, keyof ${modelToProp}>` : `${definePropsName}${genericNames}`,
        emitsToProps,
        hasModel && modelToProp
      ].filter(Boolean).join(" & ");
      const resolveEmits = [`${defineEmitsName}${genericNames}`, hasModel && modelToEmits].filter(Boolean).join(" & ");
      s.append([
        (0, utils_1.generateTypeDeclaration)(resolvePropsName, resolveProps, (_a = ctx.generic) == null ? void 0 : _a.source, ctx.isTS),
        (0, utils_1.generateTypeDeclaration)(resolveEmitsName, resolveEmits, (_b = ctx.generic) == null ? void 0 : _b.source, ctx.isTS),
        ,
      ].filter(Boolean).join(";"));
    }
  });
  return resolvers;
}

var hasRequiredResolvers;
function requireResolvers() {
  if (hasRequiredResolvers) return resolvers$1;
  hasRequiredResolvers = 1;
  (function(exports) {
    var __createBinding = resolvers$1 && resolvers$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = resolvers$1 && resolvers$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireResolvers$1(), exports);
  })(resolvers$1);
  return resolvers$1;
}

var componentInstance$1 = {};

var componentInstance = {};

var hasRequiredComponentInstance$1;
function requireComponentInstance$1() {
  if (hasRequiredComponentInstance$1) return componentInstance;
  hasRequiredComponentInstance$1 = 1;
  Object.defineProperty(componentInstance, "__esModule", { value: true });
  componentInstance.ComponentInstancePlugin = void 0;
  const types_1 = requireTypes$1();
  const bundler_1 = requireBundler();
  const utils_1 = requireUtils$1();
  componentInstance.ComponentInstancePlugin = (0, types_1.definePlugin)({
    name: "VerterComponentInstance",
    enforce: "post",
    pre(s, ctx) {
      const prefix = ctx.prefix("");
      const bundler = bundler_1.BundlerHelper.withPrefix(prefix);
      const imports = [...bundler.imports];
      const str = (0, utils_1.generateImport)(imports);
      s.prepend(`${str}
`);
    },
    post(s, ctx) {
      const prefix = ctx.prefix("");
      const bundler = bundler_1.BundlerHelper.withPrefix(prefix);
      const ProcessPropsName = ctx.prefix("ProcessProps");
      const defaultOptionsName = ctx.prefix("default_Component");
      const resolvePropsName = ctx.prefix("resolveProps");
      const resolveSlotsName = ctx.prefix("defineSlots");
      const componentName = ctx.prefix("Component");
      const genericDeclaration = ctx.generic ? `<${ctx.generic.declaration}>` : "";
      const sanitisedNames = ctx.generic ? `<${ctx.generic.sanitisedNames.join(",")}>` : "";
      const propsType = [
        `(${ProcessPropsName}<${resolvePropsName}${sanitisedNames}`,
        `${ctx.isAsync ? " extends Promise<infer P> ? P : {}" : ""}>)`
      ].join("");
      const props = [`$props: ${propsType} & {};`];
      const slots = [
        `$slots: ${resolveSlotsName}${sanitisedNames}`,
        ` extends ${ctx.isAsync ? "Promise<" : ""}infer P${ctx.isAsync ? ">" : ""}`,
        `? P extends P & 1 ? {} : P & {} : never;`
      ];
      const instanceName = ctx.prefix("instance");
      const declaration = [
        ...ctx.isSetup ? [
          `export declare const ${componentName}: typeof ${defaultOptionsName} & {new${genericDeclaration}():{`,
          ...props,
          ...slots,
          `}`,
          `& ${propsType}`,
          `};`
        ] : [
          `export declare const ${componentName}: typeof ${defaultOptionsName};`
        ],
        // `declare const ${compName}: { a: string}`,
        // `export const ${componentName};`,
        `export type ${instanceName}${genericDeclaration} = InstanceType<typeof ${componentName + sanitisedNames}>;`
      ];
      s.append([bundler.content, declaration.join("")].join("\n"));
    }
  });
  return componentInstance;
}

var hasRequiredComponentInstance;
function requireComponentInstance() {
  if (hasRequiredComponentInstance) return componentInstance$1;
  hasRequiredComponentInstance = 1;
  (function(exports) {
    var __createBinding = componentInstance$1 && componentInstance$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = componentInstance$1 && componentInstance$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireComponentInstance$1(), exports);
  })(componentInstance$1);
  return componentInstance$1;
}

var defineOptions$1 = {};

var defineOptions = {};

var hasRequiredDefineOptions$1;
function requireDefineOptions$1() {
  if (hasRequiredDefineOptions$1) return defineOptions;
  hasRequiredDefineOptions$1 = 1;
  Object.defineProperty(defineOptions, "__esModule", { value: true });
  defineOptions.DefineOptionsPlugin = void 0;
  const types_1 = requireTypes$1();
  defineOptions.DefineOptionsPlugin = (0, types_1.definePlugin)({
    name: "VerterComponentInstance",
    enforce: "post",
    handled: false,
    pre() {
      this.handled = false;
    },
    transformDeclaration(item, s, ctx) {
      var _a;
      if (item.parent.type === "VariableDeclarator" && ((_a = item.parent.init) == null ? void 0 : _a.type) === "CallExpression") {
        if (item.parent.init.callee.type === "Identifier") {
          if (item.parent.init.callee.name !== "defineOptions")
            return;
          if (ctx.isSetup) {
            s.move(item.declarator.start, item.parent.end, 0);
          } else {
            ctx.items.push({
              type: "warning",
              message: "INVALID_DEFINE_OPTIONS",
              node: item.node,
              start: item.node.start,
              end: item.node.end
            });
          }
          this.handled = true;
        }
      }
    },
    transformFunctionCall(item, s, ctx) {
      if (item.name !== "defineOptions" || this.handled) {
        return;
      }
      if (ctx.isSetup) {
        s.move(item.node.start, item.node.end, 0);
      } else {
        ctx.items.push({
          type: "warning",
          message: "INVALID_DEFINE_OPTIONS",
          node: item.node,
          start: item.node.start,
          end: item.node.end
        });
      }
    }
  });
  return defineOptions;
}

var hasRequiredDefineOptions;
function requireDefineOptions() {
  if (hasRequiredDefineOptions) return defineOptions$1;
  hasRequiredDefineOptions = 1;
  (function(exports) {
    var __createBinding = defineOptions$1 && defineOptions$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = defineOptions$1 && defineOptions$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireDefineOptions$1(), exports);
  })(defineOptions$1);
  return defineOptions$1;
}

var hasRequiredPlugins$1;
function requirePlugins$1() {
  if (hasRequiredPlugins$1) return plugins$1;
  hasRequiredPlugins$1 = 1;
  (function(exports) {
    var __createBinding = plugins$1 && plugins$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = plugins$1 && plugins$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireAttributes(), exports);
    __exportStar(requireBinding$2(), exports);
    __exportStar(requireFullContext(), exports);
    __exportStar(requireScriptDefault(), exports);
    __exportStar(requireImports(), exports);
    __exportStar(requireMacros(), exports);
    __exportStar(requireScriptBlock(), exports);
    __exportStar(requireTemplateBinding(), exports);
    __exportStar(requireTemplateRef(), exports);
    __exportStar(requireSfcCleaner$2(), exports);
    __exportStar(requireResolvers(), exports);
    __exportStar(requireComponentInstance(), exports);
    __exportStar(requireDefineOptions(), exports);
  })(plugins$1);
  return plugins$1;
}

var hasRequiredMain$1;
function requireMain$1() {
  if (hasRequiredMain$1) return main;
  hasRequiredMain$1 = 1;
  Object.defineProperty(main, "__esModule", { value: true });
  main.ResolveOptionsFilename = ResolveOptionsFilename;
  main.buildOptions = buildOptions;
  const script_1 = requireScript$1();
  const plugins_1 = requirePlugins$1();
  function ResolveOptionsFilename(ctx) {
    return ctx.blockNameResolver(`options`);
  }
  function buildOptions(items, context) {
    var _a, _b;
    const template = context.blocks.find((x) => x.type === "template");
    return (0, script_1.processScript)(items, [
      plugins_1.ImportsPlugin,
      plugins_1.ScriptBlockPlugin,
      plugins_1.AttributesPlugin,
      plugins_1.BindingPlugin,
      plugins_1.FullContextPlugin,
      plugins_1.MacrosPlugin,
      plugins_1.TemplateBindingPlugin,
      plugins_1.ScriptDefaultPlugin,
      plugins_1.SFCCleanerPlugin,
      plugins_1.ScriptResolversPlugin,
      plugins_1.ComponentInstancePlugin,
      plugins_1.DefineOptionsPlugin
    ], {
      ...context,
      templateBindings: ((_a = template == null ? void 0 : template.result) == null ? void 0 : _a.items) ? (_b = template.result) == null ? void 0 : _b.items.filter((x) => x.type === "Binding") : []
    });
  }
  return main;
}

var hasRequiredMain;
function requireMain() {
  if (hasRequiredMain) return main$1;
  hasRequiredMain = 1;
  (function(exports) {
    var __createBinding = main$1 && main$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = main$1 && main$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireMain$1(), exports);
  })(main$1);
  return main$1;
}

var hasRequiredBundle$1;
function requireBundle$1() {
  if (hasRequiredBundle$1) return bundle;
  hasRequiredBundle$1 = 1;
  Object.defineProperty(bundle, "__esModule", { value: true });
  bundle.ResolveBundleFilename = ResolveBundleFilename;
  bundle.buildBundle = buildBundle;
  const vue_1 = require$$1$2;
  const bundler_1 = requireBundler();
  const utils_1 = requireUtils$1();
  const script_1 = requireScript$1();
  const main_1 = requireMain();
  function ResolveBundleFilename(ctx) {
    return ctx.blockNameResolver(`bundle.ts`);
  }
  function buildBundle(items, context) {
    return (0, script_1.processScript)(items, [
      {
        post: (s, ctx) => {
          var _a;
          s.remove(0, s.original.length);
          const prefix = ctx.prefix("");
          const bundler = bundler_1.BundlerHelper.withPrefix(prefix);
          const ProcessPropsName = ctx.prefix("ProcessProps");
          const imports = [...bundler.imports];
          const defaultOptionsName = ctx.prefix("default_Component");
          const resolvePropsName = ctx.prefix("resolveProps");
          const resolveSlotsName = ctx.prefix("defineSlots");
          ctx.blockNameResolver;
          imports.push({
            from: `./${(0, main_1.ResolveOptionsFilename)(ctx).split("/").pop() ?? ""}`,
            items: [
              { name: defaultOptionsName },
              { name: resolvePropsName },
              { name: resolveSlotsName }
            ]
          });
          const importsStr = (0, utils_1.generateImport)(imports);
          const compName = (0, vue_1.capitalize)((0, vue_1.camelize)(((_a = ctx.filename.split("/").pop()) == null ? void 0 : _a.split(".").shift()) ?? "Comp"));
          const genericDeclaration = ctx.generic ? `<${ctx.generic.declaration}>` : "";
          const sanitisedNames = ctx.generic ? `<${ctx.generic.sanitisedNames.join(",")}>` : "";
          const vSlotProp = `'${ctx.prefix("v-slot")}'?: (i: InstanceType<typeof ${compName}${sanitisedNames}>)  => any`;
          const props = [
            `$props: (${ProcessPropsName}<${resolvePropsName}${sanitisedNames}`,
            `${ctx.isAsync ? " extends Promise<infer P> ? P : {}" : ""}>)`,
            // ` & { supa?: (i: InstanceType<typeof ${compName}${sanitisedNames}>)  => any}`,
            // ` & { ___verter___slot?: (i: InstanceType<typeof ${compName}${sanitisedNames}>)  => any}`,
            ` & {${vSlotProp}};`
          ];
          const slots = [
            `$slots: ${resolveSlotsName}${sanitisedNames}`,
            ` extends ${ctx.isAsync ? "Promise<" : ""}infer P${ctx.isAsync ? ">" : ""}`,
            `? P extends P & 1 ? {} : P & {} : never;`
          ];
          const declaration = [
            ...ctx.isSetup ? [
              `declare const ${compName}: typeof ${defaultOptionsName} & {new${genericDeclaration}():{`,
              ...props,
              ...slots,
              `}};`
            ] : [`declare const ${compName}: typeof ${defaultOptionsName};`],
            // `declare const ${compName}: { a: string}`,
            `export default ${compName};`
          ];
          s.prepend([importsStr, bundler.content, declaration.join("")].join("\n"));
        }
      }
    ], context);
  }
  return bundle;
}

var hasRequiredBundle;
function requireBundle() {
  if (hasRequiredBundle) return bundle$1;
  hasRequiredBundle = 1;
  (function(exports) {
    var __createBinding = bundle$1 && bundle$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = bundle$1 && bundle$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireBundle$1(), exports);
  })(bundle$1);
  return bundle$1;
}

var hasRequiredBuilders$1;
function requireBuilders$1() {
  if (hasRequiredBuilders$1) return builders$1;
  hasRequiredBuilders$1 = 1;
  (function(exports) {
    var __createBinding = builders$1 && builders$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = builders$1 && builders$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireBundle(), exports);
    __exportStar(requireMain(), exports);
  })(builders$1);
  return builders$1;
}

var hasRequiredScript;
function requireScript() {
  if (hasRequiredScript) return script$1;
  hasRequiredScript = 1;
  (function(exports) {
    var __createBinding = script$1 && script$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = script$1 && script$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireScript$1(), exports);
    __exportStar(requireBuilders$1(), exports);
  })(script$1);
  return script$1;
}

var template$3 = {};

var template$2 = {};

var hasRequiredTemplate$3;
function requireTemplate$3() {
  if (hasRequiredTemplate$3) return template$2;
  hasRequiredTemplate$3 = 1;
  Object.defineProperty(template$2, "__esModule", { value: true });
  template$2.declareTemplatePlugin = declareTemplatePlugin;
  template$2.processTemplate = processTemplate;
  const utils_1 = requireUtils$1();
  function declareTemplatePlugin(plugin) {
    return plugin;
  }
  function processTemplate(items, plugins, _context) {
    const context = {
      generic: null,
      isAsync: false,
      isTS: false,
      camelWhitelistAttributes: (name) => {
        return name.startsWith("data-") || name.startsWith("aria-");
      },
      isCustomElement: (_) => {
        return false;
      },
      prefix: utils_1.defaultPrefix,
      retrieveAccessor: (name) => {
        return (0, utils_1.defaultPrefix)(name);
      },
      items: [],
      ..._context
    };
    const s = context.override ? context.s : context.s.clone();
    const pluginsByType = {
      [
        "SlotDeclaration"
        /* TemplateTypes.SlotDeclaration */
      ]: [],
      [
        "Loop"
        /* TemplateTypes.Loop */
      ]: [],
      [
        "Element"
        /* TemplateTypes.Element */
      ]: [],
      [
        "Prop"
        /* TemplateTypes.Prop */
      ]: [],
      [
        "Binding"
        /* TemplateTypes.Binding */
      ]: [],
      [
        "Interpolation"
        /* TemplateTypes.Interpolation */
      ]: [],
      [
        "SlotRender"
        /* TemplateTypes.SlotRender */
      ]: [],
      [
        "Comment"
        /* TemplateTypes.Comment */
      ]: [],
      [
        "Text"
        /* TemplateTypes.Text */
      ]: [],
      [
        "Directive"
        /* TemplateTypes.Directive */
      ]: [],
      [
        "Function"
        /* TemplateTypes.Function */
      ]: [],
      [
        "Condition"
        /* TemplateTypes.Condition */
      ]: [],
      [
        "Literal"
        /* TemplateTypes.Literal */
      ]: []
    };
    const PLUGIN_TYPES = Object.keys(pluginsByType);
    const prePlugins = [];
    const postPlugins = [];
    [...plugins].sort((a, b) => {
      if (a.enforce === "pre" && b.enforce === "post") {
        return -1;
      }
      if (a.enforce === "post" && b.enforce === "pre") {
        return 1;
      }
      if (a.enforce === "pre") {
        return -1;
      }
      if (a.enforce === "post") {
        return 1;
      }
      if (b.enforce === "pre") {
        return 1;
      }
      if (b.enforce === "post") {
        return -1;
      }
      return 0;
    }).forEach((x) => {
      for (const [key, value] of Object.entries(x)) {
        if (typeof value !== "function")
          continue;
        switch (key) {
          case "pre": {
            prePlugins.push(value.bind(x));
            break;
          }
          case "post": {
            postPlugins.push(value.bind(x));
            break;
          }
          case "transform": {
            PLUGIN_TYPES.forEach((type) => {
              pluginsByType[type].push(value.bind(x));
            });
            break;
          }
          default: {
            if (key.startsWith("transform")) {
              const type = key.slice(9);
              pluginsByType[type].push(value.bind(x));
            }
          }
        }
      }
    });
    for (const plugin of prePlugins) {
      plugin(s, context);
    }
    for (const item of items) {
      for (const plugin of pluginsByType[item.type]) {
        plugin(item, s, context);
      }
    }
    for (const plugin of postPlugins) {
      plugin(s, context);
    }
    return {
      context,
      s,
      result: s.toString()
    };
  }
  return template$2;
}

var plugins = {};

var binding$1 = {};

var binding = {};

var config = {};

var hasRequiredConfig;
function requireConfig() {
  if (hasRequiredConfig) return config;
  hasRequiredConfig = 1;
  Object.defineProperty(config, "__esModule", { value: true });
  config.DEBUG = void 0;
  config.DEBUG = true;
  return config;
}

var hasRequiredBinding$1;
function requireBinding$1() {
  if (hasRequiredBinding$1) return binding;
  hasRequiredBinding$1 = 1;
  Object.defineProperty(binding, "__esModule", { value: true });
  binding.BindingPlugin = void 0;
  const config_1 = requireConfig();
  binding.BindingPlugin = {
    name: "VerterBinding",
    transformBinding(item, s, ctx) {
      var _a, _b;
      if (item.ignore || "skip" in item || item.isComponent) {
        return;
      }
      const accessor = ctx.retrieveAccessor("ctx");
      if (config_1.DEBUG) {
        if ((_a = item.name) == null ? void 0 : _a.startsWith("___DEBUG")) {
          return;
        }
      }
      if (((_b = item.parent) == null ? void 0 : _b.type) === "ObjectProperty" && item.parent.shorthand) {
        s.prependRight(item.node.loc.start.offset, `${item.name}: ${accessor}.`);
      } else {
        if (item.name) {
          let offset = item.node.loc.source.indexOf(item.name) + item.node.loc.start.offset;
          if (s.original.slice(offset, offset + item.name.length) !== item.name) {
            offset = item.exp.loc.source.indexOf(item.name) + item.exp.loc.start.offset;
          }
          s.prependRight(offset, `${accessor}.`);
        } else {
          s.prependRight(item.node.loc.start.offset, `${accessor}.`);
        }
      }
    }
  };
  return binding;
}

var hasRequiredBinding;
function requireBinding() {
  if (hasRequiredBinding) return binding$1;
  hasRequiredBinding = 1;
  (function(exports) {
    var __createBinding = binding$1 && binding$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = binding$1 && binding$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireBinding$1(), exports);
  })(binding$1);
  return binding$1;
}

var block$1 = {};

var block = {};

var hasRequiredBlock$1;
function requireBlock$1() {
  if (hasRequiredBlock$1) return block;
  hasRequiredBlock$1 = 1;
  Object.defineProperty(block, "__esModule", { value: true });
  block.BlockPlugin = void 0;
  const template_1 = requireTemplate$3();
  const compiler_core_1 = require$$0$1;
  block.BlockPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterBlock",
    conditions: /* @__PURE__ */ new Map(),
    contexts: /* @__PURE__ */ new Map(),
    addItem(element, parent, ctx) {
      let parentBlock = this.conditions.get(parent);
      if (parentBlock) {
        parentBlock.push(element);
      } else {
        this.conditions.set(parent, [element]);
      }
      if (ctx)
        this.contexts.set(element, ctx);
    },
    pre() {
      this.conditions.clear();
    },
    post(s, ctx) {
      for (const [parent, block2] of this.conditions) {
        const first = block2.shift();
        const last = block2.pop() ?? first;
        const conditions = this.contexts.get(first);
        const behaviour = parent.type === compiler_core_1.NodeTypes.ROOT ? "append" : "prepend";
        s[`${behaviour}${(conditions == null ? void 0 : conditions.blockDirection) ?? "Left"}`](first.loc.start.offset, "{()=>{");
        if (ctx.doNarrow && (conditions == null ? void 0 : conditions.conditions.length)) {
          ctx.doNarrow({
            index: first.loc.end.offset,
            inBlock: true,
            conditions: conditions.conditions,
            type: behaviour,
            direction: (conditions == null ? void 0 : conditions.blockDirection) === "Right" ? "right" : "left",
            condition: null,
            parent
          }, s);
        }
        s[`${behaviour}Right`](last.loc.end.offset, "}}");
      }
    },
    transformCondition(item) {
      if (item.element.tag === "template" && item.element.props.find((x) => x.name === "slot")) {
        return;
      }
      this.addItem(item.element, item.parent, item.context);
    },
    transformLoop(item) {
    },
    transformSlotDeclaration(item) {
      this.addItem(item.node, item.parent);
    },
    transformSlotRender(item) {
    },
    transformElement(item) {
      if (item.tag === "template" && !item.node.props.find((x) => x.name === "slot")) {
        this.addItem(item.node, item.parent);
      }
    }
  });
  return block;
}

var hasRequiredBlock;
function requireBlock() {
  if (hasRequiredBlock) return block$1;
  hasRequiredBlock = 1;
  (function(exports) {
    var __createBinding = block$1 && block$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = block$1 && block$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireBlock$1(), exports);
  })(block$1);
  return block$1;
}

var comment$1 = {};

var comment = {};

var hasRequiredComment$1;
function requireComment$1() {
  if (hasRequiredComment$1) return comment;
  hasRequiredComment$1 = 1;
  Object.defineProperty(comment, "__esModule", { value: true });
  comment.CommentPlugin = void 0;
  comment.CommentPlugin = {
    name: "VerterComment",
    transformComment(item, s) {
      const relativeStart = item.node.loc.source.indexOf(item.content);
      const relativeEnd = relativeStart + item.content.length;
      const wrap = item.content.indexOf(">") >= 0 ? true : item.content.indexOf("<") >= 0;
      s.overwrite(item.node.loc.start.offset, item.node.loc.start.offset + relativeStart, `${wrap ? "{" : ""}/*`);
      s.overwrite(item.node.loc.start.offset + relativeEnd, item.node.loc.end.offset, `*/${wrap ? "}" : ""}`);
    }
  };
  return comment;
}

var hasRequiredComment;
function requireComment() {
  if (hasRequiredComment) return comment$1;
  hasRequiredComment = 1;
  (function(exports) {
    var __createBinding = comment$1 && comment$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = comment$1 && comment$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireComment$1(), exports);
  })(comment$1);
  return comment$1;
}

var conditional$1 = {};

var conditional = {};

var hasRequiredConditional$1;
function requireConditional$1() {
  if (hasRequiredConditional$1) return conditional;
  hasRequiredConditional$1 = 1;
  Object.defineProperty(conditional, "__esModule", { value: true });
  conditional.ConditionalPlugin = void 0;
  const compiler_core_1 = require$$0$1;
  const template_1 = requireTemplate$3();
  conditional.ConditionalPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterConditional",
    processed: /* @__PURE__ */ new Set(),
    // narrows: [] as {
    //   index: number;
    //   inBlock: boolean;
    //   conditions: TemplateCondition[];
    // }[],
    pre(_, ctx) {
      this.processed.clear();
      ctx.doNarrow = (narrow, s) => {
        if (narrow.parent) {
          if (this.processed.has(narrow.parent))
            return;
          this.processed.add(narrow.parent);
        }
        const conditions = narrow.conditions.filter((x) => x !== narrow.condition);
        if (conditions.length > 0) {
          const condition = narrow.inBlock ? generateBlockCondition(conditions, s) : generateTernaryCondition(conditions, s);
          if (narrow.type === "append") {
            if (narrow.direction === "right") {
              s.appendRight(narrow.index, condition);
            } else {
              s.appendLeft(narrow.index, condition);
            }
          } else {
            if (narrow.direction === "right") {
              s.prependRight(narrow.index, condition);
            } else {
              s.prependLeft(narrow.index, condition);
            }
          }
        }
      };
    },
    // post(s, ctx) {
    //   if (!ctx.toNarrow) return;
    //   for (const narrow of ctx.toNarrow) {
    //     const conditions = narrow.conditions.filter(
    //       (x) => x !== narrow.condition
    //     );
    //     if (conditions.length > 0) {
    //       const condition = narrow.inBlock
    //         ? generateBlockCondition(conditions, s)
    //         : generateTernaryCondition(conditions, s);
    //       if (narrow.type === "append") {
    //         if (narrow.direction === "right") {
    //           s.appendRight(narrow.index, condition);
    //         } else {
    //           s.appendLeft(narrow.index, condition);
    //         }
    //       } else {
    //         if (narrow.direction === "right") {
    //           s.prependRight(narrow.index, condition);
    //         } else {
    //           s.prependLeft(narrow.index, condition);
    //         }
    //       }
    //     }
    //   }
    // },
    transformCondition(item, s, ctx) {
      const element = item.element;
      const node = item.node;
      const rawName = node.rawName;
      this.processed.add(element);
      const canMove = !(element.tag === "template" && element.props.find((x) => x.name === "slot"));
      {
        const siblings = "children" in item.parent ? item.parent.children : [];
        if (siblings[0] !== element) {
          const comments = [];
          for (let i = 0; i < siblings.length; i++) {
            const e = siblings[i];
            if (element === e) {
              break;
            }
            if (e.type === compiler_core_1.NodeTypes.COMMENT) {
              comments.push(e);
            } else {
              comments.length = 0;
            }
          }
          if (comments.length) {
            const from = Math.min(...comments.map((x) => x.loc.start.offset));
            s.move(from, element.loc.start.offset - 1, element.loc.start.offset);
          }
        }
      }
      if (canMove) {
        s.move(node.loc.start.offset, node.loc.end.offset, element.loc.start.offset);
      }
      if (node.name === "else-if") {
        s.overwrite(node.loc.start.offset + 6, node.loc.start.offset + 7, " ");
      }
      s.remove(node.loc.start.offset, node.loc.start.offset + 2);
      if (node.exp) {
        s.remove(node.loc.start.offset + rawName.length, node.loc.start.offset + rawName.length + 1);
        s.overwrite(node.exp.loc.start.offset - 1, node.exp.loc.start.offset, "(");
        s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, "){");
      } else {
        s.prependLeft(node.loc.start.offset + rawName.length, "{");
      }
      s.prependLeft(element.loc.end.offset, "}");
      if (ctx.narrow !== false) {
        if (item.context.conditions.length > 0) {
          const condition = generateBlockCondition(item.context.conditions, s);
          s.prependLeft(element.loc.start.offset, condition);
        }
      }
    },
    // transform(item, s, ctx) {
    //   if (item.type !== TemplateTypes.Function) {
    //     return;
    //   }
    //   console.log("item", item);
    //   const context = item.context as ParseTemplateContext;
    //   if (context.conditions.length === 0) {
    //     return;
    //   }
    //   const conditions = context.conditions;
    //   const node = item.node;
    //   conditions.map((x) =>
    //     s.slice(x.node.loc.start.offset, x.node.loc.end.offset).toString()
    //   );
    // },
    transformFunction(item, s, ctx) {
      if (ctx.narrow === void 0 || ctx.narrow === false || ctx.narrow !== true && !ctx.narrow.functions) {
        return;
      }
      const context = item.context;
      if (context.conditions.length === 0) {
        return;
      }
      const conditions = context.conditions;
      const node = item.node;
      const inBlock = node.type.indexOf("Arrow") === -1;
      s.prependLeft(item.body.loc.start.offset, inBlock ? generateBlockCondition(conditions, s) : generateTernaryCondition(conditions, s));
    }
  });
  function generateBlockCondition(conditions, s) {
    const text = generateConditionText(conditions, s);
    return `if(!(${text})) return;`;
  }
  function generateTernaryCondition(conditions, s) {
    const text = generateConditionText(conditions, s);
    return `!(${text})? undefined :`;
  }
  function generateConditionText(conditions, s) {
    const siblings = conditions.map((x) => x.siblings).flat().filter((x) => x);
    let negations = "";
    if (siblings.length > 0) {
      const st = generateConditionText(siblings, s);
      if (st) {
        negations = `!(${st})`;
      }
    }
    const positive = conditions.map((x) => s.slice(x.node.loc.start.offset, x.node.loc.end.offset).toString()).filter((x) => x).map((x) => {
      const ending = x.endsWith("{") ? -1 : x.length;
      if (x.startsWith("if")) {
        x = x.slice(2, ending);
      } else if (x.startsWith("else if")) {
        x = x.slice(7, ending);
      } else if (x.startsWith("else")) {
        x = x.slice(4, ending);
      }
      return x;
    }).join(" && ");
    return [negations, positive].filter((x) => x).join(" && ");
  }
  return conditional;
}

var hasRequiredConditional;
function requireConditional() {
  if (hasRequiredConditional) return conditional$1;
  hasRequiredConditional = 1;
  (function(exports) {
    var __createBinding = conditional$1 && conditional$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = conditional$1 && conditional$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireConditional$1(), exports);
  })(conditional$1);
  return conditional$1;
}

var interpolation$1 = {};

var interpolation = {};

var hasRequiredInterpolation$1;
function requireInterpolation$1() {
  if (hasRequiredInterpolation$1) return interpolation;
  hasRequiredInterpolation$1 = 1;
  Object.defineProperty(interpolation, "__esModule", { value: true });
  interpolation.InterpolationPlugin = void 0;
  interpolation.InterpolationPlugin = {
    name: "VerterInterpolation",
    transformInterpolation(item, s) {
      s.remove(item.node.loc.start.offset, item.node.loc.start.offset + 1);
      s.remove(item.node.loc.end.offset - 1, item.node.loc.end.offset);
    }
  };
  return interpolation;
}

var hasRequiredInterpolation;
function requireInterpolation() {
  if (hasRequiredInterpolation) return interpolation$1;
  hasRequiredInterpolation = 1;
  (function(exports) {
    var __createBinding = interpolation$1 && interpolation$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = interpolation$1 && interpolation$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireInterpolation$1(), exports);
  })(interpolation$1);
  return interpolation$1;
}

var prop$1 = {};

var prop = {};

var directive$1 = {};

var directive = {};

var hasRequiredDirective$1;
function requireDirective$1() {
  if (hasRequiredDirective$1) return directive;
  hasRequiredDirective$1 = 1;
  Object.defineProperty(directive, "__esModule", { value: true });
  directive.DirectivePlugin = void 0;
  const compiler_core_1 = require$$0$1;
  const template_1 = requireTemplate$3();
  const binding_1 = requireBinding();
  const vue_1 = require$$1$2;
  directive.DirectivePlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterDirective",
    // create type safe modifiers
    handleDirectiveModifiers(node, context, s, ctx) {
      if (node.modifiers.length === 0) {
        return;
      }
      const directiveAccessor = ctx.retrieveAccessor("directiveAccessor");
      const instancePropertySymbol = ctx.retrieveAccessor("instancePropertySymbol");
      const slotInstance = ctx.retrieveAccessor("slotInstance");
      const instanceToDirectiveFn = ctx.retrieveAccessor("instanceToDirectiveFn");
      const instanceToDirectiveVar = ctx.retrieveAccessor("instanceToDirectiveVar");
      const directiveName = ctx.retrieveAccessor("directiveName");
      const index = node.loc.start.offset;
      const declaration = `const ${instanceToDirectiveVar}=${instanceToDirectiveFn}(${slotInstance});const ${directiveName}=${instanceToDirectiveVar}(${directiveAccessor}.${// NOTE v-model text is the default, maybe we can add more here later
      node.name === "model" ? "vModelText" : `v${(0, vue_1.capitalize)(node.name)}`});`;
      s.prependLeft(index, declaration);
      if (ctx.doNarrow && context.conditions.length > 0) {
        ctx.doNarrow({
          index,
          conditions: context.conditions,
          inBlock: true,
          type: "prepend",
          direction: "left"
        }, s);
      }
      s.prependLeft(index, `{...{[${instancePropertySymbol}]:(${slotInstance})=>{`);
      if (node.modifiers.length > 0) {
        const start = node.modifiers[0].loc.start.offset - 1;
        const end = node.modifiers[node.modifiers.length - 1].loc.end.offset;
        s.move(start, end, index);
        s.overwrite(start, start + 1, `${directiveName}.modifiers=[`);
        s.prependLeft(end, "];");
        for (let i = 0; i < node.modifiers.length; i++) {
          const modifier = node.modifiers[i];
          if (i > 0) {
            s.overwrite(modifier.loc.start.offset - 1, modifier.loc.start.offset, ",");
          }
          s.prependRight(modifier.loc.start.offset, '"');
          s.prependLeft(modifier.loc.end.offset, '"');
        }
      }
      s.prependRight(index, "}}} ");
    },
    transformDirective(item, s, ctx) {
      var _a, _b;
      const element = item.element;
      const node = item.node;
      switch (item.name) {
        case "model": {
          const clonedS = s.clone();
          (_a = item.exp) == null ? void 0 : _a.forEach((x) => {
            binding_1.BindingPlugin.transformBinding(x, clonedS, ctx);
          });
          (_b = item.arg) == null ? void 0 : _b.forEach((x) => {
            binding_1.BindingPlugin.transformBinding(x, clonedS, ctx);
          });
          const fallbakName = element.tagType === compiler_core_1.ElementTypes.ELEMENT ? "value" : "modelValue";
          let bindingTo = element.tagType === compiler_core_1.ElementTypes.ELEMENT ? "input" : "modelValue";
          let isDynamic = false;
          if (node.arg) {
            const arg = node.arg;
            if (!node.arg.ast && arg.isStatic) {
              bindingTo = arg.content;
              s.overwrite(node.loc.start.offset, arg.loc.end.offset, bindingTo);
            } else {
              isDynamic = true;
              s.overwrite(node.loc.start.offset, arg.loc.start.offset, "");
              if (s.original[arg.loc.end.offset] === "=") {
                s.overwrite(arg.loc.end.offset, arg.loc.end.offset + 1, ":");
              }
              s.prependLeft(node.loc.start.offset, "{...{");
            }
          } else {
            s.overwrite(node.loc.start.offset, node.loc.start.offset + "v-model".length, fallbakName);
          }
          if (node.exp) {
            if (isDynamic) {
              s.remove(node.exp.loc.start.offset - 1, node.exp.loc.start.offset);
            } else {
              s.overwrite(node.exp.loc.start.offset - 1, node.exp.loc.start.offset, "{");
            }
            const exp = clonedS.slice(node.exp.loc.start.offset, node.exp.loc.end.offset);
            if (isDynamic) {
              bindingTo = clonedS.slice(node.arg.loc.start.offset, node.arg.loc.end.offset).toString().slice(1, -1);
            }
            const eventName = element.tagType === compiler_core_1.ElementTypes.ELEMENT ? "onInput" : isDynamic ? "onUpdate" : `onUpdate:${bindingTo}`;
            const valueAccessor = element.tagType === compiler_core_1.ElementTypes.ELEMENT ? "$event.target.value" : "$event";
            const pre = isDynamic ? `,[\`${eventName}:\${${bindingTo}}\`]:` : `} ${eventName}={`;
            const post = isDynamic ? "}}" : "}";
            s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, `${pre}($event)=>(${exp.toString()}=${valueAccessor})${post}`);
          } else {
            ctx.items.push({
              type: "warning",
              message: "NO_EXPRESSION_VMODEL",
              start: node.loc.start.offset,
              end: node.loc.end.offset,
              node
            });
          }
          this.handleDirectiveModifiers(node, item.context, s, ctx);
          break;
        }
        case "is": {
          if (item.element.tag === "component") {
            return;
          }
        }
        default: {
          const directiveAccessor = ctx.retrieveAccessor("directiveAccessor");
          const instancePropertySymbol = ctx.retrieveAccessor("instancePropertySymbol");
          const slotInstance = ctx.retrieveAccessor("slotInstance");
          const instanceToDirectiveFn = ctx.retrieveAccessor("instanceToDirectiveFn");
          const instanceToDirectiveVar = ctx.retrieveAccessor("instanceToDirectiveVar");
          const directiveName = ctx.retrieveAccessor("directiveName");
          const context = item.context;
          if (ctx.doNarrow && context.conditions.length > 0) {
            ctx.doNarrow({
              index: node.loc.start.offset,
              conditions: context.conditions,
              inBlock: true,
              type: "prepend",
              direction: "left"
            }, s);
          }
          const declaration = `const ${instanceToDirectiveVar}=${instanceToDirectiveFn}(${slotInstance});const ${directiveName}=${instanceToDirectiveVar}(`;
          s.prependLeft(node.loc.start.offset, `{...{[${instancePropertySymbol}]:(${slotInstance})=>{`);
          s.prependRight(node.loc.start.offset, `${directiveAccessor}.`);
          s.prependRight(node.loc.start.offset, declaration);
          s.overwrite(node.loc.start.offset + 1, node.loc.start.offset + 3, item.name[0].toUpperCase());
          s.prependLeft(node.loc.start.offset + 2 + item.name.length, ");");
          if (node.arg) {
            const arg = node.arg;
            s.overwrite(arg.loc.start.offset - 1, arg.loc.start.offset, "=");
            s.prependRight(arg.loc.start.offset - 1, `${directiveName}.arg`);
            s.prependLeft(arg.loc.end.offset, ";");
            if (arg.isStatic) {
              s.prependLeft(arg.loc.start.offset, '"');
              s.prependLeft(arg.loc.end.offset, '"');
            }
          }
          if (node.modifiers.length > 0) {
            const start = node.modifiers[0].loc.start.offset;
            const end = node.modifiers[node.modifiers.length - 1].loc.end.offset;
            s.overwrite(start - 1, start, `${directiveName}.modifiers=[`);
            s.prependLeft(end, "];");
            for (let i = 0; i < node.modifiers.length; i++) {
              const modifier = node.modifiers[i];
              if (i > 0) {
                s.overwrite(modifier.loc.start.offset - 1, modifier.loc.start.offset, ",");
              }
              s.prependRight(modifier.loc.start.offset, '"');
              s.prependLeft(modifier.loc.end.offset, '"');
            }
          }
          if (node.exp) {
            s.overwrite(node.exp.loc.start.offset - 2, node.exp.loc.start.offset, `${directiveName}.value=`);
            s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, ";");
          }
          s.prependRight(node.loc.end.offset, "}}}");
        }
      }
    }
  });
  return directive;
}

var hasRequiredDirective;
function requireDirective() {
  if (hasRequiredDirective) return directive$1;
  hasRequiredDirective = 1;
  (function(exports) {
    var __createBinding = directive$1 && directive$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = directive$1 && directive$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireDirective$1(), exports);
  })(directive$1);
  return directive$1;
}

var hasRequiredProp$1;
function requireProp$1() {
  if (hasRequiredProp$1) return prop;
  hasRequiredProp$1 = 1;
  Object.defineProperty(prop, "__esModule", { value: true });
  prop.PropPlugin = void 0;
  const template_1 = requireTemplate$3();
  const compiler_core_1 = require$$0$1;
  const vue_1 = require$$1$2;
  const directive_1 = requireDirective();
  const utils_1 = requireUtils$1();
  function overrideCamelCase(loc, s, ctx) {
    if (ctx.camelWhitelistAttributes(loc.source)) {
      return;
    }
    const offset = loc.start.offset;
    for (const match of loc.source.matchAll(/-([a-z])/g)) {
      s.overwrite(offset + match.index, offset + match.index + 2, match[1].toUpperCase());
    }
  }
  prop.PropPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterProp",
    used: {
      normalizeStyle: false,
      normalizeClass: false
    },
    pre() {
      this.used.normalizeStyle = false;
      this.used.normalizeClass = false;
    },
    post(s, ctx) {
      if (!this.used.normalizeClass && !this.used.normalizeStyle) {
        return;
      }
      const items = [
        this.used.normalizeClass && {
          name: "normalizeClass",
          alias: ctx.retrieveAccessor("normalizeClass")
        },
        this.used.normalizeStyle && {
          name: "normalizeStyle",
          alias: ctx.retrieveAccessor("normalizeStyle")
        }
      ].filter(Boolean);
      const importStr = (0, utils_1.generateImport)([
        {
          from: "vue",
          items
        }
      ]);
      s.prepend(importStr);
    },
    transformProp(prop2, s, ctx) {
      var _a, _b, _c, _d, _e, _f, _g, _h, _i, _j, _k, _l;
      if (prop2.node === null) {
        const accessorType = prop2.name === "style" ? "normalizeStyle" : "normalizeClass";
        const normaliseAccessor = ctx.retrieveAccessor(accessorType);
        const nodes = prop2.props.map((x) => x.node).filter((x) => x !== null);
        const firstDirective = nodes.find((x) => x.type === compiler_core_1.NodeTypes.DIRECTIVE && x.exp);
        if (!firstDirective) {
          return;
        }
        if (this.used[accessorType] === false) {
          this.used[accessorType] = true;
        }
        if ((_a = firstDirective.rawName) == null ? void 0 : _a.startsWith("v-bind:")) {
          s.remove(firstDirective.loc.start.offset, firstDirective.loc.start.offset + 7);
        } else if ((_b = firstDirective.rawName) == null ? void 0 : _b.startsWith(":")) {
          s.remove(firstDirective.loc.start.offset, firstDirective.loc.start.offset + 1);
        }
        s.overwrite(firstDirective.exp.loc.start.offset - 1, firstDirective.exp.loc.start.offset, "{");
        s.overwrite(firstDirective.exp.loc.end.offset, firstDirective.exp.loc.end.offset + 1, "}");
        s.prependLeft(firstDirective.exp.loc.start.offset, `${normaliseAccessor}([`);
        for (let i = 0; i < nodes.length; i++) {
          const node = nodes[i];
          if (node === firstDirective) {
            continue;
          }
          const loc = node.type === compiler_core_1.NodeTypes.DIRECTIVE ? (_c = node.exp ?? node.arg) == null ? void 0 : _c.loc : (_d = node.value) == null ? void 0 : _d.loc;
          if (loc) {
            s.prependRight(loc.start.offset, `,`);
            s.move(loc.start.offset, loc.end.offset, firstDirective.exp.loc.end.offset);
          }
          const nameLoc = node.type === compiler_core_1.NodeTypes.DIRECTIVE ? (_e = node.arg) == null ? void 0 : _e.loc : node.nameLoc;
          if (nameLoc) {
            s.remove(nameLoc.start.offset, nameLoc.end.offset + 1);
          }
        }
        s.appendRight(firstDirective.exp.loc.end.offset, "])");
      } else if ((prop2.name === "is" || prop2.node.type === compiler_core_1.NodeTypes.DIRECTIVE && prop2.node.rawName === ":is") && prop2.element.tag === "component") {
        return;
      } else if (prop2.static) {
        if (prop2.node.value) {
          s.prependRight(prop2.node.value.loc.start.offset, "{");
          s.prependLeft(prop2.node.value.loc.end.offset, "}");
        }
        if (prop2.node.nameLoc) {
          overrideCamelCase(prop2.node.nameLoc, s, ctx);
        }
      } else {
        const [nameBinding] = prop2.arg ?? [];
        if ((nameBinding == null ? void 0 : nameBinding.ignore) === true || (nameBinding == null ? void 0 : nameBinding.skip)) {
          overrideCamelCase(nameBinding.node.loc, s, ctx);
        }
        const node = prop2.node;
        if ((_f = node.rawName) == null ? void 0 : _f.startsWith("v-bind:")) {
          s.remove(node.loc.start.offset, node.loc.start.offset + 7);
        } else if ((_g = node.rawName) == null ? void 0 : _g.startsWith(":")) {
          s.remove(node.loc.start.offset, node.loc.start.offset + 1);
        } else if ((_h = node.rawName) == null ? void 0 : _h.startsWith("v-on:")) {
          if ((nameBinding == null ? void 0 : nameBinding.ignore) === true) {
            s.overwrite(node.loc.start.offset + 5, node.loc.start.offset + 6, ((_i = nameBinding.name.at(0)) == null ? void 0 : _i.toUpperCase()) ?? "");
          }
          s.overwrite(node.loc.start.offset, node.loc.start.offset + 5, "on");
        } else if ((_j = node.rawName) == null ? void 0 : _j.startsWith("@")) {
          if ((nameBinding == null ? void 0 : nameBinding.ignore) === true) {
            s.overwrite(node.loc.start.offset + 1, node.loc.start.offset + 2, ((_k = nameBinding.name.at(0)) == null ? void 0 : _k.toUpperCase()) ?? "");
          }
          s.overwrite(node.loc.start.offset, node.loc.start.offset + 1, "on");
        }
        if ((_l = node.arg) == null ? void 0 : _l.loc.source.startsWith("[")) {
          s.prependRight(node.arg.loc.start.offset, "{...{");
          if (node.exp) {
            s.overwrite(node.exp.loc.start.offset - 2, node.exp.loc.start.offset, ":");
            s.remove(node.exp.loc.end.offset, node.exp.loc.end.offset + 1);
          }
          s.prependLeft(node.loc.end.offset, "}}");
        } else {
          if (node.exp) {
            s.overwrite(node.exp.loc.start.offset - 1, node.exp.loc.start.offset, "{");
            s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, "}");
          } else if ((nameBinding == null ? void 0 : nameBinding.ignore) === true || (nameBinding == null ? void 0 : nameBinding.skip)) {
            const accessor = ctx.retrieveAccessor("ctx");
            s.appendLeft(node.loc.end.offset, `={${accessor}.${(0, vue_1.camelize)(nameBinding.name)}}`);
          }
        }
        directive_1.DirectivePlugin.handleDirectiveModifiers(node, prop2.context, s, ctx);
      }
    }
  });
  return prop;
}

var hasRequiredProp;
function requireProp() {
  if (hasRequiredProp) return prop$1;
  hasRequiredProp = 1;
  (function(exports) {
    var __createBinding = prop$1 && prop$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = prop$1 && prop$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireProp$1(), exports);
  })(prop$1);
  return prop$1;
}

var slot$1 = {};

var slot = {};

var hasRequiredSlot$1;
function requireSlot$1() {
  if (hasRequiredSlot$1) return slot;
  hasRequiredSlot$1 = 1;
  Object.defineProperty(slot, "__esModule", { value: true });
  slot.SlotPlugin = void 0;
  const compiler_core_1 = require$$0$1;
  const template_1 = requireTemplate$3();
  slot.SlotPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterSlot",
    processedParent: /* @__PURE__ */ new Set(),
    pre() {
      this.processedParent.clear();
    },
    // slots: new Set<ElementNode>(),
    // pre() {
    //   this.slots.clear();
    // },
    // post(s, ctx) {
    //   const slotInstance = ctx.retrieveAccessor("slotInstance");
    //   // const $slots = ctx.retrieveAccessor("$slot");
    //   // move children to v-slot and initialise $slots
    //   for (const element of this.slots) {
    //     const children = element.children;
    //     const first = children.shift();
    //     // nothing to do if there's no children
    //     if (!first) {
    //       break;
    //     }
    //     const last = children.pop() ?? first;
    //     const pos =
    //       element.loc.start.offset +
    //       element.loc.source
    //         .slice(0, first.loc.start.offset - element.loc.start.offset)
    //         .lastIndexOf(">");
    //     const endPos =
    //       element.loc.source
    //         .slice(last.loc.end.offset - element.loc.start.offset)
    //         .indexOf("<") + last.loc.end.offset;
    //     if (pos === -1 || pos >= first.loc.start.offset) {
    //       console.log("should not happen");
    //       continue;
    //     }
    //     // TODO narrow
    //     s.prependLeft(pos, ` v-slot={(${slotInstance})=>{`);
    //     s.move(pos + 1, endPos, pos);
    //     s.prependRight(pos, "}}");
    //   }
    // },
    handleSlotRender(slot2, s, parent, ctx) {
      if (this.processedParent.has(parent))
        return;
      const slotInstance = ctx.retrieveAccessor("slotInstance");
      const isTemplateSlot = !!slot2.parent;
      const children = [...parent.children];
      const parentSource = parent.loc.source;
      const endTagPos = parentSource.lastIndexOf("</") + parent.loc.start.offset;
      const tagEnd = parent.loc.start.offset + parentSource.slice(0, parent.children.length === 0 ? endTagPos - parent.loc.start.offset : children[0].loc.start.offset - parent.loc.start.offset).lastIndexOf(">");
      let pos = -1;
      if (isTemplateSlot) {
        const first = children.shift();
        if (!first) {
          this.processedParent.add(parent);
          return;
        }
        children.pop() ?? first;
        if (!parent.tag) {
          throw new Error("Parent tag is missing");
        }
        pos = parent.loc.start.offset + parent.tag.length + 1;
        if (pos === -1 || pos >= first.loc.start.offset) {
          console.log("should not happen");
          debugger;
          return;
        }
      } else {
        pos = slot2.prop.node.loc.start.offset;
      }
      s.prependLeft(pos, ` ${ctx.prefix("v-slot")}={(${slotInstance})=>{`);
      if (ctx.doNarrow && slot2.context.conditions.length > 0) {
        ctx.doNarrow({
          index: pos,
          inBlock: true,
          conditions: slot2.context.conditions,
          type: "append",
          // type: "prepend",
          // direction: "right",
          condition: slot2.condition
        }, s);
      }
      if (isTemplateSlot) {
        if (tagEnd < endTagPos - 2) {
          s.move(tagEnd + 1, endTagPos, pos);
        }
        s.prependRight(pos, "}}");
      } else {
        if (tagEnd < endTagPos - 2) {
          s.move(tagEnd + 1, endTagPos, slot2.prop.node.loc.end.offset);
        }
        s.prependRight(slot2.prop.node.loc.end.offset, "}}");
      }
      this.processedParent.add(parent);
      return pos;
    },
    /**
         *
         * Example:
    const $slots = defineSlots<{
        default: () => HTMLDivElement[],
    
        input: (a: { name: string }) => HTMLInputElement
    }>()
    
    declare function PatchSlots<T extends Record<string, (...args: any[]) => any>>(slots: T): {
        [K in keyof T]: T[K] extends () => any ? (props: {}) => JSX.Element : (...props: Parameters<T[K]>) => JSX.Element
    }
    
    const $slots = PatchSlots(c.$slots);
    
    
    
    
    
         * @param slot
         * @param s
         * @param ctx
         */
    transformSlotDeclaration(item, s, ctx) {
      const $slots = ctx.retrieveAccessor("$slot");
      const renderSlot = ctx.retrieveAccessor("slotComponent");
      const node = item.node;
      if (node.type === compiler_core_1.NodeTypes.ELEMENT) {
        s.overwrite(node.loc.start.offset + 1, node.loc.start.offset + 5, renderSlot);
        if (!node.isSelfClosing) {
          const nameIndex = node.loc.source.indexOf("slot", node.loc.source.lastIndexOf("</")) + node.loc.start.offset;
          s.overwrite(nameIndex, nameIndex + 4, renderSlot);
        }
        s.prependRight(node.loc.start.offset + 1, `<`);
        const insertIndex = node.loc.start.offset + 1;
        s.prependRight(insertIndex, ";");
        if (item.name) {
          if (!Array.isArray(item.name) && item.name.node) {
            if (item.name.node.type === compiler_core_1.NodeTypes.ATTRIBUTE) {
              const prop = item.name.node;
              s.move(prop.loc.start.offset, prop.loc.end.offset, insertIndex);
              if (prop.value) {
                s.overwrite(prop.nameLoc.start.offset, prop.value.loc.start.offset + 1, '["');
                s.overwrite(prop.value.loc.end.offset - 1, prop.value.loc.end.offset, '"]');
              }
            } else if ("directive" in item.name && item.name.directive) {
              const directive = item.name.directive;
              s.move(directive.loc.start.offset, directive.loc.end.offset, insertIndex);
              if (directive.arg) {
                s.remove(directive.arg.loc.start.offset, directive.arg.loc.end.offset);
              }
              if (directive.exp) {
                s.overwrite(directive.exp.loc.start.offset - 2, directive.exp.loc.start.offset, "[");
                s.overwrite(directive.exp.loc.end.offset, directive.exp.loc.end.offset + 1, "]");
              }
            }
          }
        } else {
          s.prependLeft(insertIndex, `.default`);
        }
        s.prependLeft(insertIndex, `const ${renderSlot}=${$slots}`);
        s.update(node.loc.start.offset, node.loc.start.offset + 1, "");
      }
    },
    /**
       *
       *
       * slotRender aka ___VERTER___SLOT_CALLBACK
    declare function ___VERTER___SLOT_CALLBACK<T>(slot?: (...args: T[]) => any): (cb: ((...args: T[]) => any))=>void;
    
    
    <div v-slot={(ci):any=>{
      const $slots = ci.$slots;
      ___VERTER___SLOT_CALLBACK($slots.default)(({})=>{
        return <div></div>
      })
    }}
       * @param slot
       * @param s
       * @param ctx
       */
    transformSlotRender(slot2, s, ctx) {
      var _a, _b;
      const slotRender = ctx.retrieveAccessor("slotRender");
      const slotInstance = ctx.retrieveAccessor("slotInstance");
      if (slot2.parent) {
        this.handleSlotRender(slot2, s, slot2.parent, ctx);
        const node = slot2.element;
        if (node.type === compiler_core_1.NodeTypes.ELEMENT) {
          const insertIndex = node.loc.start.offset + 1;
          s.update(node.loc.start.offset, node.loc.start.offset + 1, "");
          const tagEnd = node.isSelfClosing ? node.loc.end.offset - 2 : node.loc.source.slice(0, (((_a = node.children[0]) == null ? void 0 : _a.loc.start.offset) ?? node.loc.end.offset - "</template>".length) - node.loc.start.offset).lastIndexOf(">") + node.loc.start.offset;
          s.move(node.loc.start.offset + "<template".length, tagEnd, insertIndex);
          const prop = slot2.prop;
          if (prop.type === "Directive") {
            const start = prop.node.loc.start.offset;
            const end = start + (((_b = prop.node.rawName) == null ? void 0 : _b.startsWith("v-slot:")) ? 7 : 1);
            s.prependLeft(start, `${slotRender}(${slotInstance}.`);
            s.overwrite(start, end, `$slots`);
            if (prop.node.arg) {
              if (prop.node.arg.type === compiler_core_1.NodeTypes.SIMPLE_EXPRESSION && prop.node.arg.isStatic) {
                if (/^\w+$/.test(prop.node.arg.content)) {
                  s.prependLeft(end, ".");
                } else {
                  s.prependLeft(end, `['`);
                  s.appendLeft(prop.node.arg.loc.end.offset, `']`);
                }
              }
            }
            if (prop.node.exp) {
              s.overwrite(prop.node.exp.loc.start.offset - 2, prop.node.exp.loc.start.offset - 1, ")");
              s.prependLeft(prop.node.exp.loc.start.offset - 1, "(");
              s.overwrite(prop.node.exp.loc.start.offset - 1, prop.node.exp.loc.start.offset, "(");
              s.overwrite(prop.node.exp.loc.end.offset, prop.node.exp.loc.end.offset + 1, ")");
              s.prependLeft(prop.node.exp.loc.end.offset + 1, "=>{");
            } else {
              s.appendLeft(tagEnd, `)(()=>{`);
            }
            if (ctx.doNarrow && slot2.context.conditions.length > 0) {
              ctx.doNarrow({
                index: prop.node.exp ? prop.node.exp.loc.end.offset + 1 : tagEnd,
                inBlock: true,
                conditions: slot2.context.conditions,
                type: "append",
                // empty condition because we need the current slot condition to also apply if present
                condition: null
              }, s);
            }
            s.prependLeft(node.loc.end.offset, "});");
          } else {
            throw new Error("TODO: handle slot directive");
          }
          s.prependRight(node.loc.start.offset + 1, "<");
        } else {
          throw new Error("TODO handle slot not being an element");
        }
      } else {
        const element = slot2.element;
        this.handleSlotRender(slot2, s, element, ctx);
        slot2.prop.node.loc.end.offset;
        const directive = slot2.prop.node;
        s.overwrite(slot2.prop.node.loc.start.offset, slot2.prop.node.loc.start.offset + (directive.rawName.startsWith("#") ? 1 : "v-slot".length), `$slots`);
        s.prependRight(directive.loc.start.offset, `${slotRender}(${slotInstance}.`);
        if (directive.exp) {
          s.overwrite(directive.exp.loc.start.offset - 1, directive.exp.loc.start.offset, "");
          s.overwrite(directive.exp.loc.end.offset, directive.exp.loc.end.offset + 1, "");
        }
        if (directive.arg) {
          const arg = directive.arg;
          if (arg.isStatic) {
            if (directive.rawName.startsWith("v-")) {
              s.update(arg.loc.start.offset - 1, arg.loc.start.offset, ".");
            } else {
              s.prependLeft(arg.loc.start.offset, ".");
            }
          } else if (directive.rawName.startsWith("v-")) {
            s.remove(arg.loc.start.offset - 1, arg.loc.start.offset);
          }
          if (directive.exp) {
            s.update(directive.arg.loc.end.offset, directive.exp.loc.start.offset, "");
            s.prependLeft(directive.exp.loc.start.offset, `)((`);
            s.prependLeft(directive.exp.loc.end.offset, `)=>{`);
          } else {
            s.prependLeft(directive.arg.loc.end.offset, ")(()=>{");
          }
        } else {
          if (directive.exp) {
            s.update(directive.loc.start.offset + (directive.rawName.startsWith("#") ? 1 : "v-slot".length), directive.loc.start.offset + (directive.rawName.startsWith("#") ? 2 : "v-slot=".length), "");
            s.prependLeft(directive.exp.loc.start.offset, `.default)((`);
            s.prependLeft(directive.exp.loc.end.offset, `)=>{`);
          } else {
            s.appendLeft(directive.loc.end.offset, `.default`);
            s.appendLeft(directive.loc.end.offset, `)(()=>{`);
          }
        }
        if (ctx.doNarrow && slot2.context.conditions.length > 0) {
          ctx.doNarrow({
            index: directive.loc.end.offset,
            inBlock: true,
            conditions: slot2.context.conditions,
            type: "append",
            // empty condition because we need the current slot condition to also apply if present
            condition: null
          }, s);
        }
        s.prependRight(directive.loc.end.offset, "})");
      }
    }
  });
  return slot;
}

var hasRequiredSlot;
function requireSlot() {
  if (hasRequiredSlot) return slot$1;
  hasRequiredSlot = 1;
  (function(exports) {
    var __createBinding = slot$1 && slot$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = slot$1 && slot$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireSlot$1(), exports);
  })(slot$1);
  return slot$1;
}

var text$1 = {};

var text = {};

var hasRequiredText$1;
function requireText$1() {
  if (hasRequiredText$1) return text;
  hasRequiredText$1 = 1;
  Object.defineProperty(text, "__esModule", { value: true });
  text.TextPlugin = void 0;
  text.TextPlugin = {
    name: "VerterText",
    transformText(item, s) {
      const content = item.content.trim();
      if (!content) {
        return;
      }
      if (content === "<") {
        return;
      }
      for (let i = 0; i < content.length; i++) {
        if (content[i] === '"') {
          s.overwrite(item.node.loc.start.offset + i, item.node.loc.start.offset + i + 1, '\\"');
        }
      }
      const start = item.node.loc.source.indexOf(content) + item.node.loc.start.offset;
      const end = start + content.length;
      s.prependRight(start, '{"');
      s.prependLeft(end, '"}');
    }
  };
  return text;
}

var hasRequiredText;
function requireText() {
  if (hasRequiredText) return text$1;
  hasRequiredText = 1;
  (function(exports) {
    var __createBinding = text$1 && text$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = text$1 && text$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireText$1(), exports);
  })(text$1);
  return text$1;
}

var event$1 = {};

var event = {};

var hasRequiredEvent$1;
function requireEvent$1() {
  if (hasRequiredEvent$1) return event;
  hasRequiredEvent$1 = 1;
  Object.defineProperty(event, "__esModule", { value: true });
  event.EventPlugin = void 0;
  const template_1 = requireTemplate$3();
  const IgnoredASTTypes = /* @__PURE__ */ new Set([
    "MemberExpression",
    "ObjectExpression",
    "ArrowFunctionExpression",
    "FunctionExpression"
  ]);
  event.EventPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterPropEvent",
    inject: false,
    pre() {
      this.inject = false;
    },
    /**
       *
    declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0]) => any>(event: TArgs, cb: R): R;
       */
    post(s, ctx) {
      if (!this.inject)
        return;
      s.append("declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0]) => any>(event: TArgs, cb: R): R;");
    },
    transformProp(prop, s, ctx) {
      if (!prop.event)
        return;
      const { exp } = prop.node;
      if (!exp || !exp.ast) {
        return;
      }
      if (IgnoredASTTypes.has(exp.ast.type)) {
        return;
      }
      this.inject = true;
      const eventCb = ctx.retrieveAccessor("eventCb");
      const eventArgs = ctx.retrieveAccessor("eventArgs");
      s.prependLeft(exp.loc.start.offset, `(...${eventArgs})=>${eventCb}(${eventArgs},($event)=>$event&&0?undefined:`);
      const context = prop.context;
      if (ctx.doNarrow && context.conditions.length > 0) {
        ctx.doNarrow({
          index: exp.loc.start.offset,
          inBlock: false,
          conditions: context.conditions,
          type: "append"
        }, s);
      }
      s.prependLeft(exp.loc.end.offset, ")");
    }
  });
  return event;
}

var hasRequiredEvent;
function requireEvent() {
  if (hasRequiredEvent) return event$1;
  hasRequiredEvent = 1;
  (function(exports) {
    var __createBinding = event$1 && event$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = event$1 && event$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireEvent$1(), exports);
  })(event$1);
  return event$1;
}

var loop$1 = {};

var loop = {};

var hasRequiredLoop$1;
function requireLoop$1() {
  if (hasRequiredLoop$1) return loop;
  hasRequiredLoop$1 = 1;
  Object.defineProperty(loop, "__esModule", { value: true });
  loop.LoopPlugin = void 0;
  const template_1 = requireTemplate$3();
  const block_1 = requireBlock();
  loop.LoopPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterLoop",
    transformLoop(item, s, ctx) {
      const forParseResult = item.node.forParseResult;
      const renderList = ctx.retrieveAccessor("renderList");
      s.move(item.node.loc.start.offset, item.node.loc.end.offset, item.element.loc.start.offset);
      s.overwrite(item.node.loc.start.offset, item.node.loc.start.offset + 5, renderList);
      s.overwrite(item.node.loc.start.offset + 5, item.node.loc.start.offset + 6, "(");
      s.remove(item.node.exp.loc.end.offset, item.node.exp.loc.end.offset + 1);
      const { key, index, value, source } = forParseResult;
      const vforSource = item.node.loc.source;
      s.move(source.loc.start.offset, source.loc.end.offset, item.node.loc.start.offset + 6);
      let inOfIndex = -1;
      const tokens = ["in", "of"];
      const fromIndex = Math.max((key == null ? void 0 : key.loc.end.offset) ?? 0, (index == null ? void 0 : index.loc.end.offset) ?? 0, (value == null ? void 0 : value.loc.end.offset) ?? 0);
      const condition = vforSource.slice(fromIndex - item.node.loc.start.offset);
      for (let i = 0; i < tokens.length; i++) {
        const token = tokens[i];
        const index2 = condition.indexOf(token);
        if (index2 !== -1) {
          inOfIndex = index2;
          break;
        }
      }
      const inOfStart = fromIndex + inOfIndex;
      const inOfEnd = inOfStart + 2;
      s.move(inOfStart, inOfEnd, item.node.loc.start.offset + 6);
      s.overwrite(inOfStart, inOfEnd, ",");
      const shouldWrapParams = !condition.startsWith(")");
      s.update(item.node.exp.loc.start.offset - 1, item.node.exp.loc.start.offset, shouldWrapParams ? "(" : "");
      if (shouldWrapParams) {
        s.prependLeft(fromIndex, ")=>{");
      } else {
        s.prependLeft(fromIndex + 1, "=>{");
      }
      const context = item.context;
      if (ctx.doNarrow && context.conditions.length > 0) {
        ctx.doNarrow({
          index: item.node.exp.loc.end.offset,
          inBlock: true,
          conditions: context.conditions,
          // type: "append",
          type: "prepend",
          direction: "right",
          condition: null
        }, s);
      }
      if (item.element.props.every((x) => !["if", "else", "else-if"].includes(x.name))) {
        block_1.BlockPlugin.addItem(item.element, item.parent, item.context);
      }
      s.prependLeft(item.element.loc.end.offset, "})");
    }
  });
  return loop;
}

var hasRequiredLoop;
function requireLoop() {
  if (hasRequiredLoop) return loop$1;
  hasRequiredLoop = 1;
  (function(exports) {
    Object.defineProperty(exports, "__esModule", { value: true });
    exports.LoopPlugin = void 0;
    var loop_1 = requireLoop$1();
    Object.defineProperty(exports, "LoopPlugin", { enumerable: true, get: function() {
      return loop_1.LoopPlugin;
    } });
  })(loop$1);
  return loop$1;
}

var element$1 = {};

var element = {};

var hasRequiredElement$1;
function requireElement$1() {
  if (hasRequiredElement$1) return element;
  hasRequiredElement$1 = 1;
  Object.defineProperty(element, "__esModule", { value: true });
  element.ElementPlugin = void 0;
  const compiler_core_1 = require$$0$1;
  const template_1 = requireTemplate$3();
  const block_1 = requireBlock();
  element.ElementPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VeterElement",
    transformElement(item, s, ctx) {
      var _a;
      if (item.node.tagType !== compiler_core_1.ElementTypes.COMPONENT) {
        return;
      }
      const node = item.node;
      const tagNameStart = node.loc.start.offset + 1;
      const tagNameEnd = tagNameStart + item.tag.length;
      const offset = node.loc.start.offset;
      const closingTagStartIndex = node.isSelfClosing ? -1 : node.loc.source.lastIndexOf(`</${item.tag}`, ((_a = node.children.at(-1)) == null ? void 0 : _a.loc.end.offset) ?? void 0) + 2;
      const shouldWrap = item.tag.includes("-");
      const isProp = node.props.find((x) => x.name === "is" || x.type === compiler_core_1.NodeTypes.DIRECTIVE && x.rawName === ":is");
      if (node.tag === "component" && isProp) {
        if (isProp.type === compiler_core_1.NodeTypes.ATTRIBUTE) {
          s.update(tagNameStart, tagNameEnd, "");
          s.move(isProp.value.loc.start.offset + 1, isProp.value.loc.end.offset - 1, tagNameStart);
          s.remove(isProp.loc.start.offset, isProp.value.loc.start.offset + 1);
          s.remove(isProp.value.loc.end.offset - 1, isProp.value.loc.end.offset);
          if (closingTagStartIndex !== -1) {
            s.update(offset + closingTagStartIndex, offset + closingTagStartIndex + item.tag.length, isProp.value.content);
          }
          return;
        }
        const name = ctx.prefix("component_render");
        s.move(isProp.exp.loc.start.offset, isProp.exp.loc.end.offset, node.loc.start.offset + 1);
        if (!item.condition) {
          block_1.BlockPlugin.addItem(node, item, item.context);
        }
        s.update(node.loc.start.offset, node.loc.start.offset + 1, `const ${name}=`);
        s.appendRight(node.loc.start.offset + 1, ";\n<");
        s.update(tagNameStart, tagNameEnd, name);
        s.remove(isProp.loc.start.offset, isProp.exp.loc.start.offset);
        s.remove(isProp.exp.loc.end.offset, isProp.loc.end.offset);
        if (closingTagStartIndex !== -1) {
          s.update(offset + closingTagStartIndex, offset + closingTagStartIndex + item.tag.length, name);
        }
        return;
      }
      const componentAccessor = ctx.retrieveAccessor("ctx");
      s.prependRight(node.loc.start.offset + 1, `${componentAccessor}${shouldWrap ? '["' : "."}`);
      if (!node.isSelfClosing && closingTagStartIndex > node.tag.length) {
        s.prependRight(offset + closingTagStartIndex, `${componentAccessor}.`);
      }
      if (~item.tag.indexOf(".")) {
        renameElementTag(item, ".", closingTagStartIndex, s);
      } else if (shouldWrap) {
        renameElementTag(item, "-", closingTagStartIndex, s);
        s.prependLeft(tagNameEnd, '"]');
      }
    }
  });
  const Replacer = "l__verter__l";
  function renameElementTag(item, delimiter, closingTagStartIndex, s, ctx) {
    const node = item.node;
    const tagNameStart = node.loc.start.offset + 1;
    const tagNameEnd = tagNameStart + item.tag.length;
    const offset = node.loc.start.offset;
    if (!item.condition) {
      block_1.BlockPlugin.addItem(node, item, item.context);
    }
    s.move(tagNameStart, tagNameEnd, node.loc.start.offset);
    const newName = delimiter === "." ? item.tag.replace(/\./g, Replacer) : item.tag.replace(/-/g, "").toUpperCase();
    s.appendLeft(tagNameStart, newName);
    s.prependRight(tagNameStart, `const ${newName}=`);
    s.appendLeft(tagNameEnd, ";");
    if (!node.isSelfClosing) {
      s.appendLeft(offset + closingTagStartIndex, newName);
      s.prependRight(offset + closingTagStartIndex, "{");
      s.prependLeft(offset + closingTagStartIndex + item.tag.length, "}");
      s.move(offset + closingTagStartIndex, offset + closingTagStartIndex + item.tag.length, offset + closingTagStartIndex - 2);
    }
  }
  return element;
}

var hasRequiredElement;
function requireElement() {
  if (hasRequiredElement) return element$1;
  hasRequiredElement = 1;
  (function(exports) {
    var __createBinding = element$1 && element$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = element$1 && element$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireElement$1(), exports);
  })(element$1);
  return element$1;
}

var templateTag$1 = {};

var templateTag = {};

var hasRequiredTemplateTag$1;
function requireTemplateTag$1() {
  if (hasRequiredTemplateTag$1) return templateTag;
  hasRequiredTemplateTag$1 = 1;
  Object.defineProperty(templateTag, "__esModule", { value: true });
  templateTag.TemplateTagPlugin = void 0;
  templateTag.TemplateTagPlugin = {
    name: "VerterTemplateTag",
    pre(s, ctx) {
      const pos = ctx.block.block.tag.pos;
      s.overwrite(pos.open.start, pos.open.start + 1, `export ${ctx.isAsync ? "async " : ""}function `);
      s.update(pos.open.end - 1, pos.open.end, `(){`);
      if (ctx.generic) {
        s.prependLeft(pos.open.end - 1, `<`);
        s.move(ctx.generic.position.start, ctx.generic.position.end, pos.open.end - 1);
        s.prependRight(pos.open.end - 1, `>`);
      }
      s.overwrite(pos.close.start, pos.close.end, "</>}");
      s.appendLeft(pos.open.end, `
<>`);
    }
  };
  return templateTag;
}

var hasRequiredTemplateTag;
function requireTemplateTag() {
  if (hasRequiredTemplateTag) return templateTag$1;
  hasRequiredTemplateTag = 1;
  (function(exports) {
    Object.defineProperty(exports, "__esModule", { value: true });
    exports.TemplateTagPlugin = void 0;
    var templateTag_1 = requireTemplateTag$1();
    Object.defineProperty(exports, "TemplateTagPlugin", { enumerable: true, get: function() {
      return templateTag_1.TemplateTagPlugin;
    } });
  })(templateTag$1);
  return templateTag$1;
}

var sfcCleaner$1 = {};

var sfcCleaner = {};

var hasRequiredSfcCleaner$1;
function requireSfcCleaner$1() {
  if (hasRequiredSfcCleaner$1) return sfcCleaner;
  hasRequiredSfcCleaner$1 = 1;
  Object.defineProperty(sfcCleaner, "__esModule", { value: true });
  sfcCleaner.SFCCleanerPlugin = void 0;
  const template_1 = requireTemplate$3();
  sfcCleaner.SFCCleanerPlugin = (0, template_1.declareTemplatePlugin)({
    name: "VerterSFCCleaner",
    post(s, ctx) {
      ctx.blocks.forEach((block) => {
        if (block === ctx.block || !block.block.block.content) {
          return;
        }
        const content = s.original.slice(block.block.tag.pos.open.start, block.block.tag.pos.close.end);
        const lines = content.split("\n");
        let lineOffset = block.block.tag.pos.open.start;
        for (const l of lines) {
          s.appendLeft(lineOffset, "// ");
          lineOffset += l.length + 1;
        }
      });
    }
  });
  return sfcCleaner;
}

var hasRequiredSfcCleaner;
function requireSfcCleaner() {
  if (hasRequiredSfcCleaner) return sfcCleaner$1;
  hasRequiredSfcCleaner = 1;
  (function(exports) {
    var __createBinding = sfcCleaner$1 && sfcCleaner$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = sfcCleaner$1 && sfcCleaner$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireSfcCleaner$1(), exports);
  })(sfcCleaner$1);
  return sfcCleaner$1;
}

var context$1 = {};

var context = {};

var hasRequiredContext$1;
function requireContext$1() {
  if (hasRequiredContext$1) return context;
  hasRequiredContext$1 = 1;
  Object.defineProperty(context, "__esModule", { value: true });
  context.ContextPlugin = void 0;
  const script_1 = requireScript();
  const utils_1 = requireUtils$1();
  const config_1 = requireConfig();
  context.ContextPlugin = {
    name: "VerterContext",
    pre(s, ctx) {
      const isSetup = ctx.blocks.find((x) => x.type === "script" && x.block.tag.attributes.setup) !== void 0;
      const options = (0, script_1.ResolveOptionsFilename)(ctx);
      const TemplateBindingName = ctx.prefix("TemplateBinding");
      const FullContextName = ctx.prefix("FullContext");
      const DefaultName = ctx.prefix("default_Component");
      const ComponentInstanceName = ctx.prefix("ComponentInstance");
      const macros = isSetup ? [
        [ctx.prefix("resolveProps"), "$props"],
        [ctx.prefix("resolveEmits"), "$emit"],
        // [ctx.prefix('resolveSlots'), "$slots"],
        [ctx.prefix("defineSlots"), "$slots"]
      ] : [];
      const importStr = (0, utils_1.generateImport)([
        {
          from: `./${options}`,
          items: [
            { name: TemplateBindingName },
            { name: FullContextName },
            {
              name: DefaultName
            },
            ...macros.map(([name]) => ({ name }))
          ]
        }
      ]);
      s.prepend(`${importStr}
`);
      const instanceStr = `const ${ComponentInstanceName} = new ${DefaultName}();`;
      const CTX = ctx.retrieveAccessor("ctx");
      const generic = ctx.generic ? `<${ctx.generic.names.join(",")}>` : "";
      const ctxItems = [
        isSetup ? ctx.prefix("resolveProps") : null,
        FullContextName,
        TemplateBindingName
      ].filter(Boolean).map((x) => ctx.isTS ? `${x}${generic}` : x).map((x) => ctx.isTS ? `...({} as ${x})` : `...${x}`);
      const ctxStr = `const ${CTX} = {${[
        `...${ComponentInstanceName}`,
        `...${ctx.isTS ? `({} as Required<typeof ${DefaultName}.components> & {})` : `${DefaultName}.components`}`,
        // `...${macros.map(([name, prop]) => `${name}(${prop})`).join(",")}`,
        ...macros.map(([name, prop]) => `${prop}: ${ctx.isTS ? `{} as ${name}${generic} & {}` : name}`),
        ...ctxItems
      ].join(",")}};`;
      const slotsCtx = `const ${ctx.prefix("$slot")} = ${CTX}['$slots'];`;
      const debuggers = config_1.DEBUG ? [
        `const ___DEBUG_Verter = ${CTX};`,
        "const ___DEBUG_Default = ___VERTER___default;",
        `const ___DEBUG_Props = ({} as ___VERTER___resolveProps${generic});`,
        `const ___DEBUG_Components = ({} as Required<typeof ___VERTER___default.components> & {});`,
        `const ___DEBUG_FullContext = ({} as ___VERTER___FullContext${generic});`,
        `const ___DEBUG_Binding = ({} as ___VERTER___TemplateBinding${generic});`,
        `const ___DEBUG_Slots = ___VERTER___ctx['$slots'];`
      ].join("\n") : "";
      s.prependLeft(ctx.block.block.block.loc.start.offset, [instanceStr, ctxStr, slotsCtx, debuggers].join("\n"));
      s.append(`
      declare function ${ctx.prefix("slotRender")}<T extends (...args: any[]) => any>(slot: T): (cb: T)=>any;
export declare function ${ctx.prefix("StrictRenderSlot")}<
  T extends (...args: any[]) => any,
  Single extends boolean = ReturnType<T> extends Array<any> ? false : true
>(slot: T, children: Single extends true ? [ReturnType<T>] : ReturnType<T>): any;`);
    }
  };
  return context;
}

var hasRequiredContext;
function requireContext() {
  if (hasRequiredContext) return context$1;
  hasRequiredContext = 1;
  (function(exports) {
    var __createBinding = context$1 && context$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = context$1 && context$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireContext$1(), exports);
  })(context$1);
  return context$1;
}

var hasRequiredPlugins;
function requirePlugins() {
  if (hasRequiredPlugins) return plugins;
  hasRequiredPlugins = 1;
  (function(exports) {
    var __createBinding = plugins && plugins.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = plugins && plugins.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireBinding(), exports);
    __exportStar(requireBlock(), exports);
    __exportStar(requireComment(), exports);
    __exportStar(requireConditional(), exports);
    __exportStar(requireInterpolation(), exports);
    __exportStar(requireProp(), exports);
    __exportStar(requireSlot(), exports);
    __exportStar(requireText(), exports);
    __exportStar(requireEvent(), exports);
    __exportStar(requireLoop(), exports);
    __exportStar(requireDirective(), exports);
    __exportStar(requireElement(), exports);
    __exportStar(requireTemplateTag(), exports);
    __exportStar(requireSfcCleaner(), exports);
    __exportStar(requireContext(), exports);
  })(plugins);
  return plugins;
}

var hasRequiredTemplate$2;
function requireTemplate$2() {
  if (hasRequiredTemplate$2) return template$3;
  hasRequiredTemplate$2 = 1;
  (function(exports) {
    var __createBinding = template$3 && template$3.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __setModuleDefault = template$3 && template$3.__setModuleDefault || (Object.create ? function(o, v) {
      Object.defineProperty(o, "default", { enumerable: true, value: v });
    } : function(o, v) {
      o["default"] = v;
    });
    var __exportStar = template$3 && template$3.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    var __importStar = template$3 && template$3.__importStar || /* @__PURE__ */ function() {
      var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function(o2) {
          var ar = [];
          for (var k in o2) if (Object.prototype.hasOwnProperty.call(o2, k)) ar[ar.length] = k;
          return ar;
        };
        return ownKeys(o);
      };
      return function(mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) {
          for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        }
        __setModuleDefault(result, mod);
        return result;
      };
    }();
    Object.defineProperty(exports, "__esModule", { value: true });
    exports.ScriptDefaultPlugins = exports.DefaultPlugins = void 0;
    __exportStar(requireTemplate$3(), exports);
    const Plugins = __importStar(requirePlugins());
    exports.DefaultPlugins = [
      Plugins.InterpolationPlugin,
      Plugins.PropPlugin,
      Plugins.ContextPlugin,
      Plugins.BindingPlugin,
      Plugins.CommentPlugin,
      // Plugins.TextPlugin,
      Plugins.SlotPlugin,
      Plugins.BlockPlugin,
      Plugins.ConditionalPlugin,
      Plugins.EventPlugin,
      Plugins.LoopPlugin,
      Plugins.DirectivePlugin,
      Plugins.ElementPlugin,
      Plugins.TemplateTagPlugin,
      Plugins.SFCCleanerPlugin
    ];
    exports.ScriptDefaultPlugins = [];
  })(template$3);
  return template$3;
}

var styles$1 = {};

var styles = {};

var hasRequiredStyles$1;
function requireStyles$1() {
  if (hasRequiredStyles$1) return styles;
  hasRequiredStyles$1 = 1;
  Object.defineProperty(styles, "__esModule", { value: true });
  styles.processStyles = processStyles;
  function processStyles(lang) {
  }
  return styles;
}

var hasRequiredStyles;
function requireStyles() {
  if (hasRequiredStyles) return styles$1;
  hasRequiredStyles = 1;
  (function(exports) {
    var __createBinding = styles$1 && styles$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = styles$1 && styles$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireStyles$1(), exports);
  })(styles$1);
  return styles$1;
}

var builders = {};

var template$1 = {};

var template = {};

var hasRequiredTemplate$1;
function requireTemplate$1() {
  if (hasRequiredTemplate$1) return template;
  hasRequiredTemplate$1 = 1;
  Object.defineProperty(template, "__esModule", { value: true });
  template.buildTemplate = buildTemplate;
  const __1 = requireTemplate$2();
  const template_1 = requireTemplate$3();
  function buildTemplate(items, context) {
    return (0, template_1.processTemplate)(items, [...__1.DefaultPlugins], context);
  }
  return template;
}

var hasRequiredTemplate;
function requireTemplate() {
  if (hasRequiredTemplate) return template$1;
  hasRequiredTemplate = 1;
  (function(exports) {
    var __createBinding = template$1 && template$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = template$1 && template$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireTemplate$1(), exports);
  })(template$1);
  return template$1;
}

var hasRequiredBuilders;
function requireBuilders() {
  if (hasRequiredBuilders) return builders;
  hasRequiredBuilders = 1;
  (function(exports) {
    var __createBinding = builders && builders.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = builders && builders.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireTemplate(), exports);
  })(builders);
  return builders;
}

var types = {};

var hasRequiredTypes;
function requireTypes() {
  if (hasRequiredTypes) return types;
  hasRequiredTypes = 1;
  Object.defineProperty(types, "__esModule", { value: true });
  types.ProcessItemType = void 0;
  var ProcessItemType;
  (function(ProcessItemType2) {
    ProcessItemType2["Import"] = "import";
    ProcessItemType2["Warning"] = "warning";
    ProcessItemType2["Error"] = "error";
    ProcessItemType2["Binding"] = "binding";
    ProcessItemType2["MacroBinding"] = "macro-binding";
    ProcessItemType2["Options"] = "options";
    ProcessItemType2["DefineModel"] = "define-model";
  })(ProcessItemType || (types.ProcessItemType = ProcessItemType = {}));
  return types;
}

var hasRequiredProcess;
function requireProcess() {
  if (hasRequiredProcess) return process$1;
  hasRequiredProcess = 1;
  (function(exports) {
    var __createBinding = process$1 && process$1.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = process$1 && process$1.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireScript(), exports);
    __exportStar(requireTemplate$2(), exports);
    __exportStar(requireStyles(), exports);
    __exportStar(requireBuilders(), exports);
    __exportStar(requireTypes(), exports);
  })(process$1);
  return process$1;
}

var hasRequiredV5;
function requireV5() {
  if (hasRequiredV5) return v5;
  hasRequiredV5 = 1;
  (function(exports) {
    var __createBinding = v5 && v5.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = v5 && v5.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireParser(), exports);
    __exportStar(requireProcess(), exports);
  })(v5);
  return v5;
}

var hasRequiredDist;
function requireDist() {
  if (hasRequiredDist) return dist;
  hasRequiredDist = 1;
  (function(exports) {
    var __createBinding = dist && dist.__createBinding || (Object.create ? function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      var desc = Object.getOwnPropertyDescriptor(m, k);
      if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
        desc = { enumerable: true, get: function() {
          return m[k];
        } };
      }
      Object.defineProperty(o, k2, desc);
    } : function(o, m, k, k2) {
      if (k2 === void 0) k2 = k;
      o[k2] = m[k];
    });
    var __exportStar = dist && dist.__exportStar || function(m, exports2) {
      for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports2, p)) __createBinding(exports2, m, p);
    };
    Object.defineProperty(exports, "__esModule", { value: true });
    __exportStar(requireV5(), exports);
  })(dist);
  return dist;
}

var distExports = requireDist();

const parseFile = (fileName, content, logger) => {
  logger.info(`[Verter] parsing ${fileName}`);
  const context = distExports.parser(content, path.basename(fileName), {
    filename: path.basename(fileName),
    sourceMap: true,
    ignoreEmpty: false,
    templateParseOptions: {
      parseMode: "sfc"
    }
  });
  console.log("parsed", context);
  logger.msg("parsed context", "Err");
  return "export default { foo: 1 }";
};

const init = ({ typescript: ts }) => {
  const create = (info) => {
    const languageServiceHost = {};
    const languageServiceHostProxy = new Proxy(info.languageServiceHost, {
      get(target, key) {
        return languageServiceHost[key] ? languageServiceHost[key] : target[key];
      }
    });
    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();
    info.project.getCompilerOptions();
    process.chdir(directory);
    const languageService = ts.createLanguageService(languageServiceHostProxy);
    if (info.languageServiceHost.resolveModuleNameLiterals) {
      const _resolveModuleNameLiterals = info.languageServiceHost.resolveModuleNameLiterals.bind(
        info.languageServiceHost
      );
      languageServiceHost.resolveModuleNameLiterals = (moduleNames, containingFile, ...rest) => {
        const resolvedModules = _resolveModuleNameLiterals(
          moduleNames,
          containingFile,
          ...rest
        );
        const moduleResolver = createModuleResolver(containingFile);
        return moduleNames.map(({ text: moduleName }, index) => {
          try {
            const resolvedModule = moduleResolver(
              moduleName,
              () => resolvedModules[index]
            );
            if (resolvedModule) return { resolvedModule };
          } catch (e) {
            logger.msg(e.toString(), "Err");
            return resolvedModules[index];
          }
          return resolvedModules[index];
        });
      };
    } else if (info.languageServiceHost.resolveModuleNames) {
      const _resolveModuleNames = info.languageServiceHost.resolveModuleNames.bind(
        info.languageServiceHost
      );
      languageServiceHost.resolveModuleNames = (moduleNames, containingFile, ...rest) => {
        const resolvedModules = _resolveModuleNames(
          moduleNames,
          containingFile,
          ...rest
        );
        const moduleResolver = createModuleResolver(containingFile);
        return moduleNames.map((moduleName, index) => {
          try {
            const resolvedModule = moduleResolver(
              moduleName,
              () => {
                var _a;
                return (
                  // @ts-expect-error
                  (_a = languageServiceHost.getResolvedModuleWithFailedLookupLocationsFromCache) == null ? void 0 : _a.call(
                    languageServiceHost,
                    moduleName,
                    containingFile
                  )
                );
              }
            );
            if (resolvedModule) return resolvedModule;
          } catch (e) {
            logger.msg(e.toString(), "Err");
            return resolvedModules[index];
          }
          return resolvedModules[index];
        });
      };
    }
    const createModuleResolver = (containingFile) => (moduleName, resolveModule) => {
      if (isRelativeVue(moduleName)) {
        logger.info(
          "[Verter] createModuleResolver relative vue - " + moduleName + " -- " + path.resolve(path.dirname(containingFile), moduleName)
        );
        return {
          extension: ts.Extension.Tsx,
          isExternalLibraryImport: false,
          resolvedFileName: path.resolve(
            path.dirname(containingFile),
            moduleName
          )
        };
      }
      if (!isVue(moduleName)) {
        return;
      }
      const resolvedModule = resolveModule();
      logger.info(
        "[Verter] createModuleResolver vue - " + resolvedModule + " -- " + resolvedModule
      );
      if (!resolvedModule) return;
      const baseUrl = info.project.getCompilerOptions().baseUrl;
      const match = "/index.ts";
      const failedLocations = resolvedModule.failedLookupLocations;
      failedLocations.reduce(
        (locations, location) => {
          if ((baseUrl ? location.includes(baseUrl) : true) && location.endsWith(match)) {
            return [...locations, location.replace(match, "")];
          }
          return locations;
        },
        []
      );
      const vueModulePath = failedLocations.find(
        (x) => (baseUrl ? x.includes(baseUrl) : true) && x.endsWith(match) && fs.existsSync(x)
      );
      if (!vueModulePath) return;
      return {
        extension: ts.Extension.Dts,
        isExternalLibraryImport: false,
        resolvedFileName: path.resolve(vueModulePath)
      };
    };
    const _readFile = info.serverHost.readFile.bind(info.serverHost);
    info.serverHost.readFile = (fileName) => {
      const file = _readFile(fileName);
      if (isVue(fileName) && file) {
        logger.info("[Verter] readFile - " + fileName + " -- " + file.length);
        return parseFile(fileName, file, logger);
      }
      return file;
    };
    return languageService;
  };
  const getExternalFiles = (project) => {
    const files = project.getFileNames(true, true).filter(isVue);
    project.projectService.logger.info(
      "[Verter] Got files\n" + files.join("\n")
    );
    return files;
  };
  return {
    create,
    getExternalFiles
  };
};
module.exports = init;
