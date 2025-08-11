import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { TemplateBindingPlugin } from "../template-binding";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { SupportedMacros, MacrosPlugin } from "./macros";
import { ProcessItem, ProcessItemType } from "../../../types";
import { ImportsPlugin } from "../imports";

describe("process script plugins macros", () => {
  function _parse(
    content: string,
    wrapper: string | boolean = false,
    lang = "js",

    pre = "",
    post = ""
  ) {
    const prepend = `${pre}<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}">`;
    const source = `${prepend}${content}</script>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [MacrosPlugin, TemplateBindingPlugin, ScriptBlockPlugin, BindingPlugin],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: wrapper === false,
        block: scriptBlock,
        blockNameResolver: (name) => name,
      }
    );

    return r;
  }

  describe("setup", () => {
    function parse(content: string, lang = "ts", pre: string = "") {
      return _parse(`${pre ? pre + "\n" : ""}${content}`, false, lang, pre);
    }
    describe.each([true, false])("declarator: %s;", (hasDeclarator) => {
      describe("defineProps & withDefaults", () => {
        function content(str: string) {
          return `${hasDeclarator ? "const props=" : ""}${str}`;
        }
        function withBinding(
          before: Partial<ProcessItem & { node: any }>[],
          after?: Partial<ProcessItem & { node: any }>[]
        ) {
          const items = [...before];
          if (hasDeclarator) {
            items.push({
              type: ProcessItemType.Binding,
              name: "props",
              originalName: "props",
              item: {
                type: "Declaration",
                name: "props",
              },
              node: {
                type: "Identifier",
                name: "props",
              },
            } as ProcessItem);
          }

          if (after) {
            items.push(...after);
          }
          return items;
        }

        describe("defineProps()", () => {
          const _content = content("defineProps()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "props" : context.prefix("PropsValue"),
                  macro: "defineProps",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineProps",
                    },
                  },
                  declarationType: "empty",
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            expect(result).toContain(
              hasDeclarator
                ? _content
                : `const ${context.prefix("PropsValue")}=${_content}`
            );
          });
        });

        describe("array", () => {
          describe("defineProps(['foo', 'bar'])", () => {
            const script = `defineProps(['foo', 'bar'])`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("PropsRaw"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `const ${context.prefix("PropsRaw")}=${context.prefix(
                  script
                )};${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }defineProps(${context.prefix("extractValue")}(${context.prefix(
                  "PropsRaw"
                )}));`
              );
            });
          });

          describe('withDefaults(defineProps(["foo", "bar"]), { foo: 1 })', () => {
            const script = `withDefaults(defineProps(["foo", "bar"]), { foo: 1 })`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: undefined,
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("PropsRaw"),
                  },
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "withDefaults",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "withDefaults",
                      },
                    },
                    declarationType: "object",
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const defineProps = 'defineProps(["foo", "bar"])';
              const definePropsPrefixed = context.prefix(defineProps);
              expect(result).toContain(
                `const ${context.prefix("PropsRaw")}=${definePropsPrefixed};${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }withDefaults(defineProps(${context.prefix(
                  "extractValue"
                )}(${context.prefix("PropsRaw")})), { foo: 1 });`
              );
            });
          });

          describe("defineProps<'foo'|'bar'>()", () => {
            const script = `defineProps<'foo'|'bar'>()`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("PropsType"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const typeDef = "'foo'|'bar'";
              expect(result).toContain(
                `type ${context.prefix("PropsType")}=${typeDef};${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }defineProps<${context.prefix("PropsType")}&{}>();`
              );
            });
          });

          describe("withDefaults(defineProps<'foo'|'bar'>(['foo', 'bar']), { foo: '1' })", () => {
            const script = `withDefaults(defineProps<'foo'|'bar'>(['foo', 'bar']), { foo: '1' })`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: undefined,
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("PropsType"),
                  },
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "withDefaults",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "withDefaults",
                      },
                    },
                    declarationType: "object",
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `type ${context.prefix("PropsType")}='foo'|'bar';${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }withDefaults(defineProps<${context.prefix(
                  "PropsType"
                )}&{}>(['foo', 'bar']), { foo: '1' });`
              );
            });
          });

          if (hasDeclarator) {
            describe("Destructuring", () => {
              describe('const { foo, bar } = defineProps(["foo", "bar"])', () => {
                const script = `const { foo, bar } = defineProps(["foo", "bar"])`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "bar",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, bar } = defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")}));`
                  );
                });
              });
              describe('const { foo, bar } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })', () => {
                const script = `const { foo, bar } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "bar",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, bar } = withDefaults(defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")})), { foo: 1 });`
                  );
                });
              });
              describe('const { foo, ...rest } = defineProps(["foo", "bar"])', () => {
                const script = `const { foo, ...rest } = defineProps(["foo", "bar"])`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "rest",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, ...rest } = defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")}));`
                  );
                });
              });
              describe('const { foo, ...rest } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })', () => {
                const script = `const { foo, ...rest } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "rest",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, ...rest } = withDefaults(defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")})), { foo: 1 });`
                  );
                });
              });
            });
          }

          describe("javascript", () => {
            describe("defineProps(['foo', 'bar'])", () => {
              const script = `defineProps(['foo', 'bar'])`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("PropsRaw")}=${context.prefix(
                    script
                  )};${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }defineProps(${context.prefix("extractValue")}(${context.prefix(
                    "PropsRaw"
                  )}));`
                );
              });
            });
            describe("withDefaults(defineProps(['foo', 'bar']), { foo: 1 })", () => {
              const script = `withDefaults(defineProps(['foo', 'bar']), { foo: 1 })`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                      declarationType: "object",
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("PropsRaw")}=${context.prefix(
                    "defineProps(['foo', 'bar'])"
                  )};${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }withDefaults(defineProps(${context.prefix(
                    "extractValue"
                  )}(${context.prefix("PropsRaw")})), { foo: 1 });`
                );
              });
            });
          });

          describe("generic", () => {
            describe("defineProps<T>(['foo', 'bar'])", () => {
              const script = `defineProps<T>(['foo', 'bar'])`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("PropsType"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("PropsType")}=T;${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }defineProps<${context.prefix("PropsType")}&{}>(['foo', 'bar']);`
                );
              });
            });
            describe("withDefaults(defineProps<T>(['foo', 'bar']), { foo: '1' })", () => {
              const script = `withDefaults(defineProps<T>(['foo', 'bar']), { foo: '1' })`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("PropsType"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                      declarationType: "object",
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("PropsType")}=T;${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }withDefaults(defineProps<${context.prefix(
                    "PropsType"
                  )}&{}>(['foo', 'bar']), { foo: '1' });`
                );
              });
            });

            if (hasDeclarator) {
              describe("Destructuring", () => {
                describe('const { foo, bar } = defineProps<T>(["foo", "bar"])', () => {
                  const script = `const { foo, bar } = defineProps<T>(["foo", "bar"])`;

                  function process() {
                    return parse(script);
                  }
                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "bar",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, bar } = defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]);`
                    );
                  });
                });
                describe('const { foo, bar } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })', () => {
                  const script = `const { foo, bar } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })`;

                  function process() {
                    return parse(script);
                  }

                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "withDefaults",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "withDefaults",
                          },
                        },
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "bar",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, bar } = withDefaults(defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]), { foo: 1 });`
                    );
                  });
                });

                describe('const { foo, ...rest } = defineProps<T>(["foo", "bar"])', () => {
                  const script = `const { foo, ...rest } = defineProps<T>(["foo", "bar"])`;

                  function process() {
                    return parse(script);
                  }
                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "rest",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, ...rest } = defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]);`
                    );
                  });
                });
                describe('const { foo, ...rest } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })', () => {
                  const script = `const { foo, ...rest } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })`;

                  function process() {
                    return parse(script);
                  }

                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "withDefaults",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "withDefaults",
                          },
                        },
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "rest",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, ...rest } = withDefaults(defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]), { foo: 1 });`
                    );
                  });
                });
              });
            }
          });
        });
        describe("options", () => {
          describe("defineProps({foo: String, bar: { type: String, required: true }})", () => {
            const script = `defineProps({foo: String, bar: { type: String, required: true }})`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("PropsRaw"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `const ${context.prefix("PropsRaw")}=${context.prefix(
                  script
                )};${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }defineProps(${context.prefix("extractValue")}(${context.prefix(
                  "PropsRaw"
                )}));`
              );
            });
          });

          describe("withDefaults(defineProps({foo: String, bar: { type: String, required: true }}), { foo: 1 })", () => {
            const script = `withDefaults(defineProps({foo: String, bar: { type: String, required: true }}), { foo: 1 })`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: undefined,
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("PropsRaw"),
                  },
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "withDefaults",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "withDefaults",
                      },
                    },
                    declarationType: "object",
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const defineProps =
                "defineProps({foo: String, bar: { type: String, required: true }})";
              const definePropsPrefixed = context.prefix(defineProps);
              expect(result).toContain(
                `const ${context.prefix("PropsRaw")}=${definePropsPrefixed};${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }withDefaults(defineProps(${context.prefix(
                  "extractValue"
                )}(${context.prefix("PropsRaw")})), { foo: 1 });`
              );
            });
          });

          describe("defineProps<{foo?: string; bar: string}>()", () => {
            const script = `defineProps<{foo?: string; bar: string}>()`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("PropsType"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const typeDef = "{foo?: string; bar: string}";
              expect(result).toContain(
                `type ${context.prefix("PropsType")}=${typeDef};${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }defineProps<${context.prefix("PropsType")}&{}>();`
              );
            });
          });

          describe("withDefaults(defineProps<{foo?: string; bar: string}>({foo: String, bar: { type: String, required: true }}), { foo: '1' })", () => {
            const script = `withDefaults(defineProps<{foo?: string; bar: string}>({foo: String, bar: { type: String, required: true }}), { foo: '1' })`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: undefined,
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("PropsType"),
                  },
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "withDefaults",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "withDefaults",
                      },
                    },
                    declarationType: "object",
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `type ${context.prefix(
                  "PropsType"
                )}={foo?: string; bar: string};${
                  hasDeclarator
                    ? `const props=`
                    : `const ${context.prefix("PropsValue")}=`
                }withDefaults(defineProps<${context.prefix(
                  "PropsType"
                )}&{}>({foo: String, bar: { type: String, required: true }}), { foo: '1' });`
              );
            });
          });

          if (hasDeclarator) {
            describe("Destructuring", () => {
              describe('const { foo, bar } = defineProps(["foo", "bar"])', () => {
                const script = `const { foo, bar } = defineProps(["foo", "bar"])`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "bar",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, bar } = defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")}));`
                  );
                });
              });
              describe('const { foo, bar } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })', () => {
                const script = `const { foo, bar } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "bar",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, bar } = withDefaults(defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")})), { foo: 1 });`
                  );
                });
              });
              describe('const { foo, ...rest } = defineProps(["foo", "bar"])', () => {
                const script = `const { foo, ...rest } = defineProps(["foo", "bar"])`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "rest",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, ...rest } = defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")}));`
                  );
                });
              });
              describe('const { foo, ...rest } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })', () => {
                const script = `const { foo, ...rest } = withDefaults(defineProps(["foo", "bar"]), { foo: 1 })`;
                function process() {
                  return parse(script);
                }
                test("context", () => {
                  const { context } = process();
                  expect(context.items).toMatchObject([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "foo",
                    },
                    {
                      type: ProcessItemType.Binding,
                      name: "rest",
                    },
                  ]);
                });
                test("output", () => {
                  const { result, context } = process();
                  expect(result).toContain(
                    `const ${context.prefix("PropsRaw")}=${context.prefix(
                      "defineProps"
                    )}(["foo", "bar"]);const { foo, ...rest } = withDefaults(defineProps(${context.prefix(
                      "extractValue"
                    )}(${context.prefix("PropsRaw")})), { foo: 1 });`
                  );
                });
              });
            });
          }

          describe("javascript", () => {
            describe("defineProps({foo: String, bar: { type: String, required: true }})", () => {
              const script = `defineProps({foo: String, bar: { type: String, required: true }})`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("PropsRaw")}=${context.prefix(
                    script
                  )};${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }defineProps(${context.prefix("extractValue")}(${context.prefix(
                    "PropsRaw"
                  )}));`
                );
              });
            });
            describe("withDefaults(defineProps({foo: String, bar: { type: String, required: true }}), { foo: 1 })", () => {
              const script = `withDefaults(defineProps({foo: String, bar: { type: String, required: true }}), { foo: 1 })`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("PropsRaw"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                      declarationType: "object",
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("PropsRaw")}=${context.prefix(
                    "defineProps({foo: String, bar: { type: String, required: true }})"
                  )};${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }withDefaults(defineProps(${context.prefix(
                    "extractValue"
                  )}(${context.prefix("PropsRaw")})), { foo: 1 });`
                );
              });
            });
          });

          describe("generic", () => {
            describe("defineProps<T>({foo: String, bar: { type: String, required: true }})", () => {
              const script = `defineProps<T>({foo: String, bar: { type: String, required: true }})`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("PropsType"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("PropsType")}=T;${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }defineProps<${context.prefix(
                    "PropsType"
                  )}&{}>({foo: String, bar: { type: String, required: true }});`
                );
              });
            });
            describe("withDefaults(defineProps<T>({foo: String, bar: { type: String, required: true }}), { foo: '1' })", () => {
              const script = `withDefaults(defineProps<T>({foo: String, bar: { type: String, required: true }}), { foo: '1' })`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: undefined,
                      macro: "defineProps",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineProps",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("PropsType"),
                    },
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "props"
                        : context.prefix("PropsValue"),
                      macro: "withDefaults",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "withDefaults",
                        },
                      },
                      declarationType: "object",
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("PropsType")}=T;${
                    hasDeclarator
                      ? `const props=`
                      : `const ${context.prefix("PropsValue")}=`
                  }withDefaults(defineProps<${context.prefix(
                    "PropsType"
                  )}&{}>({foo: String, bar: { type: String, required: true }}), { foo: '1' });`
                );
              });
            });

            if (hasDeclarator) {
              describe("Destructuring", () => {
                describe('const { foo, bar } = defineProps<T>(["foo", "bar"])', () => {
                  const script = `const { foo, bar } = defineProps<T>(["foo", "bar"])`;

                  function process() {
                    return parse(script);
                  }
                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "bar",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, bar } = defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]);`
                    );
                  });
                });
                describe('const { foo, bar } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })', () => {
                  const script = `const { foo, bar } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })`;

                  function process() {
                    return parse(script);
                  }

                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "withDefaults",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "withDefaults",
                          },
                        },
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "bar",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, bar } = withDefaults(defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]), { foo: 1 });`
                    );
                  });
                });

                describe('const { foo, ...rest } = defineProps<T>(["foo", "bar"])', () => {
                  const script = `const { foo, ...rest } = defineProps<T>(["foo", "bar"])`;

                  function process() {
                    return parse(script);
                  }
                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "rest",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, ...rest } = defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]);`
                    );
                  });
                });
                describe('const { foo, ...rest } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })', () => {
                  const script = `const { foo, ...rest } = withDefaults(defineProps<T>(["foo", "bar"]), { foo: 1 })`;

                  function process() {
                    return parse(script);
                  }

                  test("context", () => {
                    const { context } = process();
                    expect(context.items).toMatchObject([
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "defineProps",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "defineProps",
                          },
                        },
                        declarationType: "type",
                        declarationName: context.prefix("PropsType"),
                      },
                      {
                        type: ProcessItemType.MacroBinding,
                        name: undefined,
                        macro: "withDefaults",
                        node: {
                          type: "CallExpression",
                          callee: {
                            type: "Identifier",
                            name: "withDefaults",
                          },
                        },
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "foo",
                      },
                      {
                        type: ProcessItemType.Binding,
                        name: "rest",
                      },
                    ]);
                  });

                  test("output", () => {
                    const { result, context } = process();
                    expect(result).toContain(
                      `type ${context.prefix(
                        "PropsType"
                      )}=T;const { foo, ...rest } = withDefaults(defineProps<${context.prefix(
                        "PropsType"
                      )}&{}>(["foo", "bar"]), { foo: 1 });`
                    );
                  });
                });
              });
            }
          });
        });
        describe("types", () => {
          const _content = content("defineProps<Foo>()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "props" : context.prefix("PropsValue"),
                  macro: "defineProps",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineProps",
                    },
                  },
                  declarationType: "type",
                  declarationName: context.prefix("PropsType"),
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            const typeDef = "Foo";
            expect(result).toContain(
              `type ${context.prefix("PropsType")}=${typeDef};${
                hasDeclarator
                  ? `const props=`
                  : `const ${context.prefix("PropsValue")}=`
              }defineProps<${context.prefix("PropsType")}&{}>();`
            );
          });
        });

        describe("import", () => {
          describe("vue", () => {
            const _content = content("defineProps()");
            function process() {
              return parse(`import { defineProps } from 'vue';\n${_content}`);
            }

            test("context", () => {
              const { context } = process();

              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.Binding,
                    name: "defineProps",
                  },
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "props"
                      : context.prefix("PropsValue"),
                    macro: "defineProps",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineProps",
                      },
                    },
                    declarationType: "empty",
                  },
                ])
              );
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });

          describe("not-vue", () => {
            const _content = content("defineProps()");
            function process() {
              return parse(
                `import { defineProps } from 'not-vue';\n${_content}`
              );
            }

            test("context", () => {
              const { context } = process();

              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.Binding,
                    name: "defineProps",
                  },
                ])
              );
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });
        });

        describe("override local", () => {
          describe("const defineProps = () => {}", () => {
            const _content = content("defineProps()");
            function process() {
              return parse(`const defineProps = () => {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();

              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.Binding,
                    name: "defineProps",
                  },
                ])
              );
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });
          describe("function defineProps() {}", () => {
            const _content = content("defineProps()");
            function process() {
              return parse(`function defineProps() {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();
              const expectedResult = withBinding([
                {
                  type: ProcessItemType.Binding,
                  name: "defineProps",
                },
              ]);

              expect(context.items).toMatchObject(expectedResult);
              expect(context.items.length).toBe(expectedResult.length);
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });

          describe("class defineProps {}", () => {
            const _content = content("defineProps()");
            function process() {
              return parse(`class defineProps {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();
              const expectedResult = withBinding([
                {
                  type: ProcessItemType.Binding,
                  name: "defineProps",
                },
              ]);

              expect(context.items).toMatchObject(expectedResult);
              expect(context.items.length).toBe(expectedResult.length);
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });

          describe("type defineProps = {};", () => {
            const _content = content("defineProps()");
            function process() {
              return parse(`type defineProps = {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();
              const expectedResult = withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "props" : context.prefix("PropsValue"),
                  macro: "defineProps",
                  declarationType: "empty",
                },
              ]);

              expect(context.items).toMatchObject(expectedResult);
              expect(context.items.length).toBe(expectedResult.length);
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });
        });
      });

      describe("defineEmits", () => {
        function content(str: string) {
          return `${hasDeclarator ? "const emit=" : ""}${str}`;
        }
        function withBinding(
          before: Partial<ProcessItem & { node: any }>[],
          after?: Partial<ProcessItem & { node: any }>[]
        ) {
          const items = [...before];
          if (hasDeclarator) {
            items.push({
              type: ProcessItemType.Binding,
              name: "emit",
              originalName: "emit",
              item: {
                type: "Declaration",
                name: "emit",
              },
              node: {
                type: "Identifier",
                name: "emit",
              },
            } as ProcessItem);
          }

          if (after) {
            items.push(...after);
          }
          return items;
        }

        describe("defineEmits()", () => {
          const _content = content("defineEmits()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "emit" : context.prefix("EmitsValue"),
                  macro: "defineEmits",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineEmits",
                    },
                  },
                  declarationType: "empty",
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            expect(result).toContain(
              hasDeclarator
                ? _content
                : `const ${context.prefix("EmitsValue")}=${_content}`
            );
          });
        });

        describe("array", () => {
          describe("defineEmits(['foo', 'bar'])", () => {
            const script = `defineEmits(['foo', 'bar'])`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator ? "emit" : context.prefix("EmitsValue"),
                    macro: "defineEmits",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineEmits",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("EmitsRaw"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `const ${context.prefix("EmitsRaw")}=${context.prefix(
                  script
                )};${
                  hasDeclarator
                    ? `const emit=`
                    : `const ${context.prefix("EmitsValue")}=`
                }defineEmits(${context.prefix("extractValue")}(${context.prefix(
                  "EmitsRaw"
                )}));`
              );
            });
          });

          describe("defineEmits<'foo'|'bar'>()", () => {
            const script = `defineEmits<'foo'|'bar'>()`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator ? "emit" : context.prefix("EmitsValue"),
                    macro: "defineEmits",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineEmits",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("EmitsType"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const typeDef = "'foo'|'bar'";
              expect(result).toContain(
                `type ${context.prefix("EmitsType")}=${typeDef};${
                  hasDeclarator
                    ? `const emit=`
                    : `const ${context.prefix("EmitsValue")}=`
                }defineEmits<${context.prefix("EmitsType")}&{}>();`
              );
            });
          });

          describe("javascript", () => {
            describe("defineEmits(['foo', 'bar'])", () => {
              const script = `defineEmits(['foo', 'bar'])`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "emit"
                        : context.prefix("EmitsValue"),
                      macro: "defineEmits",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineEmits",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("EmitsRaw"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("EmitsRaw")}=${context.prefix(
                    script
                  )};${
                    hasDeclarator
                      ? `const emit=`
                      : `const ${context.prefix("EmitsValue")}=`
                  }defineEmits(${context.prefix("extractValue")}(${context.prefix(
                    "EmitsRaw"
                  )}));`
                );
              });
            });
          });

          describe("generic", () => {
            describe("defineEmits<T>(['foo', 'bar'])", () => {
              const script = `defineEmits<T>(['foo', 'bar'])`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "emit"
                        : context.prefix("EmitsValue"),
                      macro: "defineEmits",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineEmits",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("EmitsType"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("EmitsType")}=T;${
                    hasDeclarator
                      ? `const emit=`
                      : `const ${context.prefix("EmitsValue")}=`
                  }defineEmits<${context.prefix("EmitsType")}&{}>(['foo', 'bar']);`
                );
              });
            });
          });
        });
        describe("options", () => {
          describe("defineEmits({foo:()=>true, bar:(arg)=>true})", () => {
            const script = `defineEmits({foo:()=>true, bar:(arg)=>true})`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator ? "emit" : context.prefix("EmitsValue"),
                    macro: "defineEmits",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineEmits",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("EmitsRaw"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `const ${context.prefix("EmitsRaw")}=${context.prefix(
                  script
                )};${
                  hasDeclarator
                    ? `const emit=`
                    : `const ${context.prefix("EmitsValue")}=`
                }defineEmits(${context.prefix("extractValue")}(${context.prefix(
                  "EmitsRaw"
                )}));`
              );
            });
          });

          describe("defineEmits<{ foo: ()=> void, bar: (args: {baz: string})=>void}>()", () => {
            const script = `defineEmits<{ foo: ()=> void, bar: (args: {baz: string})=>void}>()`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator ? "emit" : context.prefix("EmitsValue"),
                    macro: "defineEmits",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineEmits",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("EmitsType"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const typeDef =
                "{ foo: ()=> void, bar: (args: {baz: string})=>void}";
              expect(result).toContain(
                `type ${context.prefix("EmitsType")}=${typeDef};${
                  hasDeclarator
                    ? `const emit=`
                    : `const ${context.prefix("EmitsValue")}=`
                }defineEmits<${context.prefix("EmitsType")}&{}>();`
              );
            });
          });

          describe("javascript", () => {
            describe("defineEmits({ foo: ()=>true, bar: (arg)=>true})", () => {
              const script = `defineEmits({ foo: ()=>true, bar: (arg)=>true})`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "emit"
                        : context.prefix("EmitsValue"),
                      macro: "defineEmits",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineEmits",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("EmitsRaw"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("EmitsRaw")}=${context.prefix(
                    script
                  )};${
                    hasDeclarator
                      ? `const emit=`
                      : `const ${context.prefix("EmitsValue")}=`
                  }defineEmits(${context.prefix("extractValue")}(${context.prefix(
                    "EmitsRaw"
                  )}));`
                );
              });
            });
          });

          describe("generic", () => {
            describe("defineEmits<T>({ foo: ()=>true, bar: (arg)=>true})", () => {
              const script = `defineEmits<T>({ foo: ()=>true, bar: (arg)=>true})`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "emit"
                        : context.prefix("EmitsValue"),
                      macro: "defineEmits",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineEmits",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("EmitsType"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("EmitsType")}=T;${
                    hasDeclarator
                      ? `const emit=`
                      : `const ${context.prefix("EmitsValue")}=`
                  }defineEmits<${context.prefix(
                    "EmitsType"
                  )}&{}>({ foo: ()=>true, bar: (arg)=>true});`
                );
              });
            });
          });
        });
        describe("types", () => {
          const _content = content("defineEmits<Foo>()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "emit" : context.prefix("EmitsValue"),
                  macro: "defineEmits",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineEmits",
                    },
                  },
                  declarationType: "type",
                  declarationName: context.prefix("EmitsType"),
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            const typeDef = "Foo";
            expect(result).toContain(
              `type ${context.prefix("EmitsType")}=${typeDef};${
                hasDeclarator
                  ? `const emit=`
                  : `const ${context.prefix("EmitsValue")}=`
              }defineEmits<${context.prefix("EmitsType")}&{}>();`
            );
          });
        });
      });

      describe("defineSlots", () => {
        function content(str: string) {
          return `${hasDeclarator ? "const slots=" : ""}${str}`;
        }
        function withBinding(
          before: Partial<ProcessItem & { node: any }>[],
          after?: Partial<ProcessItem & { node: any }>[]
        ) {
          const items = [...before];
          if (hasDeclarator) {
            items.push({
              type: ProcessItemType.Binding,
              name: "slots",
              originalName: "slots",
              item: {
                type: "Declaration",
                name: "slots",
              },
              node: {
                type: "Identifier",
                name: "slots",
              },
            } as ProcessItem);
          }

          if (after) {
            items.push(...after);
          }
          return items;
        }

        describe("defineSlots()", () => {
          const _content = content("defineSlots()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "slots" : context.prefix("SlotsValue"),
                  macro: "defineSlots",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineSlots",
                    },
                  },
                  declarationType: "empty",
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            expect(result).toContain(
              hasDeclarator
                ? _content
                : `const ${context.prefix("SlotsValue")}=${_content}`
            );
          });
        });

        describe("array", () => {
          describe("defineSlots(['foo', 'bar'])", () => {
            const script = `defineSlots(['foo', 'bar'])`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "slots"
                      : context.prefix("SlotsValue"),
                    macro: "defineSlots",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineSlots",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("SlotsRaw"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `const ${context.prefix("SlotsRaw")}=${context.prefix(
                  script
                )};${
                  hasDeclarator
                    ? `const slots=`
                    : `const ${context.prefix("SlotsValue")}=`
                }defineSlots(${context.prefix("extractValue")}(${context.prefix(
                  "SlotsRaw"
                )}));`
              );
            });
          });

          describe("defineSlots<'foo'|'bar'>()", () => {
            const script = `defineSlots<'foo'|'bar'>()`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "slots"
                      : context.prefix("SlotsValue"),
                    macro: "defineSlots",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineSlots",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("SlotsType"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const typeDef = "'foo'|'bar'";
              expect(result).toContain(
                `type ${context.prefix("SlotsType")}=${typeDef};${
                  hasDeclarator
                    ? `const slots=`
                    : `const ${context.prefix("SlotsValue")}=`
                }defineSlots<${context.prefix("SlotsType")}&{}>();`
              );
            });
          });

          describe("javascript", () => {
            describe("defineSlots(['foo', 'bar'])", () => {
              const script = `defineSlots(['foo', 'bar'])`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "slots"
                        : context.prefix("SlotsValue"),
                      macro: "defineSlots",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineSlots",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("SlotsRaw"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("SlotsRaw")}=${context.prefix(
                    script
                  )};${
                    hasDeclarator
                      ? `const slots=`
                      : `const ${context.prefix("SlotsValue")}=`
                  }defineSlots(${context.prefix("extractValue")}(${context.prefix(
                    "SlotsRaw"
                  )}));`
                );
              });
            });
          });

          describe("generic", () => {
            describe("defineSlots<T>(['foo', 'bar'])", () => {
              const script = `defineSlots<T>(['foo', 'bar'])`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "slots"
                        : context.prefix("SlotsValue"),
                      macro: "defineSlots",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineSlots",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("SlotsType"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("SlotsType")}=T;${
                    hasDeclarator
                      ? `const slots=`
                      : `const ${context.prefix("SlotsValue")}=`
                  }defineSlots<${context.prefix("SlotsType")}&{}>(['foo', 'bar']);`
                );
              });
            });
          });
        });
        describe("options", () => {
          describe("defineSlots({foo:()=>true, bar:(arg)=>true})", () => {
            const script = `defineSlots({foo:()=>true, bar:(arg)=>true})`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "slots"
                      : context.prefix("SlotsValue"),
                    macro: "defineSlots",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineSlots",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("SlotsRaw"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `const ${context.prefix("SlotsRaw")}=${context.prefix(
                  script
                )};${
                  hasDeclarator
                    ? `const slots=`
                    : `const ${context.prefix("SlotsValue")}=`
                }defineSlots(${context.prefix("extractValue")}(${context.prefix(
                  "SlotsRaw"
                )}));`
              );
            });
          });

          describe("defineSlots<{ foo: ()=> void, bar: (args: {baz: string})=>void}>()", () => {
            const script = `defineSlots<{ foo: ()=> void, bar: (args: {baz: string})=>void}>()`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator
                      ? "slots"
                      : context.prefix("SlotsValue"),
                    macro: "defineSlots",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineSlots",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("SlotsType"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const typeDef =
                "{ foo: ()=> void, bar: (args: {baz: string})=>void}";
              expect(result).toContain(
                `type ${context.prefix("SlotsType")}=${typeDef};${
                  hasDeclarator
                    ? `const slots=`
                    : `const ${context.prefix("SlotsValue")}=`
                }defineSlots<${context.prefix("SlotsType")}&{}>();`
              );
            });
          });

          describe("javascript", () => {
            describe("defineSlots({ foo: ()=>true, bar: (arg)=>true})", () => {
              const script = `defineSlots({ foo: ()=>true, bar: (arg)=>true})`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "slots"
                        : context.prefix("SlotsValue"),
                      macro: "defineSlots",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineSlots",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("SlotsRaw"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("SlotsRaw")}=${context.prefix(
                    script
                  )};${
                    hasDeclarator
                      ? `const slots=`
                      : `const ${context.prefix("SlotsValue")}=`
                  }defineSlots(${context.prefix("extractValue")}(${context.prefix(
                    "SlotsRaw"
                  )}));`
                );
              });
            });
          });

          describe("generic", () => {
            describe("defineSlots<T>({ foo: ()=>true, bar: (arg)=>true})", () => {
              const script = `defineSlots<T>({ foo: ()=>true, bar: (arg)=>true})`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator
                        ? "slots"
                        : context.prefix("SlotsValue"),
                      macro: "defineSlots",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineSlots",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("SlotsType"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("SlotsType")}=T;${
                    hasDeclarator
                      ? `const slots=`
                      : `const ${context.prefix("SlotsValue")}=`
                  }defineSlots<${context.prefix(
                    "SlotsType"
                  )}&{}>({ foo: ()=>true, bar: (arg)=>true});`
                );
              });
            });
          });
        });
        describe("types", () => {
          const _content = content("defineSlots<Foo>()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "slots" : context.prefix("SlotsValue"),
                  macro: "defineSlots",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineSlots",
                    },
                  },
                  declarationType: "type",
                  declarationName: context.prefix("SlotsType"),
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            const typeDef = "Foo";
            expect(result).toContain(
              `type ${context.prefix("SlotsType")}=${typeDef};${
                hasDeclarator
                  ? `const slots=`
                  : `const ${context.prefix("SlotsValue")}=`
              }defineSlots<${context.prefix("SlotsType")}&{}>();`
            );
          });
        });
      });

      describe("defineExpose", () => {
        function content(str: string) {
          return `${hasDeclarator ? "const expose=" : ""}${str}`;
        }
        function withBinding(
          before: Partial<ProcessItem & { node: any }>[],
          after?: Partial<ProcessItem & { node: any }>[]
        ) {
          const items = [...before];
          if (hasDeclarator) {
            items.push({
              type: ProcessItemType.Binding,
              name: "expose",
              originalName: "expose",
              item: {
                type: "Declaration",
                name: "expose",
              },
              node: {
                type: "Identifier",
                name: "expose",
              },
            } as ProcessItem);
          }

          if (after) {
            items.push(...after);
          }
          return items;
        }

        describe("defineExpose()", () => {
          const _content = content("defineExpose()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "expose" : undefined,
                  macro: "defineExpose",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineExpose",
                    },
                  },
                  declarationType: "empty",
                },
              ])
            );
          });

          test("output", () => {
            const { result } = process();

            expect(result).toContain(
              hasDeclarator
                ? _content
                : // does not have `const ...`
                  `{${_content}`
            );
          });
        });

        describe("options", () => {
          describe("defineExpose({foo: '1', bar() {}})", () => {
            const script = `defineExpose({foo: '1', bar() {}})`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator ? "expose" : undefined,
                    macro: "defineExpose",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineExpose",
                      },
                    },
                    declarationType: "object",
                    declarationName: context.prefix("ExposeRaw"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              expect(result).toContain(
                `const ${context.prefix("ExposeRaw")}=${context.prefix(
                  script
                )};${
                  hasDeclarator ? `const expose=` : ""
                }defineExpose(${context.prefix("extractValue")}(${context.prefix(
                  "ExposeRaw"
                )}));`
              );
            });
          });

          describe("defineExpose<{foo?: string; bar: string}>()", () => {
            const script = `defineExpose<{foo?: string; bar: string}>()`;
            const _content = content(script);
            function process() {
              return parse(_content);
            }

            test("context", () => {
              const { context } = process();
              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator ? "expose" : undefined,
                    macro: "defineExpose",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineExpose",
                      },
                    },
                    declarationType: "type",
                    declarationName: context.prefix("ExposeType"),
                  },
                ])
              );
            });

            test("output", () => {
              const { result, context } = process();
              const typeDef = "{foo?: string; bar: string}";
              expect(result).toContain(
                `type ${context.prefix("ExposeType")}=${typeDef};${
                  hasDeclarator ? `const expose=` : ""
                }defineExpose<${context.prefix("ExposeType")}&{}>();`
              );
            });
          });

          describe("javascript", () => {
            describe("defineExpose({foo: '1', bar() {}})", () => {
              const script = `defineExpose({foo: '1', bar() {}})`;

              const _content = content(script);
              function process() {
                return parse(_content, "js");
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator ? "expose" : undefined,
                      macro: "defineExpose",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineExpose",
                        },
                      },
                      declarationType: "object",
                      declarationName: context.prefix("ExposeRaw"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `const ${context.prefix("ExposeRaw")}=${context.prefix(
                    script
                  )};${
                    hasDeclarator ? `const expose=` : ""
                  }defineExpose(${context.prefix(
                    "extractValue"
                  )}(${context.prefix("ExposeRaw")}));`
                );
              });
            });
          });

          describe("generic", () => {
            describe("defineExpose<T>({foo: '1', bar() {}})", () => {
              const script = `defineExpose<T>({foo: '1', bar() {}})`;

              const _content = content(script);
              function process() {
                return parse(_content);
              }
              test("context", () => {
                const { context } = process();
                expect(context.items).toMatchObject(
                  withBinding([
                    {
                      type: ProcessItemType.MacroBinding,
                      name: hasDeclarator ? "expose" : undefined,
                      macro: "defineExpose",
                      node: {
                        type: "CallExpression",
                        callee: {
                          type: "Identifier",
                          name: "defineExpose",
                        },
                      },
                      declarationType: "type",
                      declarationName: context.prefix("ExposeType"),
                    },
                  ])
                );
              });

              test("output", () => {
                const { result, context } = process();
                expect(result).toContain(
                  `type ${context.prefix("ExposeType")}=T;${
                    hasDeclarator ? `const expose=` : ""
                  }defineExpose<${context.prefix(
                    "ExposeType"
                  )}&{}>({foo: '1', bar() {}});`
                );
              });
            });
          });
        });
        describe("types", () => {
          const _content = content("defineExpose<Foo>()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();

            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "expose" : undefined,
                  macro: "defineExpose",
                  node: {
                    type: "CallExpression",
                    callee: {
                      type: "Identifier",
                      name: "defineExpose",
                    },
                  },
                  declarationType: "type",
                  declarationName: context.prefix("ExposeType"),
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            const typeDef = "Foo";
            expect(result).toContain(
              `type ${context.prefix("ExposeType")}=${typeDef};${
                hasDeclarator ? `const expose=` : ""
              }defineExpose<${context.prefix("ExposeType")}&{}>();`
            );
          });
        });

        describe("import", () => {
          describe("vue", () => {
            const _content = content("defineExpose()");
            function process() {
              return parse(`import { defineExpose } from 'vue';\n${_content}`);
            }

            test("context", () => {
              const { context } = process();

              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.Binding,
                    name: "defineExpose",
                  },
                  {
                    type: ProcessItemType.MacroBinding,
                    name: hasDeclarator ? "expose" : undefined,
                    macro: "defineExpose",
                    node: {
                      type: "CallExpression",
                      callee: {
                        type: "Identifier",
                        name: "defineExpose",
                      },
                    },
                    declarationType: "empty",
                  },
                ])
              );
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });

          describe("not-vue", () => {
            const _content = content("defineExpose()");
            function process() {
              return parse(
                `import { defineExpose } from 'not-vue';\n${_content}`
              );
            }

            test("context", () => {
              const { context } = process();

              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.Binding,
                    name: "defineExpose",
                  },
                ])
              );
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });
        });

        describe("override local", () => {
          describe("const defineExpose = () => {}", () => {
            const _content = content("defineExpose()");
            function process() {
              return parse(`const defineExpose = () => {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();

              expect(context.items).toMatchObject(
                withBinding([
                  {
                    type: ProcessItemType.Binding,
                    name: "defineExpose",
                  },
                ])
              );
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });
          describe("function defineExpose() {}", () => {
            const _content = content("defineExpose()");
            function process() {
              return parse(`function defineExpose() {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();
              const expectedResult = withBinding([
                {
                  type: ProcessItemType.Binding,
                  name: "defineExpose",
                },
              ]);

              expect(context.items).toMatchObject(expectedResult);
              expect(context.items.length).toBe(expectedResult.length);
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });

          describe("class defineExpose {}", () => {
            const _content = content("defineExpose()");
            function process() {
              return parse(`class defineExpose {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();
              const expectedResult = withBinding([
                {
                  type: ProcessItemType.Binding,
                  name: "defineExpose",
                },
              ]);

              expect(context.items).toMatchObject(expectedResult);
              expect(context.items.length).toBe(expectedResult.length);
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });

          describe("type defineExpose = {};", () => {
            const _content = content("defineExpose()");
            function process() {
              return parse(`type defineExpose = {};\n${_content}`);
            }
            test("context", () => {
              const { context } = process();
              const expectedResult = withBinding([
                {
                  type: ProcessItemType.MacroBinding,
                  name: hasDeclarator ? "expose" : undefined,
                  macro: "defineExpose",
                  declarationType: "empty",
                },
              ]);

              expect(context.items).toMatchObject(expectedResult);
              expect(context.items.length).toBe(expectedResult.length);
            });

            test("output", () => {
              const { result } = process();
              expect(result).toContain(_content);
            });
          });
        });
      });

      describe.skip("defineModel", () => {
        function content(str: string, varName: string = "value") {
          return `${hasDeclarator ? `const ${varName}=` : ""}${str}`;
        }
        function withBinding(
          before: Partial<ProcessItem & { node: any }>[],
          varName: string = "value",
          after?: Partial<ProcessItem & { node: any }>[]
        ) {
          const items = [...before];
          if (hasDeclarator) {
            items.push({
              type: ProcessItemType.Binding,
              name: varName,
              originalName: varName,
              item: {
                type: "Declaration",
                name: varName,
              },
              node: {
                type: "Identifier",
                name: varName,
              },
            } as ProcessItem);
          }

          if (after) {
            items.push(...after);
          }
          return items;
        }

        describe("defineModel()", () => {
          const _content = content("defineModel()");
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();
            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.DefineModel,
                  name: "modelValue",
                  varName: hasDeclarator
                    ? "value"
                    : context.prefix("models_modelValue"),
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            expect(result).toContain(
              (hasDeclarator
                ? _content
                : `const ${context.prefix("models_modelValue")}=`) + _content
            );
          });
        });

        describe('defineModel<"foo"|"bar">()', () => {
          const _content = content('defineModel<"foo"|"bar">()');
          function process() {
            return parse(_content);
          }

          test("context", () => {
            const { context } = process();
            expect(context.items).toMatchObject(
              withBinding([
                {
                  type: ProcessItemType.DefineModel,
                  name: "modelValue",
                  varName: hasDeclarator
                    ? "value"
                    : context.prefix("models_modelValue"),
                },
              ])
            );
          });

          test("output", () => {
            const { result, context } = process();
            expect(result).toContain(
              (hasDeclarator
                ? ""
                : `const ${context.prefix("models_modelValue")}=`) + _content
            );
          });
        });
      });
    });

    describe.skip("defineOptions", () => {
      function content(str: string) {
        return `${str}`;
      }
      function withBinding(
        before: Partial<ProcessItem & { node: any }>[],
        after?: Partial<ProcessItem & { node: any }>[]
      ) {
        const items = [...before];

        if (after) {
          items.push(...after);
        }
        return items;
      }

      describe("defineOptions()", () => {
        const _content = content("defineOptions()");
        function process() {
          return parse(_content);
        }

        test("context", () => {
          const { context } = process();
          expect(context.items).toMatchObject(
            withBinding([
              {
                type: ProcessItemType.Warning,
              },
            ])
          );
        });

        test("output", () => {
          const { result } = process();
          expect(result).toContain(_content);
        });
      });

      describe('defineOptions({ name: "foo" })', () => {
        const _content = content('defineOptions({ name: "foo" })');
        function process() {
          return parse(_content);
        }
        test("context", () => {
          const { context } = process();
          const expectedResult = withBinding([
            {
              type: ProcessItemType.Options,
            },
          ]);

          expect(context.items).toMatchObject(expectedResult);
          expect(context.items.length).toBe(expectedResult.length);
        });

        test("output", () => {
          const { result } = process();
          expect(result).toContain("{" + _content);
        });
      });
    });
  });
});
