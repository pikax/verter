// it should generate the script 
import { type ParseScriptContext } from "@verter/core";
import { parseLocations, LocationType } from "@verter/core";
import { walkIdentifiers } from '@vue/compiler-core'
import { getBlockFilename } from "../utils";
import { MagicString, type SFCScriptCompileOptions, type SFCDescriptor, compileScript } from "vue/compiler-sfc";
import type * as _babel_types from "@babel/types";

export const RenderContextExportName = 'RenderContext'


// function compileScriptSafe(
//   descriptor: SFCDescriptor,
//   options: SFCScriptCompileOptions
// ) {
//   try {
//     return compileScript(descriptor, options);
//   } catch (e) {
//     console.error("failed compiling script", e);
//     return null;
//   }
// }


export function processScript(context: ParseScriptContext) {
  // todo move locations to argument
  const locations = parseLocations(context);
  const filename = getBlockFilename("script", context)

  const generic = context.generic ? `<${context.generic}>` : ''
  let s: MagicString


  // TODO add options function
  if (context.script == null) {
    s = new MagicString([`export function resolve${RenderContextExportName}${generic}() {} {`,
      `return {}`,
      `} `
    ].join('\n')
    );
  } else {
    const startOffset = context.script.loc.start.offset;
    const endOffset = context.script.loc.end.offset;

    s = context.s.snip(startOffset, endOffset)

    // TODO sort this out
    const prependContent = [
      '',
      `export${context.isAsync ? ' async' : ''} function resolve${RenderContextExportName}${generic}() {`,
      ''
    ]

    const returnContent = [
      '',
      'return {} as '];

    if (!locations.import) {
      locations.import = []
    }
    locations.import.push({
      type: LocationType.Import,
      generated: true,
      from: 'vue',
      items: [
        {
          name: 'ShallowUnwrapRef',
          alias: '__VERTER_ShallowUnwrapRef'
        }
      ]

    })

    const appendContent = [
      '',
      '}',
      ''
    ]


    const ctxNodes: _babel_types.Node[] = []



    /**
     * This will move the import statements to the top of the file
     * and also keep track of the end of the import statements 
     */
    let importContentEndIndex = 0
    if (locations.import?.length > 0) {
      for (const it of locations.import.filter(x => !x.generated)) {
        s.move(it.node.start + startOffset, it.node.end + startOffset, 0)
        if (importContentEndIndex < it.node.end + startOffset) {
          importContentEndIndex = it.node.end + startOffset
        }
      }
    }

    /**
     * Add declaration to the return context
     */
    if (locations.declaration?.length > 0) {
      locations.declaration.filter(x => !x.generated).forEach(it => {
        ctxNodes.push(it.node)
      })
    }


    const ctxVariables = new Set<string>();
    for (let i = 0; i < ctxNodes.length; i++) {
      const node = ctxNodes[i]
      if (node.type === 'ImportDeclaration' && node.importKind === 'type') {
        continue
      }
      walkIdentifiers(node, (name, parent) => {
        // if (parent?.type === 'CallExpression') {
        //   return;
        // }
        // if (parent?.type === 'ImportSpecifier') {
        //   if (parent.importKind === 'type') {
        //     return
        //   }
        // }

        ctxVariables.add(name.name)
      }, true)
    }

    const returnCtx = [...ctxVariables].map(x => `${x}: typeof ${x}`).join(',\n')

    s.appendLeft(importContentEndIndex, prependContent.join('\n'))
    s.append(returnContent.join('\n') + `{${returnCtx}}`)
    s.append(appendContent.join('\n'))


    s.append(`type ${RenderContextExportName}Type${generic} = ReturnType<typeof resolve${RenderContextExportName}${generic}>`
      + `${context.isAsync ? ' extends Promise<infer R> ? R : never' : ''};`)

    s.append(`\nexport declare function ${RenderContextExportName}${generic}(): __VERTER_ShallowUnwrapRef<${RenderContextExportName}Type${generic}>;`)


    // add generated imports
    if (locations.import?.length > 0) {
      const from: Record<string, Set<string>> = {}

      locations.import.filter(x => x.generated).forEach(it => {
        if (!from[it.from]) {
          from[it.from] = new Set()
        }
        const importItem = from[it.from]
        for (let i = 0; i < it.items.length; i++) {
          const item = it.items[i];
          importItem.add(`${it.asType || item.type ? 'type ' : ''}${item.name}${item.alias ? ` as ${item.alias}` : ''}`)
        }
      })

      for (const [fromStr, imports] of Object.entries(from)) {
        s.prepend(`import { ${[...imports].join(', ')} } from '${fromStr}';\n`)
      }
    }
  }


  return {
    filename,
    loc: context.script?.loc ?? {},

    s,
    content: s.toString(),
  }

}