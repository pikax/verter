import { ParseScriptContext } from "@verter/core";


export type BlockType = 'bundle' | 'render' | 'script' | 'options' | 'style' | 'custom';

// const BlockExtensionMap = {
//     render: 'render.tsx',
//     script: 'script',
//     style: 'css',
//     custom: 'custom',
// } satisfies Record<BlockType, string>;

export function getBlockFilename(block: BlockType, context: ParseScriptContext) {
    switch (block) {
        case 'bundle':
            return context.filename + '.bundle.tsx';
        case 'render':
            return context.filename + '.render.tsx';
        case 'script':
            return context.filename + '.script.ts';
        case 'options':
            return context.filename + '.options.' + (context.script?.lang ?? 'js');

    }
    return context.filename + '.TODO.tsx'
}