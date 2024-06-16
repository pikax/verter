import { processRender } from './render';
import { build } from '../processor.test.helpers'

describe('processRender', () => {
    function process(source: string, filename = 'test.vue') {
        const b = build(source, filename)
        return processRender(b.context, b.locations)
    }


    it('should return new file name', () => {
        const result = process(`<template><div></div></template>`)
        expect(result).toMatchObject({
            filename: 'test.vue.render.tsx',
        })
    })


    it('should return the render function', () => {
        const result = process(`<template><div></div></template>`)

        expect(result.source).toContain('return <div></div>')
    })


})

