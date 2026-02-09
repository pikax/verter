import * as monaco from 'monaco-editor-core'

/**
 * Language configurations based on Vue REPL
 * https://github.com/vuejs/repl/blob/main/src/monaco/language-configs.ts
 */

const css: monaco.languages.LanguageConfiguration = {
  comments: {
    blockComment: ['/*', '*/'],
  },
  brackets: [
    ['{', '}'],
    ['[', ']'],
    ['(', ')'],
  ],
  autoClosingPairs: [
    { open: '{', close: '}', notIn: ['string', 'comment'] },
    { open: '[', close: ']', notIn: ['string', 'comment'] },
    { open: '(', close: ')', notIn: ['string', 'comment'] },
    { open: '"', close: '"', notIn: ['string', 'comment'] },
    { open: "'", close: "'", notIn: ['string', 'comment'] },
  ],
  surroundingPairs: [
    { open: '{', close: '}' },
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: '"', close: '"' },
    { open: "'", close: "'" },
  ],
  folding: {
    markers: {
      start: /^\s*\/\*\s*#region\b\s*(.*?)\s*\*\//,
      end: /^\s*\/\*\s*#endregion\b.*\*\//,
    },
  },
  indentationRules: {
    increaseIndentPattern: /^.*\{[^}"']*$/,
    decreaseIndentPattern: /^\s*\}/,
  },
  wordPattern:
    /(#?-?\d*\.\d\w*%?)|((::|[@#.!:])?[\w-?]+%?)|::|[@#.!:]/g,
}

const js: monaco.languages.LanguageConfiguration = {
  comments: {
    lineComment: '//',
    blockComment: ['/*', '*/'],
  },
  brackets: [
    ['{', '}'],
    ['[', ']'],
    ['(', ')'],
  ],
  autoClosingPairs: [
    { open: '{', close: '}' },
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: "'", close: "'", notIn: ['string', 'comment'] },
    { open: '"', close: '"', notIn: ['string', 'comment'] },
    { open: '`', close: '`', notIn: ['string', 'comment'] },
    { open: '/**', close: ' */', notIn: ['string'] },
  ],
  surroundingPairs: [
    { open: '{', close: '}' },
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: "'", close: "'" },
    { open: '"', close: '"' },
    { open: '`', close: '`' },
  ],
  folding: {
    markers: {
      start: /^\s*\/\/\s*#?region\b/,
      end: /^\s*\/\/\s*#?endregion\b/,
    },
  },
  wordPattern:
    /(-?\d*\.\d\w*)|([^\`\~\!\@\#\%\^\&\*\(\)\-\=\+\[\{\]\}\\\|\;\:\'\"\,\.\<\>\/\?\s]+)/g,
  indentationRules: {
    increaseIndentPattern:
      /^((?!\/\/).)*(\{[^}"'`]*|\([^)"'`]*|\[[^\]"'`]*)$/,
    decreaseIndentPattern: /^((?!.*?\/\*).*\*\/)?\s*[\}\]\)].*$/,
  },
  onEnterRules: [
    {
      beforeText: /^\s*\/\*\*(?!\/)([^\*]|\*(?!\/))*$/,
      afterText: /^\s*\*\/$/,
      action: {
        indentAction: monaco.languages.IndentAction.IndentOutdent,
        appendText: ' * ',
      },
    },
    {
      beforeText: /^\s*\/\*\*(?!\/)([^\*]|\*(?!\/))*$/,
      action: {
        indentAction: monaco.languages.IndentAction.None,
        appendText: ' * ',
      },
    },
    {
      beforeText: /^(\t|(\ \ ))*\ \*(\ ([^\*]|\*(?!\/))*)?$/,
      action: {
        indentAction: monaco.languages.IndentAction.None,
        appendText: '* ',
      },
    },
    {
      beforeText: /^(\t|(\ \ ))*\ \*\/\s*$/,
      action: {
        indentAction: monaco.languages.IndentAction.None,
        removeText: 1,
      },
    },
  ],
}

