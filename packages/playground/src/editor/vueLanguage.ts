import * as monaco from 'monaco-editor-core'

/**
 * Vue SFC Monarch language definition
 * Auto-generated from extensions/vscode/syntaxes/vue.tmLanguage.json
 * Run: pnpm run generate:vue-language
 */
const vueLanguage: monaco.languages.IMonarchLanguage = {
  defaultToken: '',
  tokenPostfix: '.vue',
  ignoreCase: false,

  // Embedded language patterns
  brackets: [
    { open: '{{', close: '}}', token: 'delimiter.interpolation' },
    { open: '{', close: '}', token: 'delimiter.bracket' },
    { open: '[', close: ']', token: 'delimiter.bracket' },
    { open: '(', close: ')', token: 'delimiter.bracket' },
    { open: '<', close: '>', token: 'delimiter.html' },
  ],

  keywords: [
    'import', 'export', 'from', 'default', 'const', 'let', 'var', 'function',
    'return', 'if', 'else', 'for', 'while', 'do', 'switch', 'case', 'break',
    'continue', 'new', 'this', 'class', 'extends', 'implements', 'interface',
    'type', 'enum', 'async', 'await', 'try', 'catch', 'finally', 'throw',
    'typeof', 'instanceof', 'in', 'of', 'void', 'null', 'undefined', 'true', 'false',
    'as', 'is', 'keyof', 'readonly', 'public', 'private', 'protected', 'static',
    'abstract', 'declare', 'namespace', 'module', 'require', 'yield', 'delete',
    'debugger', 'with', 'super', 'get', 'set', 'satisfies',
  ],

  vueKeywords: [
    'ref', 'reactive', 'computed', 'watch', 'watchEffect', 'watchPostEffect',
    'watchSyncEffect', 'onMounted', 'onUnmounted', 'onBeforeMount', 'onBeforeUnmount',
    'onUpdated', 'onBeforeUpdate', 'onActivated', 'onDeactivated', 'onErrorCaptured',
    'onRenderTracked', 'onRenderTriggered', 'onServerPrefetch',
    'defineProps', 'defineEmits', 'defineExpose', 'defineOptions', 'defineSlots',
    'defineModel', 'withDefaults', 'useSlots', 'useAttrs', 'provide', 'inject',
    'toRef', 'toRefs', 'toRaw', 'unref', 'isRef', 'isReactive', 'isReadonly',
    'shallowRef', 'shallowReactive', 'shallowReadonly', 'markRaw', 'effectScope',
    'getCurrentScope', 'onScopeDispose', 'triggerRef', 'customRef',
    'nextTick', 'h', 'createApp', 'defineComponent', 'defineAsyncComponent',
  ],

  typeKeywords: [
    'string', 'number', 'boolean', 'object', 'any', 'never', 'unknown', 'void',
    'Array', 'Object', 'Function', 'Promise', 'Map', 'Set', 'WeakMap', 'WeakSet',
    'Record', 'Partial', 'Required', 'Readonly', 'Pick', 'Omit', 'Exclude',
    'Extract', 'NonNullable', 'ReturnType', 'InstanceType', 'Parameters',
    'Ref', 'ComputedRef', 'ShallowRef', 'UnwrapRef', 'ToRefs', 'PropType',
  ],

  operators: [
    '=', '>', '<', '!', '~', '?', ':', '==', '<=', '>=', '!=', '===', '!==',
    '&&', '||', '??', '++', '--', '+', '-', '*', '/', '&', '|', '^', '%',
    '<<', '>>', '>>>', '+=', '-=', '*=', '/=', '&=', '|=', '^=', '%=',
    '<<=', '>>=', '>>>=', '=>', '?.', '...', '??=', '&&=', '||=',
  ],

  symbols: /[=><!~?:&|+\-*\/\^%]+/,
  escapes: /\\(?:[abfnrtv\\"']|x[0-9A-Fa-f]{1,4}|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})/,
  digits: /\d+(_+\d+)*/,

  tokenizer: {
    root: [
      // HTML comments
      [/<!--/, 'comment', '@htmlComment'],

      // Script block
      [/(<)(script)/, ['delimiter.html', { token: 'tag', next: '@scriptTag' }]],
      [/(<\/)(script)(>)/, ['delimiter.html', 'tag', 'delimiter.html']],

      // Style block
      [/(<)(style)/, ['delimiter.html', { token: 'tag', next: '@styleTag' }]],
      [/(<\/)(style)(>)/, ['delimiter.html', 'tag', 'delimiter.html']],

      // Template block
      [/(<)(template)/, ['delimiter.html', { token: 'tag', next: '@templateTag' }]],
      [/(<\/)(template)(>)/, ['delimiter.html', 'tag', 'delimiter.html']],

      // Other HTML tags
      [/(<)(\w+)/, ['delimiter.html', { token: 'tag', next: '@htmlTag' }]],
      [/(<\/)(\w+)(>)/, ['delimiter.html', 'tag', 'delimiter.html']],

      // Text content
      [/[^<]+/, ''],
    ],

    htmlComment: [
      [/-->/, 'comment', '@pop'],
      [/./, 'comment'],
    ],

    // Script tag attributes and body (combined to avoid nested state issues)
    scriptTag: [
      // First handle attributes until we see >
      [/\s+/, ''],
      [/(setup|lang|src|generic)/, 'attribute.name'],
      [/=/, 'delimiter'],
      [/"ts"|'ts'/, 'attribute.value'],
      [/"[^"]*"|'[^']*'/, 'attribute.value'],
      [/\/>/, 'delimiter.html', '@pop'],
      [/>/, 'delimiter.html'],
      // Then handle script body content
      [/(<\/)(script)(>)/, ['delimiter.html', 'tag', { token: 'delimiter.html', next: '@pop' }]],
      { include: '@typescript' },
    ],

    typescript: [
      // Whitespace
      [/\s+/, ''],

      // Comments
      [/\/\/.*$/, 'comment'],
      [/\/\*/, 'comment', '@tsComment'],

      // JSX/TSX tags (for potential JSX in script)
      [/(<)(\w+)/, ['delimiter.html', 'tag']],
      [/(<\/)(\w+)(>)/, ['delimiter.html', 'tag', 'delimiter.html']],

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
      [/@digits[eE][\-+]?@digits/, 'number.float'],
      [/@digits/, 'number'],

      // Keywords and identifiers
      [/[a-zA-Z_$][\w$]*/, {
        cases: {
          '@keywords': 'keyword',
          '@vueKeywords': 'keyword',
          '@typeKeywords': 'type',
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

    tsComment: [
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
      [/\$\{/, 'delimiter.bracket', '@templateExpr'],
      [/[^\\`$]+/, 'string'],
      [/@escapes/, 'string.escape'],
      [/`/, 'string', '@pop'],
    ],

    templateExpr: [
      [/\}/, 'delimiter.bracket', '@pop'],
      { include: '@typescript' },
    ],

    // Style tag attributes and body (combined to avoid nested state issues)
    styleTag: [
      // First handle attributes until we see >
      [/\s+/, ''],
      [/(scoped|lang|src|module)/, 'attribute.name'],
      [/=/, 'delimiter'],
      [/"[^"]*"|'[^']*'/, 'attribute.value'],
      [/\/>/, 'delimiter.html', '@pop'],
      [/>/, 'delimiter.html'],
      // Then handle style body content
      [/(<\/)(style)(>)/, ['delimiter.html', 'tag', { token: 'delimiter.html', next: '@pop' }]],
      { include: '@css' },
    ],

    css: [
      // Comments
      [/\/\*/, 'comment', '@cssComment'],
      // At-rules
      [/@[\w-]+/, 'keyword'],
      // Class and ID selectors
      [/[.#][\w-]+/, 'tag.class'],
      // Element selectors and pseudo
      [/[\w-]+/, 'tag'],
      [/:+[\w-]+/, 'tag.pseudo'],
      // Combinators
      [/[>+~]/, 'operator'],
      // Open declaration block
      [/\{/, 'delimiter.bracket', '@cssDeclarations'],
      // Whitespace
      [/\s+/, ''],
    ],

    cssDeclarations: [
      // Comments
      [/\/\*/, 'comment', '@cssComment'],
      // Close declaration block
      [/\}/, 'delimiter.bracket', '@pop'],
      // Property names
      [/[\w-]+(?=\s*:)/, 'attribute.name'],
      // CSS variables
      [/--[\w-]+/, 'variable'],
      // Colon
      [/:/, 'delimiter', '@cssValue'],
      // Whitespace
      [/\s+/, ''],
    ],

    cssValue: [
      // End of value
      [/;/, 'delimiter', '@pop'],
      [/\}/, { token: 'delimiter.bracket', next: '@pop', goBack: 1 }],
      // Comments
      [/\/\*/, 'comment', '@cssComment'],
      // Important
      [/!important/, 'keyword'],
      // Colors
      [/#[0-9a-fA-F]{3,8}/, 'number.hex'],
      // Numbers with units
      [/\d+(\.\d+)?(px|em|rem|%|vh|vw|vmin|vmax|ch|ex|s|ms|deg|rad|turn|fr)?/, 'number'],
      // Strings
      [/"[^"]*"/, 'string'],
      [/'[^']*'/, 'string'],
      // Functions
      [/[\w-]+(?=\()/, 'attribute.value'],
      [/\(/, 'delimiter.parenthesis', '@cssFunctionArgs'],
      // CSS variables reference
      [/var/, 'attribute.value'],
      // Other values
      [/[\w-]+/, 'attribute.value'],
      // Operators
      [/[,\/]/, 'delimiter'],
      // Whitespace
      [/\s+/, ''],
    ],

    cssFunctionArgs: [
      [/\)/, 'delimiter.parenthesis', '@pop'],
      [/--[\w-]+/, 'variable'],
      [/#[0-9a-fA-F]{3,8}/, 'number.hex'],
      [/\d+(\.\d+)?(px|em|rem|%|vh|vw|deg|rad)?/, 'number'],
      [/"[^"]*"/, 'string'],
      [/'[^']*'/, 'string'],
      [/[\w-]+/, 'attribute.value'],
      [/[,]/, 'delimiter'],
      [/\s+/, ''],
    ],

    cssComment: [
      [/\*\//, 'comment', '@pop'],
      [/./, 'comment'],
    ],

    // Template tag attributes and body (combined to avoid nested state issues)
    templateTag: [
      // First handle attributes until we see >
      [/\s+/, ''],
      [/(lang|src)/, 'attribute.name'],
      [/=/, 'delimiter'],
      [/"[^"]*"|'[^']*'/, 'attribute.value'],
      [/\/>/, 'delimiter.html', '@pop'],
      [/>/, 'delimiter.html'],
      // Then handle template body content
      [/(<\/)(template)(>)/, ['delimiter.html', 'tag', { token: 'delimiter.html', next: '@pop' }]],
      { include: '@vueTemplate' },
    ],

    vueTemplate: [
      // Comments
      [/<!--/, 'comment', '@htmlComment'],

      // Vue directives
      [/(@|v-on:|v-bind:|v-slot:|v-model|v-if|v-else-if|v-else|v-for|v-show|v-html|v-text|v-pre|v-cloak|v-once|v-memo|v-)/, 'attribute.name'],
      [/(:|@|#)[\w.-]+/, 'attribute.name'],

      // Mustache interpolation
      [/\{\{/, 'delimiter.interpolation', '@interpolation'],

      // HTML tags
      [/(<)([A-Z][\w-]*)/, ['delimiter.html', 'tag']], // Component tags (PascalCase)
      [/(<)([a-z][\w-]*)/, ['delimiter.html', 'tag.html']], // HTML tags
      [/(<\/)([\w-]+)(>)/, ['delimiter.html', 'tag', 'delimiter.html']],
      [/\/>/, 'delimiter.html'],
      [/>/, 'delimiter.html'],

      // Attributes
      [/[\w-]+(?=\s*=)/, 'attribute.name'],
      [/=/, 'delimiter'],
      [/"[^"]*"/, 'attribute.value'],
      [/'[^']*'/, 'attribute.value'],

      // Text
      [/[^<{]+/, ''],
    ],

    interpolation: [
      [/\}\}/, 'delimiter.interpolation', '@pop'],
      // Simple expressions in interpolation
      [/[a-zA-Z_$][\w$]*/, {
        cases: {
          '@keywords': 'keyword',
          '@default': 'identifier',
        },
      }],
      [/\./, 'delimiter'],
      [/[()]/, 'delimiter.bracket'],
      [/"[^"]*"/, 'string'],
      [/'[^']*'/, 'string'],
      [/\d+/, 'number'],
      [/[+\-*/%<>=!&|?:]/, 'operator'],
    ],

    // Generic HTML tag
    htmlTag: [
      [/\s+/, ''],
      [/[\w-]+(?=\s*=)/, 'attribute.name'],
      [/=/, 'delimiter'],
      [/"[^"]*"/, 'attribute.value'],
      [/'[^']*'/, 'attribute.value'],
      [/\/>/, 'delimiter.html', '@pop'],
      [/>/, 'delimiter.html', '@pop'],
    ],
  },
}

// Language configuration from vue-language-configuration.json
const vueLanguageConfig: monaco.languages.LanguageConfiguration = {
  comments: {"blockComment":["<!--","-->"]},
  brackets: [["<!--","-->"],["{{","}}"],["(",")"],["{","}"],["[","]"]],
  autoClosingPairs: [{"open":"{{","close":"}}"},{"open":"{","close":"}"},{"open":"[","close":"]"},{"open":"(","close":")"},{"open":"'","close":"'"},{"open":"\"","close":"\""},{"open":"<!--","close":"-->","notIn":["comment","string"]}],
  surroundingPairs: [{"open":"'","close":"'"},{"open":"\"","close":"\""},{"open":"{{","close":"}}"},{"open":"{","close":"}"},{"open":"[","close":"]"},{"open":"(","close":")"},{"open":"<!--","close":"-->"}],
  folding: {
    markers: {
      start: /^\s*<!--\s*#region\b.*-->/,
      end: /^\s*<!--\s*#endregion\b.*-->/,
    },
  },
}

export function registerVueLanguage() {
  // Register the Vue language
  monaco.languages.register({
    id: 'vue',
    extensions: ['.vue'],
    aliases: ['Vue', 'vue'],
  })

  // Set the tokenizer
  monaco.languages.setMonarchTokensProvider('vue', vueLanguage)

  // Set the language configuration
  monaco.languages.setLanguageConfiguration('vue', vueLanguageConfig)
}
