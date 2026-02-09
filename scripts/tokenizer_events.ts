/**
 * Script to extract tokenizer events from the Vue tokenizer (source of truth)
 * Run with: npx ts-node --esm scripts/tokenizer_events.ts
 */

// Polyfill the build-time flags
declare global {
  var __BROWSER__: boolean;
  var __DEV__: boolean;
}
(globalThis as any).__BROWSER__ = true;
(globalThis as any).__DEV__ = true;

enum QuoteType {
  NoValue = 0,
  Unquoted = 1,
  Single = 2,
  Double = 3,
}

interface TokenEvent {
  type: string;
  args: (number | string)[];
}

class EventRecorder {
  events: TokenEvent[] = [];

  ontext(start: number, endIndex: number): void {
    this.events.push({ type: 'Text', args: [start, endIndex] });
  }
  ontextentity(char: string, start: number, endIndex: number): void {
    this.events.push({ type: 'TextEntity', args: [char, start, endIndex] });
  }
  oninterpolation(start: number, endIndex: number): void {
    this.events.push({ type: 'Interpolation', args: [start, endIndex] });
  }
  onopentagname(start: number, endIndex: number): void {
    this.events.push({ type: 'OpenTagName', args: [start, endIndex] });
  }
  onopentagend(endIndex: number): void {
    this.events.push({ type: 'OpenTagEnd', args: [endIndex] });
  }
  onselfclosingtag(endIndex: number): void {
    this.events.push({ type: 'SelfClosingTag', args: [endIndex] });
  }
  onclosetag(start: number, endIndex: number): void {
    this.events.push({ type: 'CloseTag', args: [start, endIndex] });
  }
  onattribdata(start: number, endIndex: number): void {
    this.events.push({ type: 'AttribData', args: [start, endIndex] });
  }
  onattribentity(char: string, start: number, end: number): void {
    this.events.push({ type: 'AttribEntity', args: [char, start, end] });
  }
  onattribend(quote: QuoteType, endIndex: number): void {
    this.events.push({ type: 'AttribEnd', args: [quote, endIndex] });
  }
  onattribname(start: number, endIndex: number): void {
    this.events.push({ type: 'AttribName', args: [start, endIndex] });
  }
  onattribnameend(endIndex: number): void {
    this.events.push({ type: 'AttribNameEnd', args: [endIndex] });
  }
  ondirname(start: number, endIndex: number): void {
    this.events.push({ type: 'DirName', args: [start, endIndex] });
  }
  ondirarg(start: number, endIndex: number): void {
    this.events.push({ type: 'DirArg', args: [start, endIndex] });
  }
  ondirmodifier(start: number, endIndex: number): void {
    this.events.push({ type: 'DirModifier', args: [start, endIndex] });
  }
  oncomment(start: number, endIndex: number): void {
    this.events.push({ type: 'Comment', args: [start, endIndex] });
  }
  oncdata(start: number, endIndex: number): void {
    this.events.push({ type: 'Cdata', args: [start, endIndex] });
  }
  onprocessinginstruction(start: number, endIndex: number): void {
    this.events.push({ type: 'ProcessingInstruction', args: [start, endIndex] });
  }
  onend(): void {
    this.events.push({ type: 'End', args: [] });
  }
  onerr(code: number, index: number): void {
    this.events.push({ type: 'Error', args: [code, index] });
  }
}

// Inline a simplified version of the tokenizer for browser mode
// (since the full tokenizer has complex imports)

enum CharCodes {
  Tab = 0x9,
  NewLine = 0xa,
  FormFeed = 0xc,
  CarriageReturn = 0xd,
  Space = 0x20,
  ExclamationMark = 0x21,
  Number = 0x23,
  Amp = 0x26,
  SingleQuote = 0x27,
  DoubleQuote = 0x22,
  GraveAccent = 96,
  Dash = 0x2d,
  Slash = 0x2f,
  Zero = 0x30,
  Nine = 0x39,
  Semi = 0x3b,
  Lt = 0x3c,
  Eq = 0x3d,
  Gt = 0x3e,
  Questionmark = 0x3f,
  UpperA = 0x41,
  LowerA = 0x61,
  UpperF = 0x46,
  LowerF = 0x66,
  UpperZ = 0x5a,
  LowerZ = 0x7a,
  LowerX = 0x78,
  LowerV = 0x76,
  Dot = 0x2e,
  Colon = 0x3a,
  At = 0x40,
  LeftSquare = 91,
  RightSquare = 93,
}