const ts: monaco.languages.LanguageConfiguration = {
  ...js,
  // @ts-ignore - colorizedBracketPairs is a valid property
  colorizedBracketPairs: [
    ['(', ')'],
    ['[', ']'],
    ['{', '}'],
    ['<', '>'],
  ],
}

// CSS Monarch tokenizer
const cssTokenizer: monaco.languages.IMonarchLanguage = {
  defaultToken: '',
  tokenPostfix: '.css',

  tokenizer: {
    root: [
      { include: '@selector' },
    ],

    selector: [
      [/\/\*/, 'comment', '@comment'],
      [/\/\/.*$/, 'comment'],
      [/@[\w-]+/, 'keyword'],
      [/[.#][\w-]+/, 'tag'],
      [/[\w-]+/, 'tag'],
      [/\{/, 'delimiter.bracket', '@declarationBody'],
      [/[>+~,]/, 'delimiter'],
      [/\[/, 'delimiter.bracket', '@attributeSelector'],
    ],

    attributeSelector: [
      [/[\w-]+/, 'attribute.name'],
      [/[=~|^$*]?=/, 'operator'],
      [/"[^"]*"|'[^']*'/, 'string'],
      [/\]/, 'delimiter.bracket', '@pop'],
    ],

    declarationBody: [
      [/\/\*/, 'comment', '@comment'],
      [/[\w-]+(?=\s*:)/, 'attribute.name'],
      [/:/, 'delimiter'],
      [/;/, 'delimiter'],
      [/#[0-9a-fA-F]{3,8}/, 'number.hex'],
      [/\d+(\.\d+)?(px|em|rem|%|vh|vw|vmin|vmax|ch|ex|s|ms|deg|rad|turn|fr)?/, 'number'],
      [/"[^"]*"|'[^']*'/, 'string'],
      [/[\w-]+(?=\()/, 'attribute.value'],
      [/\(/, 'delimiter.parenthesis', '@functionArgs'],
      [/--[\w-]+/, 'variable'],
      [/[\w-]+/, 'attribute.value'],
      [/\{/, 'delimiter.bracket', '@declarationBody'],
      [/\}/, 'delimiter.bracket', '@pop'],
    ],

    functionArgs: [
      [/[^()]+/, 'attribute.value'],
      [/\(/, 'delimiter.parenthesis', '@functionArgs'],
      [/\)/, 'delimiter.parenthesis', '@pop'],
    ],

    comment: [
      [/\*\//, 'comment', '@pop'],
      [/./, 'comment'],
    ],
  },
}

// TypeScript/JavaScript Monarch tokenizer
const tsTokenizer: monaco.languages.IMonarchLanguage = {
  defaultToken: '',
  tokenPostfix: '.ts',

  keywords: [
    'abstract', 'any', 'as', 'asserts', 'async', 'await', 'bigint', 'boolean',
    'break', 'case', 'catch', 'class', 'const', 'constructor', 'continue',
    'debugger', 'declare', 'default', 'delete', 'do', 'else', 'enum', 'export',
    'extends', 'false', 'finally', 'for', 'from', 'function', 'get', 'if',
    'implements', 'import', 'in', 'infer', 'instanceof', 'interface', 'is',
    'keyof', 'let', 'module', 'namespace', 'never', 'new', 'null', 'number',
    'object', 'of', 'override', 'package', 'private', 'protected', 'public',
    'readonly', 'require', 'return', 'satisfies', 'set', 'static', 'string',
    'super', 'switch', 'symbol', 'this', 'throw', 'true', 'try', 'type',
    'typeof', 'undefined', 'unique', 'unknown', 'var', 'void', 'while', 'with',
    'yield',
  ],

  operators: [
    '<=', '>=', '==', '!=', '===', '!==', '=>', '+', '-', '**',
    '*', '/', '%', '++', '--', '<<', '>>', '>>>', '&',
    '|', '^', '!', '~', '&&', '||', '??', '?', ':', '=', '+=', '-=',
    '*=', '**=', '/=', '%=', '<<=', '>>=', '>>>=', '&=', '|=',
    '^=', '@', '?.', '??=', '&&=', '||=', '...',
  ],

  symbols: /[=><!~?:&|+\-*\/\^%]+/,
  escapes: /\\(?:[abfnrtv\\"']|x[0-9A-Fa-f]{1,4}|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})/,
  digits: /\d+(_+\d+)*/,

  tokenizer: {
    root: [
      { include: '@common' },
    ],

    common: [
      // Comments
      [/\/\/.*$/, 'comment'],
      [/\/\*/, 'comment', '@comment'],

      // Strings
      [/"([^"\\]|\\.)*$/, 'string.invalid'],
      [/'([^'\\]|\\.)*$/, 'string.invalid'],
      [/"/, 'string', '@stringDouble'],
      [/'/, 'string', '@stringSingle'],
      [/`/, 'string', '@stringBacktick'],

      // Numbers
      [/0[xX][0-9a-fA-F]+/, 'number.hex'],
      [/0[oO][0-7]+/, 'number.octal'],
      [/0[bB][01]+/, 'number.binary'],
      [/@digits\.@digits([eE][\-+]?@digits)?/, 'number.float'],
      [/@digits/, 'number'],

      // Keywords and identifiers
      [/[a-zA-Z_$][\w$]*/, {
        cases: {
          '@keywords': 'keyword',
          '@default': 'identifier',
        },
      }],

      // Delimiters and operators
      [/[{}()\[\]]/, 'delimiter.bracket'],
      [/[;,.]/, 'delimiter'],
      [/@symbols/, {
        cases: {
          '@operators': 'operator',
          '@default': '',
        },
      }],
    ],

    comment: [
      [/\*\//, 'comment', '@pop'],
      [/./, 'comment'],
    ],

    stringDouble: [
      [/[^\\"]+/, 'string'],
      [/@escapes/, 'string.escape'],
      [/"/, 'string', '@pop'],
    ],

    stringSingle: [
      [/[^\\']+/, 'string'],
      [/@escapes/, 'string.escape'],
      [/'/, 'string', '@pop'],
    ],

    stringBacktick: [
      [/\$\{/, 'delimiter.bracket', '@bracketCounting'],
      [/[^\\`$]+/, 'string'],
      [/@escapes/, 'string.escape'],
      [/`/, 'string', '@pop'],
    ],

    bracketCounting: [
      [/\{/, 'delimiter.bracket', '@bracketCounting'],
      [/\}/, 'delimiter.bracket', '@pop'],
      { include: '@common' },
    ],
  },
}

export function registerLanguages() {
  // Register CSS
  monaco.languages.register({
    id: 'css',
    extensions: ['.css'],
    aliases: ['CSS', 'css'],
    mimetypes: ['text/css'],
  })
  monaco.languages.setMonarchTokensProvider('css', cssTokenizer)
  monaco.languages.setLanguageConfiguration('css', css)

  // Register JavaScript
  monaco.languages.register({
    id: 'javascript',
    extensions: ['.js', '.jsx', '.mjs', '.cjs'],
    aliases: ['JavaScript', 'js', 'javascript'],
    mimetypes: ['text/javascript'],
  })
  monaco.languages.setMonarchTokensProvider('javascript', {
    ...tsTokenizer,
    tokenPostfix: '.js',
  })
  monaco.languages.setLanguageConfiguration('javascript', js)

  // Register TypeScript
  monaco.languages.register({
    id: 'typescript',
    extensions: ['.ts', '.tsx', '.mts', '.cts'],
    aliases: ['TypeScript', 'ts', 'typescript'],
    mimetypes: ['text/typescript'],
  })
  monaco.languages.setMonarchTokensProvider('typescript', tsTokenizer)
  monaco.languages.setLanguageConfiguration('typescript', ts)
}
