import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption, WalkResult } from "../types.js";
import Declaration from "../declaration/declaration.js";

const possibleExports = [
  "export default /*#__PURE__*/_",
  "export default /*#__PURE__*/",
  "export default ",
];

export default {
  name: "Options",

  process: (context) => {
    const source = context.script?.content;
    // empty component
    if (!source) {
      return [
        {
          type: LocationType.Import,
          generated: true,
          node: context.script,
          // TODO change the import location
          from: "vue",
          items: [
            {
              name: "defineComponent",
              type: true,
            },
          ],
        },
        {
          type: LocationType.Declaration,
          generated: true,
          node: undefined,
          declaration: {
            name: "__options",
            content: `defineComponent({})`,
          },
        },
        {
          type: LocationType.Declaration,
          generated: true,
          node: undefined,
          declaration: {
            type: "type",
            name: "Type__options",
            content: `typeof __options`,
          },
        },
      ];
    }

    // setup is very easy to process since export default is always the last thing
    if (context.isSetup) {
      let content: string = source;
      for (let i = 0; i < possibleExports.length; i++) {
        const element = possibleExports[i];
        const indexOf = content.indexOf(element);
        if (~indexOf) {
          content = content.slice(indexOf + element.length);
          break;
        }
      }

      if (content.startsWith("defineComponent")) {
        content = content.slice("defineComponent".length);
      }

      // append generic to setup
      if (context.generic) {
        content = content.replace("setup(", `setup<${context.generic}>(`);
      }

      // vue imports
      const imports = source.startsWith("import")
        ? source.slice(0, source.indexOf("\n"))
        : "";

      // literal-const are declared outside of setup
      // this adds them back in
      const literalConst = new Set(
        Object.entries(context.script.bindings ?? {})
          .filter(([_, x]) => x === "literal-const")
          .map(([x]) => x)
      );

      const declarations = context.script.scriptSetupAst
        .filter((x) =>
          x?.declarations?.find((d) => literalConst.has(d.id.name))
        )
        .map((x) => Declaration.walk(x, context))
        .filter(Boolean) as WalkResult[] | undefined;

      if (declarations.length) {
        content = content.replace(
          ") {",
          `) { ${declarations?.map((x) =>  x.declaration?.content).join("")}`
        );
      }

      return [
        {
          type: LocationType.Import,
          generated: true,
          node: context.script,
          // TODO change the import location
          from: "vue",
          items: [
            {
              name: "defineComponent",
              type: true,
            },
          ],
        },
        {
          type: LocationType.Declaration,
          node: context.script,

          generated: true,

          declaration: {
            name: "__options",
            content: `defineComponent(${content})`,
          },
        },
        {
          type: LocationType.Declaration,
          node: context.script,
          generated: true,

          declaration: {
            type: "type",
            name: "Type__options",
            content: `typeof __options`,
          },
        },
        // {
        //   type: LocationType.Export,
        //   node: context.script,
        //   item: {
        //     default: true,
        //     name: "__options",
        //     alias: "Type__options",
        //     type: true,
        //   },
        // },
      ]
        .concat(
          Object.values(context.script!.imports ?? {}).map((x) => ({
            type: LocationType.Import,
            generated: false,
            node: context.script,
            from: x.source,
            items: [
              {
                name: x.imported,
                alias: x.local,
                type: x.isType,
              },
            ],
          }))
        )
        .concat(
          imports
            ? [
                {
                  type: LocationType.Import,
                  generated: false,
                  node: context.script,
                  from: imports.slice(),
                  items: [],
                },
              ]
            : []
        );
    }

    let content = source;
    for (const it of possibleExports) {
      const index = source.indexOf(it);
      if (index === -1) continue;

      const c = source.slice(index + it.length);
      content = c.startsWith("{") ? `defineComponent(${c})` : c;
      break;
    }

    // return [
    //   {
    //     type: LocationType.Import,
    //     node: context.script,
    //     // TODO change the import location
    //     from: "vue",
    //     items: [
    //       {
    //         name: "defineComponent",
    //         type: true,
    //       },
    //     ],
    //   },
    //   {}
    // ];

    return [
      {
        type: LocationType.Import,
        node: context.script,
        generated: true,
        // TODO change the import location
        from: "vue",
        items: [
          {
            name: "defineComponent",
            type: true,
          },
        ],
      },
      ...(context.generic
        ? [
            {
              type: LocationType.Declaration,
              generated: true,
              node: undefined,
              declaration: {
                name: "__optionsGenerator",
                content: `(<${context.generic},>() => { return ${content} })`,
              },
            },
            {
              type: LocationType.Declaration,
              generated: true,
              node: undefined,
              declaration: {
                name: "__options",
                content: `__optionsGenerator()`,
              },
            },
          ]
        : [
            {
              type: LocationType.Declaration,
              generated: true,
              node: undefined,
              declaration: {
                name: "__options",
                content,
              },
            },
          ]),
      {
        type: LocationType.Declaration,
        generated: true,
        node: undefined,
        declaration: {
          type: "type",
          name: "Type__options",
          content: `typeof __options`,
        },
      },
    ];
  },

  //   walk: (node, context) => {
  //     if (!context.isSetup) return;
  //     const expression = checkForSetupMethodCall("defineSlots", node);
  //     if (!expression) return;
  //     const source = context.script!.loc.source;

  //     const slotType = expression.typeParameters?.params[0];
  //     if (!slotType) return;

  //     return [
  //       {
  //         type: LocationType.Import,
  //         node: expression,
  //         // TODO change the import location
  //         from: "vue",
  //         items: [
  //           {
  //             name: "SlotsType",
  //             type: true,
  //           },
  //         ],
  //       },
  //       {
  //         type: LocationType.Slots,
  //         node: expression,
  //         content: `SlotsType<${retrieveNodeString(slotType, source)}>`,
  //       },
  //     ];
  //   },
} satisfies PluginOption;