const defaultDelimitersOpen = new Uint8Array([123, 123]);
const defaultDelimitersClose = new Uint8Array([125, 125]);

enum State {
  Text = 1,
  InterpolationOpen,
  Interpolation,
  InterpolationClose,
  BeforeTagName,
  InTagName,
  InSelfClosingTag,
  BeforeClosingTagName,
  InClosingTagName,
  AfterClosingTagName,
  BeforeAttrName,
  InAttrName,
  InDirName,
  InDirArg,
  InDirDynamicArg,
  InDirModifier,
  AfterAttrName,
  BeforeAttrValue,
  InAttrValueDq,
  InAttrValueSq,
  InAttrValueNq,
  BeforeDeclaration,
  InDeclaration,
  InProcessingInstruction,
  BeforeComment,
  CDATASequence,
  InSpecialComment,
  InCommentLike,
  BeforeSpecialS,
  BeforeSpecialT,
  SpecialStartSequence,
  InRCDATA,
  InEntity,
  InSFCRootTagName,
}

function isTagStartChar(c: number): boolean {
  return (
    (c >= CharCodes.LowerA && c <= CharCodes.LowerZ) ||
    (c >= CharCodes.UpperA && c <= CharCodes.UpperZ)
  );
}

function isWhitespace(c: number): boolean {
  return (
    c === CharCodes.Space ||
    c === CharCodes.NewLine ||
    c === CharCodes.Tab ||
    c === CharCodes.FormFeed ||
    c === CharCodes.CarriageReturn
  );
}

function isEndOfTagSection(c: number): boolean {
  return c === CharCodes.Slash || c === CharCodes.Gt || isWhitespace(c);
}

const Sequences = {
  Cdata: new Uint8Array([0x43, 0x44, 0x41, 0x54, 0x41, 0x5b]),
  CdataEnd: new Uint8Array([0x5d, 0x5d, 0x3e]),
  CommentEnd: new Uint8Array([0x2d, 0x2d, 0x3e]),
  ScriptEnd: new Uint8Array([0x3c, 0x2f, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74]),
  StyleEnd: new Uint8Array([0x3c, 0x2f, 0x73, 0x74, 0x79, 0x6c, 0x65]),
  TitleEnd: new Uint8Array([0x3c, 0x2f, 0x74, 0x69, 0x74, 0x6c, 0x65]),
  TextareaEnd: new Uint8Array([0x3c, 0x2f, 116, 101, 120, 116, 97, 114, 101, 97]),
};

class Tokenizer {
  state: State = State.Text;
  private buffer = '';
  sectionStart = 0;
  private index = 0;
  private baseState = State.Text;
  inRCDATA = false;
  inXML = false;
  inVPre = false;
  private newlines: number[] = [];

  delimiterOpen: Uint8Array = defaultDelimitersOpen;
  delimiterClose: Uint8Array = defaultDelimitersClose;
  private delimiterIndex = -1;

  currentSequence: Uint8Array = undefined!;
  private sequenceIndex = 0;

  constructor(
    private readonly stack: any[],
    private readonly cbs: EventRecorder,
  ) {}

  reset(): void {
    this.state = State.Text;
    this.buffer = '';
    this.sectionStart = 0;
    this.index = 0;
    this.baseState = State.Text;
    this.inRCDATA = false;
    this.currentSequence = undefined!;
    this.newlines.length = 0;
    this.delimiterOpen = defaultDelimitersOpen;
    this.delimiterClose = defaultDelimitersClose;
  }

