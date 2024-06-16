import { createBuilder } from '@verter/core'

const builder = createBuilder({})

export function build(source: string, filename: string) {
    return builder.preProcess(filename, source)
}