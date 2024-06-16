import { ParseScriptContext } from "@verter/core";


export type BlockType = 'render' | 'script' | 'style' | 'custom';

// const BlockExtensionMap = {
//     render: 'render.tsx',
//     script: 'script',
//     style: 'css',
//     custom: 'custom',
// } satisfies Record<BlockType, string>;

export function getBlockFilename(block: BlockType, context: ParseScriptContext) {
    switch (block) {
        case 'render':
            return context.filename + '.render.tsx';
        case 'script':
            return context.filename + '.script.ts';

    }
    return context.filename + '.TODO.tsx'
}