  private peek() {
    return this.buffer.charCodeAt(this.index + 1);
  }

  private stateText(c: number): void {
    if (c === CharCodes.Lt) {
      if (this.index > this.sectionStart) {
        this.cbs.ontext(this.sectionStart, this.index);
      }
      this.state = State.BeforeTagName;
      this.sectionStart = this.index;
    } else if (!this.inVPre && c === this.delimiterOpen[0]) {
      this.state = State.InterpolationOpen;
      this.delimiterIndex = 0;
      this.stateInterpolationOpen(c);
    }
  }

  private stateInterpolationOpen(c: number): void {
    if (c === this.delimiterOpen[this.delimiterIndex]) {
      if (this.delimiterIndex === this.delimiterOpen.length - 1) {
        const start = this.index + 1 - this.delimiterOpen.length;
        if (start > this.sectionStart) {
          this.cbs.ontext(this.sectionStart, start);
        }
        this.state = State.Interpolation;
        this.sectionStart = start;
      } else {
        this.delimiterIndex++;
      }
    } else if (this.inRCDATA) {
      this.state = State.InRCDATA;
      this.stateInRCDATA(c);
    } else {
      this.state = State.Text;
      this.stateText(c);
    }
  }

  private stateInterpolation(c: number): void {
    if (c === this.delimiterClose[0]) {
      this.state = State.InterpolationClose;
      this.delimiterIndex = 0;
      this.stateInterpolationClose(c);
    }
  }

  private stateInterpolationClose(c: number) {
    if (c === this.delimiterClose[this.delimiterIndex]) {
      if (this.delimiterIndex === this.delimiterClose.length - 1) {
        this.cbs.oninterpolation(this.sectionStart, this.index + 1);
        if (this.inRCDATA) {
          this.state = State.InRCDATA;
        } else {
          this.state = State.Text;
        }
        this.sectionStart = this.index + 1;
      } else {
        this.delimiterIndex++;
      }
    } else {
      this.state = State.Interpolation;
      this.stateInterpolation(c);
    }
  }

  private stateSpecialStartSequence(c: number): void {
    const isEnd = this.sequenceIndex === this.currentSequence.length;
    const isMatch = isEnd
      ? isEndOfTagSection(c)
      : (c | 0x20) === this.currentSequence[this.sequenceIndex];

    if (!isMatch) {
      this.inRCDATA = false;
    } else if (!isEnd) {
      this.sequenceIndex++;
      return;
    }

    this.sequenceIndex = 0;
    this.state = State.InTagName;
    this.stateInTagName(c);
  }

  private stateInRCDATA(c: number): void {
    if (this.sequenceIndex === this.currentSequence.length) {
      if (c === CharCodes.Gt || isWhitespace(c)) {
        const endOfText = this.index - this.currentSequence.length;
        if (this.sectionStart < endOfText) {
          const actualIndex = this.index;
          this.index = endOfText;
          this.cbs.ontext(this.sectionStart, endOfText);
          this.index = actualIndex;
        }
        this.sectionStart = endOfText + 2;
        this.stateInClosingTagName(c);
        this.inRCDATA = false;
        return;
      }
      this.sequenceIndex = 0;
    }

    if ((c | 0x20) === this.currentSequence[this.sequenceIndex]) {
      this.sequenceIndex += 1;
    } else if (this.sequenceIndex === 0) {
      if (this.fastForwardTo(CharCodes.Lt)) {
        this.sequenceIndex = 1;
      }
    } else {
      this.sequenceIndex = Number(c === CharCodes.Lt);
    }
  }

  private stateCDATASequence(c: number): void {
    if (c === Sequences.Cdata[this.sequenceIndex]) {
      if (++this.sequenceIndex === Sequences.Cdata.length) {
        this.state = State.InCommentLike;
        this.currentSequence = Sequences.CdataEnd;
        this.sequenceIndex = 0;
        this.sectionStart = this.index + 1;
      }
    } else {
      this.sequenceIndex = 0;
      this.state = State.InDeclaration;
      this.stateInDeclaration(c);
    }
  }

