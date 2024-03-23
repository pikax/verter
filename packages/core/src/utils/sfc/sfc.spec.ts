import { extractBlocksFromDescriptor, catchEmptyBlocks } from "./"
import { parse } from '@vue/compiler-sfc'


describe('Utils SFC', () => {

    describe('catchEmptyBlocks', () => {

        function doExtract(source: string) {
            const { descriptor } = parse(source)
            return catchEmptyBlocks(descriptor)
        }

        it('should work', () => {
            const source = `<script></script>\n`;
            const blocks = doExtract(source)

            expect(blocks).toHaveLength(1)
            expect(blocks).toMatchObject([
                {
                    type: 'empty',
                    attrs: {},
                    content: '',
                    loc: {
                        source,
                        start: {
                            offset: 8
                        },
                        end: {
                            offset: 8
                        }
                    }
                }
            ])
        })

        it('with content', () => {
            const source = `<script>    </script>`;
            const blocks = doExtract(source)

            expect(blocks).toHaveLength(1)
            expect(blocks).toMatchObject([
                {
                    type: 'empty',
                    attrs: {},
                    content: '    ',
                    loc: {
                        source,
                        start: {
                            offset: 8
                        },
                        end: {
                            offset: 12
                        }
                    }
                }
            ])
        })


        it('should not work if has content', () => {
            const source = `<script> Hey </script>`;
            const blocks = doExtract(source)

            expect(blocks).toHaveLength(0)
            expect(blocks).toEqual([])
        })

        it('should not catch commented', () => {
            const source = `<!-- <script></script> -->`;

            const blocks = doExtract(source)

            expect(blocks).toHaveLength(0)
            expect(blocks).toEqual([])
        })

        it('commented + valid', () => {
            const source = `<script></script>
            <!-- <script></script> -->`;

            const blocks = doExtract(source)

            expect(blocks).toHaveLength(1)
            expect(blocks).toMatchObject([
                {
                    type: 'empty',
                    attrs: {},
                    content: '',
                    loc: {
                        source,
                        start: {
                            offset: 8
                        },
                        end: {
                            offset: 8
                        }
                    }
                }
            ])
        })

        it('valid sandwitched between comments', () => {
            const source = `<!-- <script></script> -->
            <script></script>
            <!-- <script></script> -->`;

            const blocks = doExtract(source)

            expect(blocks).toHaveLength(1)
            expect(blocks).toMatchObject([
                {
                    type: 'empty',
                    attrs: {},
                    content: '',
                    loc: {
                        source,
                        start: {
                            offset: 47
                        },
                        end: {
                            offset: 47
                        }
                    }
                }
            ])
        })

    })


    describe('extractBlocksFromDescriptor', () => {
        function doExtract(source: string) {
            const { descriptor } = parse(source)
            return extractBlocksFromDescriptor(descriptor)
        }

        it('should parse', () => {
            const result = doExtract(`<script>
            const test = '';
            </script>`)

            expect(result).toMatchObject([
                {
                    block: {
                        type: 'script'
                    },
                    tag: {
                        type: 'script',
                        content: '',
                        pos: {
                            open: { start: 0, end: 8 },
                            close: {
                                start: 50,
                                end: 59
                            },
                            content: {
                                start: 7,
                                end: 7
                            }
                        },
                    }
                }
            ])
        })

        it('should parse with templte', () => {
            const source = `<script>
            const test = '';
            </script>
            <template> 
                <div>1</div>
            </template>`

            const result = doExtract(source)

            expect(result).toMatchObject([
                {
                    block: {
                        type: 'script'
                    },
                    tag: {
                        type: 'script',
                        content: '',
                        pos: {
                            open: { start: 0, end: 8 },
                            close: {
                                start: 50,
                                end: 59
                            },
                            content: {
                                start: 7,
                                end: 7
                            }
                        },
                    }
                }, {
                    block: {
                        type: 'template'
                    },
                    tag: {
                        type: 'template',
                        content: '',
                        pos: {
                            open: { start: 72, end: 82 },
                            close: {
                                start: 125,
                                end: 136
                            },
                            content: {
                                start: 81,
                                end: 81
                            }
                        }
                    }
                }
            ])
        })


        // vue SFC does not provide empty blocks
        it('should parse empty', () => {
            const source = `<script>
            </script>
            <template>
                <div>1</div>
            </template>
            `;
            const result = doExtract(source)

            expect(result).toMatchObject([
                {
                    block: {
                        type: 'empty'
                    },
                    tag: {
                        type: 'script',
                        content: '',
                        pos: {
                            open: { start: 0, end: 8 },
                            close: {
                                start: 21,
                                end: 30
                            },
                            content: {
                                start: 7,
                                end: 7
                            }
                        },
                    }
                }, {
                    block: {
                        type: 'template'
                    },
                    tag: {
                        type: 'template',
                        content: '',
                        pos: {
                            open: { start: 43, end: 53 },
                            close: {
                                start: 95,
                                end: 106
                            },
                            content: {
                                start: 52,
                                end: 52
                            }
                        },
                    }
                }
            ])
        })

        it('with commented block', () => {
            const source = `<script>
            const test = '';
            </script>
            <!--<script setup></script>-->
            <template> 
                <div>1</div>
            </template>`

            const result = doExtract(source)

            expect(result).toMatchObject([
                {
                    block: {
                        type: 'script',
                        loc: {
                            start: {
                                offset: 8
                            },
                            end: {
                                offset: 50
                            }
                        }
                    },
                    tag: {
                        type: 'script',
                        content: '',
                        pos: {
                            open: { start: 0, end: 8 },
                            close: {
                                start: 50,
                                end: 59
                            },
                            content: {
                                start: 7,
                                end: 7
                            }
                        },
                    }
                }, {
                    block: {
                        type: 'template',
                        loc: {
                            start: {
                                offset: 125
                            },
                            end: {
                                offset: 168
                            }
                        }
                    },
                    tag: {
                        type: 'template',
                        content: '',
                        pos: {
                            open: { start: 115, end: 125 },
                            close: {
                                start: 168,
                                end: 179
                            },
                            content: {
                                start: 124,
                                end: 124
                            }
                        }
                    }
                }
            ])
        })

        it('with many commented blocks', () => {
            const source = `
            <!--This is a comment-->
            <script>
            const test = '';
            </script>
            <!--<script setup></script>-->
            <!--Another comment-->
            <!--<script setup></script>-->
            <!--<script setup></script>-->
            <!--<script setup></script>-->
            <template> 
                <div>1</div>
            </template>
            <!--<script setup></script>-->
            `

            const result = doExtract(source)

            expect(result).toMatchObject([
                {
                    block: {
                        type: 'script',
                        loc: {
                            start: {
                                offset: 58
                            },
                            end: {
                                offset: 100
                            }
                        }
                    },
                    tag: {
                        type: 'script',
                        content: '',
                        pos: {
                            open: { start: 50, end: 58 },
                            close: {
                                start: 100,
                                end: 109
                            },
                            content: {
                                start: 57,
                                end: 57
                            }
                        },
                    }
                }, {
                    block: {
                        type: 'template'
                    },
                    tag: {
                        type: 'template',
                        content: '',
                        pos: {
                            open: { start: 329, end: 339 },
                            close: {
                                start: 382,
                                end: 393
                            },
                            content: {
                                start: 338,
                                end: 338
                            }
                        }
                    }
                }
            ])
        })
    })



})