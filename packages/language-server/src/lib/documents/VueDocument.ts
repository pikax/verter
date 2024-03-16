import { Range, TextDocument } from "vscode-languageserver-textdocument";
import { basename } from "path";
import { WritableDocument } from "./Document";
import { urlToPath } from "../../utils";
import { importVueCompiler } from "../../importPackages";
import { TemplateBuilder, createBuilder } from "@verter/core";
import { ParseScriptContext } from "@verter/core/src/plugins";
import { BulkRegistration } from "vscode-languageserver";
import { MagicString } from "vue/compiler-sfc";

import { SourceMapConsumer } from 'source-map-js'

export class VueDocument extends WritableDocument {
  languageId = "vue";

  get uri() {
    return this._uri;
  }
  get filePath() {
    return this.path;
  }
  private path = urlToPath(this._uri);

  get parsed() {
    return this._parsed;
  }

  get blocks() {
    return this._blocks
  }

  _name = ''

  private _compiler = importVueCompiler(this._uri);
  private _parsed = this.parseVue();
  private _builder = createBuilder()
  private _blocks = [] as Array<'script' | 'template' | 'style'>;
  constructor(private _uri: string, private content: string) {
    super();
  }

  fromTextDocument(doc: TextDocument) {
    this.version = doc.version;
    this.content = doc.getText();
    this._parsed = this.parseVue();
    this._blocks = this.updateBlocks()
    // this.lineCount = doc.lineCount;
  }

  setText(text: string) {
    this.content = text;
    ++this.version;
    this.lineOffsets = undefined;
    this._parsed = this.parseVue();
  }
  getText(range?: Range | undefined): string {
    if (range) {
      return this.content.substring(
        this.offsetAt(range.start),
        this.offsetAt(range.end)
      );
    }
    return this.content;
  }

  protected parseVue() {
    // TODO add options to parser
    const name = basename(this._uri);

    this._name = name

    const parsed = this._compiler.parse(this.content, {
      filename: name,
      //   filename: this._uri,
      templateParseOptions: {
        parseMode: "sfc",
      },
    });

    // console.log("");
    // Template.process({

    // })

    const context = {
      filename: name,
      id: this._uri,
      isSetup: false, //Boolean(compiled?.setup),
      sfc: parsed,
      script: null, // compiled,
      generic: undefined, //compiled?.attrs.generic,
      template: parsed.descriptor.template,
    } satisfies ParseScriptContext;

    const { content, map } = TemplateBuilder.process(context);

    this.template = {
      content,
      map,
      mapConsumer: new SourceMapConsumer(map!)
    }

    // const response = this._builder.process(name, this.content)
    // console.log('contnet', content)

    return parsed;
  }

  template: {
    content: string,
    map?: ReturnType<MagicString['generateMap']>
    mapConsumer: SourceMapConsumer
  }

  // something() {



  //   this.template.mapConsumer.generatedPositionFor()
  // }


  protected updateBlocks() {
    const parsed = this._parsed;
    this._blocks = [];
    if (parsed.descriptor.script || parsed.descriptor.scriptSetup) {
      this._blocks.push('script')
    }
    if (parsed.descriptor.template) {
      this._blocks.push('template')
    }
    if (parsed.descriptor.styles?.length > 0) {
      this._blocks.push('style')
    }
    return this._blocks
  }
}