  private fastForwardTo(c: number): boolean {
    while (++this.index < this.buffer.length) {
      const cc = this.buffer.charCodeAt(this.index);
      if (cc === CharCodes.NewLine) {
        this.newlines.push(this.index);
      }
      if (cc === c) {
        return true;
      }
    }
    this.index = this.buffer.length - 1;
    return false;
  }

  private stateInCommentLike(c: number): void {
    if (c === this.currentSequence[this.sequenceIndex]) {
      if (++this.sequenceIndex === this.currentSequence.length) {
        if (this.currentSequence === Sequences.CdataEnd) {
          this.cbs.oncdata(this.sectionStart, this.index - 2);
        } else {
          this.cbs.oncomment(this.sectionStart, this.index - 2);
        }
        this.sequenceIndex = 0;
        this.sectionStart = this.index + 1;
        this.state = State.Text;
      }
    } else if (this.sequenceIndex === 0) {
      if (this.fastForwardTo(this.currentSequence[0])) {
        this.sequenceIndex = 1;
      }
    } else if (c !== this.currentSequence[this.sequenceIndex - 1]) {
      this.sequenceIndex = 0;
    }
  }

  private startSpecial(sequence: Uint8Array, offset: number) {
    this.enterRCDATA(sequence, offset);
    this.state = State.SpecialStartSequence;
  }

  enterRCDATA(sequence: Uint8Array, offset: number): void {
    this.inRCDATA = true;
    this.currentSequence = sequence;
    this.sequenceIndex = offset;
  }

  private stateBeforeTagName(c: number): void {
    if (c === CharCodes.ExclamationMark) {
      this.state = State.BeforeDeclaration;
      this.sectionStart = this.index + 1;
    } else if (c === CharCodes.Questionmark) {
      this.state = State.InProcessingInstruction;
      this.sectionStart = this.index + 1;
    } else if (isTagStartChar(c)) {
      this.sectionStart = this.index;
      this.state = State.InTagName;
    } else if (c === CharCodes.Slash) {
      this.state = State.BeforeClosingTagName;
    } else {
      this.state = State.Text;
      this.stateText(c);
    }
  }

  private stateInTagName(c: number): void {
    if (isEndOfTagSection(c)) {
      this.handleTagName(c);
    }
  }

  private handleTagName(c: number) {
    this.cbs.onopentagname(this.sectionStart, this.index);
    this.sectionStart = -1;
    this.state = State.BeforeAttrName;
    this.stateBeforeAttrName(c);
  }

  private stateBeforeClosingTagName(c: number): void {
    if (isWhitespace(c)) {
      // Ignore
    } else if (c === CharCodes.Gt) {
      this.cbs.onerr(5, this.index); // MISSING_END_TAG_NAME
      this.state = State.Text;
      this.sectionStart = this.index + 1;
    } else {
      this.state = isTagStartChar(c)
        ? State.InClosingTagName
        : State.InSpecialComment;
      this.sectionStart = this.index;
    }
  }

  private stateInClosingTagName(c: number): void {
    if (c === CharCodes.Gt || isWhitespace(c)) {
      this.cbs.onclosetag(this.sectionStart, this.index);
      this.sectionStart = -1;
      this.state = State.AfterClosingTagName;
      this.stateAfterClosingTagName(c);
    }
  }

  private stateAfterClosingTagName(c: number): void {
    if (c === CharCodes.Gt) {
      this.state = State.Text;
      this.sectionStart = this.index + 1;
    }
  }

  private stateBeforeAttrName(c: number): void {
    if (c === CharCodes.Gt) {
      this.cbs.onopentagend(this.index);
      if (this.inRCDATA) {
        this.state = State.InRCDATA;
      } else {
        this.state = State.Text;
      }
      this.sectionStart = this.index + 1;
    } else if (c === CharCodes.Slash) {
      this.state = State.InSelfClosingTag;
      if (this.peek() !== CharCodes.Gt) {
        this.cbs.onerr(21, this.index); // UNEXPECTED_SOLIDUS_IN_TAG
      }
    } else if (c === CharCodes.Lt && this.peek() === CharCodes.Slash) {
      this.cbs.onopentagend(this.index);
      this.state = State.BeforeTagName;
      this.sectionStart = this.index;
    } else if (!isWhitespace(c)) {
      if (c === CharCodes.Eq) {
        this.cbs.onerr(19, this.index); // UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME
      }
      this.handleAttrStart(c);
    }
  }

  private handleAttrStart(c: number) {
    if (c === CharCodes.LowerV && this.peek() === CharCodes.Dash) {
      this.state = State.InDirName;
      this.sectionStart = this.index;
    } else if (
      c === CharCodes.Dot ||
      c === CharCodes.Colon ||
      c === CharCodes.At ||
      c === CharCodes.Number
    ) {
      this.cbs.ondirname(this.index, this.index + 1);
      this.state = State.InDirArg;
      this.sectionStart = this.index + 1;
    } else {
      this.state = State.InAttrName;
      this.sectionStart = this.index;
    }
  }

  private stateInSelfClosingTag(c: number): void {
    if (c === CharCodes.Gt) {
      this.cbs.onselfclosingtag(this.index);
      this.state = State.Text;
      this.sectionStart = this.index + 1;
      this.inRCDATA = false;
    } else if (!isWhitespace(c)) {
      this.state = State.BeforeAttrName;
      this.stateBeforeAttrName(c);
    }
  }

  private stateInAttrName(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      this.cbs.onattribname(this.sectionStart, this.index);
      this.handleAttrNameEnd(c);
    } else if (
      c === CharCodes.DoubleQuote ||
      c === CharCodes.SingleQuote ||
      c === CharCodes.Lt
    ) {
      this.cbs.onerr(17, this.index); // UNEXPECTED_CHARACTER_IN_ATTRIBUTE_NAME
    }
  }

  private stateInDirName(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      this.cbs.ondirname(this.sectionStart, this.index);
      this.handleAttrNameEnd(c);
    } else if (c === CharCodes.Colon) {
      this.cbs.ondirname(this.sectionStart, this.index);
      this.state = State.InDirArg;
      this.sectionStart = this.index + 1;
    } else if (c === CharCodes.Dot) {
      this.cbs.ondirname(this.sectionStart, this.index);
      this.state = State.InDirModifier;
      this.sectionStart = this.index + 1;
    }
  }

  private stateInDirArg(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      this.cbs.ondirarg(this.sectionStart, this.index);
      this.handleAttrNameEnd(c);
    } else if (c === CharCodes.LeftSquare) {
      this.state = State.InDirDynamicArg;
    } else if (c === CharCodes.Dot) {
      this.cbs.ondirarg(this.sectionStart, this.index);
      this.state = State.InDirModifier;
      this.sectionStart = this.index + 1;
    }
  }

  private stateInDynamicDirArg(c: number): void {
    if (c === CharCodes.RightSquare) {
      this.state = State.InDirArg;
    } else if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      this.cbs.ondirarg(this.sectionStart, this.index + 1);
      this.handleAttrNameEnd(c);
      this.cbs.onerr(27, this.index); // X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END
    }
  }

  private stateInDirModifier(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      this.cbs.ondirmodifier(this.sectionStart, this.index);
      this.handleAttrNameEnd(c);
    } else if (c === CharCodes.Dot) {
      this.cbs.ondirmodifier(this.sectionStart, this.index);
      this.sectionStart = this.index + 1;
    }
  }

  private handleAttrNameEnd(c: number): void {
    this.sectionStart = this.index;
    this.state = State.AfterAttrName;
    this.cbs.onattribnameend(this.index);
    this.stateAfterAttrName(c);
  }

  private stateAfterAttrName(c: number): void {
    if (c === CharCodes.Eq) {
      this.state = State.BeforeAttrValue;
    } else if (c === CharCodes.Slash || c === CharCodes.Gt) {
      this.cbs.onattribend(QuoteType.NoValue, this.sectionStart);
      this.sectionStart = -1;
      this.state = State.BeforeAttrName;
      this.stateBeforeAttrName(c);
    } else if (!isWhitespace(c)) {
      this.cbs.onattribend(QuoteType.NoValue, this.sectionStart);
      this.handleAttrStart(c);
    }
  }

  private stateBeforeAttrValue(c: number): void {
    if (c === CharCodes.DoubleQuote) {
      this.state = State.InAttrValueDq;
      this.sectionStart = this.index + 1;
    } else if (c === CharCodes.SingleQuote) {
      this.state = State.InAttrValueSq;
      this.sectionStart = this.index + 1;
    } else if (!isWhitespace(c)) {
      this.sectionStart = this.index;
      this.state = State.InAttrValueNq;
      this.stateInAttrValueNoQuotes(c);
    }
  }

  private handleInAttrValue(c: number, quote: number) {
    if (c === quote || (this.fastForwardTo(quote))) {
      this.cbs.onattribdata(this.sectionStart, this.index);
      this.sectionStart = -1;
      this.cbs.onattribend(
        quote === CharCodes.DoubleQuote ? QuoteType.Double : QuoteType.Single,
        this.index + 1,
      );
      this.state = State.BeforeAttrName;
    }
  }

  private stateInAttrValueDoubleQuotes(c: number): void {
    this.handleInAttrValue(c, CharCodes.DoubleQuote);
  }

  private stateInAttrValueSingleQuotes(c: number): void {
    this.handleInAttrValue(c, CharCodes.SingleQuote);
  }

  private stateInAttrValueNoQuotes(c: number): void {
    if (isWhitespace(c) || c === CharCodes.Gt) {
      this.cbs.onattribdata(this.sectionStart, this.index);
      this.sectionStart = -1;
      this.cbs.onattribend(QuoteType.Unquoted, this.index);
      this.state = State.BeforeAttrName;
      this.stateBeforeAttrName(c);
    } else if (
      c === CharCodes.DoubleQuote ||
      c === CharCodes.SingleQuote ||
      c === CharCodes.Lt ||
      c === CharCodes.Eq ||
      c === CharCodes.GraveAccent
    ) {
      this.cbs.onerr(18, this.index); // UNEXPECTED_CHARACTER_IN_UNQUOTED_ATTRIBUTE_VALUE
    }
  }

  private stateBeforeDeclaration(c: number): void {
    if (c === CharCodes.LeftSquare) {
      this.state = State.CDATASequence;
      this.sequenceIndex = 0;
    } else {
      this.state =
        c === CharCodes.Dash ? State.BeforeComment : State.InDeclaration;
    }
  }

  private stateInDeclaration(c: number): void {
    if (c === CharCodes.Gt || this.fastForwardTo(CharCodes.Gt)) {
      this.state = State.Text;
      this.sectionStart = this.index + 1;
    }
  }

  private stateInProcessingInstruction(c: number): void {
    if (c === CharCodes.Gt || this.fastForwardTo(CharCodes.Gt)) {
      this.cbs.onprocessinginstruction(this.sectionStart, this.index);
      this.state = State.Text;
      this.sectionStart = this.index + 1;
    }
  }

  private stateBeforeComment(c: number): void {
    if (c === CharCodes.Dash) {
      this.state = State.InCommentLike;
      this.currentSequence = Sequences.CommentEnd;
      this.sequenceIndex = 2;
      this.sectionStart = this.index + 1;
    } else {
      this.state = State.InDeclaration;
    }
  }

  private stateInSpecialComment(c: number): void {
    if (c === CharCodes.Gt || this.fastForwardTo(CharCodes.Gt)) {
      this.cbs.oncomment(this.sectionStart, this.index);
      this.state = State.Text;
      this.sectionStart = this.index + 1;
    }
  }

  private stateBeforeSpecialS(c: number): void {
    if (c === Sequences.ScriptEnd[3]) {
      this.startSpecial(Sequences.ScriptEnd, 4);
    } else if (c === Sequences.StyleEnd[3]) {
      this.startSpecial(Sequences.StyleEnd, 4);
    } else {
      this.state = State.InTagName;
      this.stateInTagName(c);
    }
  }

  private stateBeforeSpecialT(c: number): void {
    if (c === Sequences.TitleEnd[3]) {
      this.startSpecial(Sequences.TitleEnd, 4);
    } else if (c === Sequences.TextareaEnd[3]) {
      this.startSpecial(Sequences.TextareaEnd, 4);
    } else {
      this.state = State.InTagName;
      this.stateInTagName(c);
    }
  }

  parse(input: string): void {
    this.buffer = input;
    while (this.index < this.buffer.length) {
      const c = this.buffer.charCodeAt(this.index);
      if (c === CharCodes.NewLine) {
        this.newlines.push(this.index);
      }
      switch (this.state) {
        case State.Text:
          this.stateText(c);
          break;
        case State.InterpolationOpen:
          this.stateInterpolationOpen(c);
          break;
        case State.Interpolation:
          this.stateInterpolation(c);
          break;
        case State.InterpolationClose:
          this.stateInterpolationClose(c);
          break;
        case State.SpecialStartSequence:
          this.stateSpecialStartSequence(c);
          break;
        case State.InRCDATA:
          this.stateInRCDATA(c);
          break;
        case State.CDATASequence:
          this.stateCDATASequence(c);
          break;
        case State.InAttrValueDq:
          this.stateInAttrValueDoubleQuotes(c);
          break;
        case State.InAttrName:
          this.stateInAttrName(c);
          break;
        case State.InDirName:
          this.stateInDirName(c);
          break;
        case State.InDirArg:
          this.stateInDirArg(c);
          break;
        case State.InDirDynamicArg:
          this.stateInDynamicDirArg(c);
          break;
        case State.InDirModifier:
          this.stateInDirModifier(c);
          break;
        case State.InCommentLike:
          this.stateInCommentLike(c);
          break;
        case State.InSpecialComment:
          this.stateInSpecialComment(c);
          break;
        case State.BeforeAttrName:
          this.stateBeforeAttrName(c);
          break;
        case State.InTagName:
          this.stateInTagName(c);
          break;
        case State.InClosingTagName:
          this.stateInClosingTagName(c);
          break;
        case State.BeforeTagName:
          this.stateBeforeTagName(c);
          break;
        case State.AfterAttrName:
          this.stateAfterAttrName(c);
          break;
        case State.InAttrValueSq:
          this.stateInAttrValueSingleQuotes(c);
          break;
        case State.BeforeAttrValue:
          this.stateBeforeAttrValue(c);
          break;
        case State.BeforeClosingTagName:
          this.stateBeforeClosingTagName(c);
          break;
        case State.AfterClosingTagName:
          this.stateAfterClosingTagName(c);
          break;
        case State.BeforeSpecialS:
          this.stateBeforeSpecialS(c);
          break;
        case State.BeforeSpecialT:
          this.stateBeforeSpecialT(c);
          break;
        case State.InAttrValueNq:
          this.stateInAttrValueNoQuotes(c);
          break;
        case State.InSelfClosingTag:
          this.stateInSelfClosingTag(c);
          break;
        case State.InDeclaration:
          this.stateInDeclaration(c);
          break;
        case State.BeforeDeclaration:
          this.stateBeforeDeclaration(c);
          break;
        case State.BeforeComment:
          this.stateBeforeComment(c);
          break;
        case State.InProcessingInstruction:
          this.stateInProcessingInstruction(c);
          break;
      }
      this.index++;
    }
    this.cleanup();
    this.finish();
  }

  private cleanup() {
    if (this.sectionStart !== this.index) {
      if (
        this.state === State.Text ||
        (this.state === State.InRCDATA && this.sequenceIndex === 0)
      ) {
        this.cbs.ontext(this.sectionStart, this.index);
        this.sectionStart = this.index;
      } else if (
        this.state === State.InAttrValueDq ||
        this.state === State.InAttrValueSq ||
        this.state === State.InAttrValueNq
      ) {
        this.cbs.onattribdata(this.sectionStart, this.index);
        this.sectionStart = this.index;
      }
    }
  }

  private finish() {
    this.handleTrailingData();
    this.cbs.onend();
  }

  private handleTrailingData() {
    const endIndex = this.buffer.length;
    if (this.sectionStart >= endIndex) {
      return;
    }
    if (this.state === State.InCommentLike) {
      if (this.currentSequence === Sequences.CdataEnd) {
        this.cbs.oncdata(this.sectionStart, endIndex);
      } else {
        this.cbs.oncomment(this.sectionStart, endIndex);
      }
    } else if (
      this.state === State.InTagName ||
      this.state === State.BeforeAttrName ||
      this.state === State.BeforeAttrValue ||
      this.state === State.AfterAttrName ||
      this.state === State.InAttrName ||
      this.state === State.InDirName ||
      this.state === State.InDirArg ||
      this.state === State.InDirDynamicArg ||
      this.state === State.InDirModifier ||
      this.state === State.InAttrValueSq ||
      this.state === State.InAttrValueDq ||
      this.state === State.InAttrValueNq ||
      this.state === State.InClosingTagName
    ) {
      // Ignore incomplete tags
    } else {
      this.cbs.ontext(this.sectionStart, endIndex);
    }
  }
}

function tokenize(input: string): TokenEvent[] {
  const recorder = new EventRecorder();
  const tokenizer = new Tokenizer([], recorder);
  tokenizer.parse(input);
  return recorder.events;
}

function formatEvents(events: TokenEvent[], input: string): string {
  return events.map(e => {
    const args = e.args.map(a => {
      if (typeof a === 'number') return a;
      return `'${a}'`;
    }).join(', ');

    // Show the span content for position-based events
    if (e.args.length === 2 && typeof e.args[0] === 'number' && typeof e.args[1] === 'number') {
      const content = input.slice(e.args[0] as number, e.args[1] as number);
      return `${e.type}(${args}) -> "${content}"`;
    }
    return `${e.type}(${args})`;
  }).join('\n');
}

// Test cases
const testCases = [
  '<div></div>',
  '<div>hello</div>',
  '{{ msg }}',
  '<div>{{ msg }}</div>',
  'before {{ msg }} after',
  '<div class="foo"></div>',
  "<div class='foo'></div>",
  '<div class=foo></div>',
  '<input disabled>',
  '<br/>',
  '<!-- comment -->',
  '<div v-if="show"></div>',
  '<div v-else></div>',
  '<div :class="cls"></div>',
  '<div @click="handler"></div>',
  '<div @click.stop="handler"></div>',
  '<div @click.stop.prevent="handler"></div>',
  '<div :[prop]="value"></div>',
  '<input type="text" disabled />',
  '<?xml version="1.0"?>',
  '<!DOCTYPE html><html></html>',
  '<div   class = "foo"   ></div>',
  '<div class=""></div>',
  '<!-- comment -- with dashes -->',
  `<template>
  <div v-if="show">
    <span>{{ message }}</span>
  </div>
</template>`,
];

// Output format suitable for Rust tests
console.log('// Vue Tokenizer Event Reference');
console.log('// Generated from Vue tokenizer source of truth\n');

for (const input of testCases) {
  const events = tokenize(input);
  console.log(`// Input: ${JSON.stringify(input)}`);
  console.log(formatEvents(events, input));
  console.log();
}

// Also output as JSON for programmatic use
console.log('\n// JSON format for programmatic comparison:');
console.log('const EXPECTED_EVENTS = ' + JSON.stringify(
  testCases.map(input => ({
    input,
    events: tokenize(input)
  })),
  null,
  2
));
