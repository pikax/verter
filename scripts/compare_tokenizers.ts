/**
 * Compare Vue tokenizer output with Slim tokenizer output
 * to identify discrepancies
 */

import * as fs from "fs";
import * as path from "path";

// Vue tokenizer implementation (simplified from original)
const enum CharCodes {
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
  Dot = 0x2e,
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
  Colon = 0x3a,
  At = 0x40,
  LeftSquare = 91,
  RightSquare = 93,
}

const enum State {
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

const enum QuoteType {
  NoValue = 0,
  Unquoted = 1,
  Single = 2,
  Double = 3,
}

interface TokenEvent {
  type: string;
  args: number[];
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

function isTagStartChar(c: number): boolean {
  return (
    (c >= CharCodes.LowerA && c <= CharCodes.LowerZ) ||
    (c >= CharCodes.UpperA && c <= CharCodes.UpperZ)
  );
}

function tokenizeVue(input: string): TokenEvent[] {
  const events: TokenEvent[] = [];
  let index = 0;
  let sectionStart = 0;
  let state: State = State.Text;
  let inVPre = false;
  const delimiterOpen = "{{";
  const delimiterClose = "}}";

  function stateText(c: number): void {
    if (c === CharCodes.Lt) {
      if (index > sectionStart) {
        events.push({ type: "Text", args: [sectionStart, index] });
      }
      state = State.BeforeTagName;
      sectionStart = index;
    } else if (!inVPre && c === delimiterOpen.charCodeAt(0)) {
      if (input.slice(index, index + delimiterOpen.length) === delimiterOpen) {
        if (index > sectionStart) {
          events.push({ type: "Text", args: [sectionStart, index] });
        }
        state = State.InterpolationOpen;
        sectionStart = index;
      }
    }
  }

  function stateInterpolationOpen(c: number): void {
    if (input.slice(index, index + delimiterOpen.length) === delimiterOpen) {
      index += delimiterOpen.length;
      state = State.Interpolation;
    } else {
      state = State.Text;
    }
  }

  function stateInterpolation(c: number): void {
    if (c === delimiterClose.charCodeAt(0)) {
      if (input.slice(index, index + delimiterClose.length) === delimiterClose) {
        state = State.InterpolationClose;
      }
    }
  }

  function stateInterpolationClose(): void {
    index += delimiterClose.length;
    events.push({ type: "Interpolation", args: [sectionStart, index] });
    sectionStart = index;
    state = State.Text;
  }

  function stateBeforeTagName(c: number): void {
    if (c === CharCodes.ExclamationMark) {
      state = State.BeforeDeclaration;
      sectionStart = index;
    } else if (c === CharCodes.Questionmark) {
      state = State.InProcessingInstruction;
      sectionStart = index;
    } else if (isTagStartChar(c)) {
      sectionStart = index;
      state = State.InTagName;
    } else if (c === CharCodes.Slash) {
      state = State.BeforeClosingTagName;
    } else {
      state = State.Text;
    }
  }

  function stateInTagName(c: number): void {
    if (isEndOfTagSection(c)) {
      events.push({ type: "OpenTagName", args: [sectionStart, index] });
      sectionStart = index;
      state = State.BeforeAttrName;
      stateBeforeAttrName(c);
    }
  }

  function stateBeforeClosingTagName(c: number): void {
    if (isTagStartChar(c)) {
      state = State.InClosingTagName;
      sectionStart = index;
    } else if (c === CharCodes.Gt) {
      state = State.Text;
      sectionStart = index + 1;
    } else {
      state = State.InSpecialComment;
      sectionStart = index;
    }
  }

  function stateInClosingTagName(c: number): void {
    if (c === CharCodes.Gt || isWhitespace(c)) {
      events.push({ type: "CloseTag", args: [sectionStart, index] });
      sectionStart = index;
      state = State.AfterClosingTagName;
      stateAfterClosingTagName(c);
    }
  }

  function stateAfterClosingTagName(c: number): void {
    if (c === CharCodes.Gt) {
      state = State.Text;
      sectionStart = index + 1;
    }
  }

  function stateBeforeAttrName(c: number): void {
    if (c === CharCodes.Gt) {
      events.push({ type: "OpenTagEnd", args: [index] });
      sectionStart = index + 1;
      state = State.Text;
    } else if (c === CharCodes.Slash) {
      state = State.InSelfClosingTag;
    } else if (!isWhitespace(c)) {
      handleAttrStart(c);
    }
  }

  function handleAttrStart(c: number): void {
    if (c === CharCodes.LowerV && input.charCodeAt(index + 1) === CharCodes.Dash) {
      state = State.InDirName;
      sectionStart = index;
    } else if (
      c === CharCodes.Dot ||
      c === CharCodes.Colon ||
      c === CharCodes.At ||
      c === CharCodes.Number
    ) {
      state = State.InDirName;
      sectionStart = index;
    } else {
      state = State.InAttrName;
      sectionStart = index;
    }
  }

  function stateInSelfClosingTag(c: number): void {
    if (c === CharCodes.Gt) {
      events.push({ type: "SelfClosingTag", args: [index] });
      state = State.Text;
      sectionStart = index + 1;
    } else if (!isWhitespace(c)) {
      state = State.BeforeAttrName;
      stateBeforeAttrName(c);
    }
  }

  function stateInAttrName(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      events.push({ type: "AttribName", args: [sectionStart, index] });
      sectionStart = index;
      state = State.AfterAttrName;
      stateAfterAttrName(c);
    }
  }

  function stateInDirName(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      events.push({ type: "DirName", args: [sectionStart, index] });
      sectionStart = index;
      state = State.AfterAttrName;
      stateAfterAttrName(c);
    } else if (c === CharCodes.Colon) {
      events.push({ type: "DirName", args: [sectionStart, index] });
      state = State.InDirArg;
      sectionStart = index;
    } else if (c === CharCodes.Dot) {
      events.push({ type: "DirName", args: [sectionStart, index] });
      state = State.InDirModifier;
      sectionStart = index + 1;
    }
  }

  function stateInDirArg(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      events.push({ type: "DirArg", args: [sectionStart, index] });
      sectionStart = index;
      state = State.AfterAttrName;
      stateAfterAttrName(c);
    } else if (c === CharCodes.LeftSquare) {
      state = State.InDirDynamicArg;
    } else if (c === CharCodes.Dot) {
      events.push({ type: "DirArg", args: [sectionStart, index] });
      state = State.InDirModifier;
      sectionStart = index + 1;
    }
  }

  function stateInDirDynamicArg(c: number): void {
    if (c === CharCodes.RightSquare) {
      state = State.InDirArg;
    }
  }

  function stateInDirModifier(c: number): void {
    if (c === CharCodes.Eq || isEndOfTagSection(c)) {
      events.push({ type: "DirModifier", args: [sectionStart, index] });
      sectionStart = index;
      state = State.AfterAttrName;
      stateAfterAttrName(c);
    } else if (c === CharCodes.Dot) {
      events.push({ type: "DirModifier", args: [sectionStart, index] });
      sectionStart = index + 1;
    }
  }

  function stateAfterAttrName(c: number): void {
    if (c === CharCodes.Eq) {
      events.push({ type: "AttribNameEnd", args: [index] });
      state = State.BeforeAttrValue;
    } else if (c === CharCodes.Gt || c === CharCodes.Slash) {
      events.push({ type: "AttribNameEnd", args: [sectionStart] });
      events.push({ type: "AttribEnd", args: [QuoteType.NoValue, sectionStart] });
      state = State.BeforeAttrName;
      stateBeforeAttrName(c);
    } else if (!isWhitespace(c)) {
      events.push({ type: "AttribNameEnd", args: [sectionStart] });
      events.push({ type: "AttribEnd", args: [QuoteType.NoValue, sectionStart] });
      handleAttrStart(c);
    }
  }

  function stateBeforeAttrValue(c: number): void {
    if (c === CharCodes.DoubleQuote) {
      state = State.InAttrValueDq;
      sectionStart = index + 1;
    } else if (c === CharCodes.SingleQuote) {
      state = State.InAttrValueSq;
      sectionStart = index + 1;
    } else if (!isWhitespace(c)) {
      state = State.InAttrValueNq;
      sectionStart = index;
    }
  }

  function stateInAttrValueDq(c: number): void {
    if (c === CharCodes.DoubleQuote) {
      events.push({ type: "AttribData", args: [sectionStart, index] });
      events.push({ type: "AttribEnd", args: [QuoteType.Double, index + 1] });
      state = State.BeforeAttrName;
      sectionStart = index + 1;
    }
  }

  function stateInAttrValueSq(c: number): void {
    if (c === CharCodes.SingleQuote) {
      events.push({ type: "AttribData", args: [sectionStart, index] });
      events.push({ type: "AttribEnd", args: [QuoteType.Single, index + 1] });
      state = State.BeforeAttrName;
      sectionStart = index + 1;
    }
  }

  function stateInAttrValueNq(c: number): void {
    if (isWhitespace(c) || c === CharCodes.Gt) {
      events.push({ type: "AttribData", args: [sectionStart, index] });
      events.push({ type: "AttribEnd", args: [QuoteType.Unquoted, index] });
      state = State.BeforeAttrName;
      sectionStart = index;
      stateBeforeAttrName(c);
    }
  }

  function stateBeforeDeclaration(c: number): void {
    if (c === CharCodes.Dash) {
      state = State.BeforeComment;
      sectionStart = index + 1;
    } else if (c === CharCodes.LeftSquare) {
      // CDATA - skipping for now
      state = State.Text;
    } else {
      state = State.InDeclaration;
    }
  }

  function stateBeforeComment(c: number): void {
    if (c === CharCodes.Dash) {
      state = State.InCommentLike;
      sectionStart = index + 1;
    } else {
      state = State.InDeclaration;
    }
  }

  function stateInCommentLike(c: number): void {
    if (c === CharCodes.Dash) {
      if (input.slice(index, index + 3) === "-->") {
        events.push({ type: "Comment", args: [sectionStart, index] });
        index += 2;
        sectionStart = index + 1;
        state = State.Text;
      }
    }
  }

  function stateInDeclaration(c: number): void {
    if (c === CharCodes.Gt) {
      state = State.Text;
      sectionStart = index + 1;
    }
  }

  function stateInProcessingInstruction(c: number): void {
    if (c === CharCodes.Gt) {
      const piStart = sectionStart + 1; // Skip '?'
      events.push({ type: "ProcessingInstruction", args: [piStart, index] });
      sectionStart = index + 1;
      state = State.Text;
    }
  }

  function stateInSpecialComment(c: number): void {
    if (c === CharCodes.Gt) {
      events.push({ type: "Comment", args: [sectionStart, index] });
      sectionStart = index + 1;
      state = State.Text;
    }
  }

  // Main loop
  while (index < input.length) {
    const c = input.charCodeAt(index);

    switch (state) {
      case State.Text:
        stateText(c);
        break;
      case State.InterpolationOpen:
        stateInterpolationOpen(c);
        continue; // Don't increment, handled inside
      case State.Interpolation:
        stateInterpolation(c);
        break;
      case State.InterpolationClose:
        stateInterpolationClose();
        continue; // Don't increment, handled inside
      case State.BeforeTagName:
        stateBeforeTagName(c);
        break;
      case State.InTagName:
        stateInTagName(c);
        break;
      case State.InSelfClosingTag:
        stateInSelfClosingTag(c);
        break;
      case State.BeforeClosingTagName:
        stateBeforeClosingTagName(c);
        break;
      case State.InClosingTagName:
        stateInClosingTagName(c);
        break;
      case State.AfterClosingTagName:
        stateAfterClosingTagName(c);
        break;
      case State.BeforeAttrName:
        stateBeforeAttrName(c);
        break;
      case State.InAttrName:
        stateInAttrName(c);
        break;
      case State.InDirName:
        stateInDirName(c);
        break;
      case State.InDirArg:
        stateInDirArg(c);
        break;
      case State.InDirDynamicArg:
        stateInDirDynamicArg(c);
        break;
      case State.InDirModifier:
        stateInDirModifier(c);
        break;
      case State.AfterAttrName:
        stateAfterAttrName(c);
        break;
      case State.BeforeAttrValue:
        stateBeforeAttrValue(c);
        break;
      case State.InAttrValueDq:
        stateInAttrValueDq(c);
        break;
      case State.InAttrValueSq:
        stateInAttrValueSq(c);
        break;
      case State.InAttrValueNq:
        stateInAttrValueNq(c);
        break;
      case State.BeforeDeclaration:
        stateBeforeDeclaration(c);
        break;
      case State.BeforeComment:
        stateBeforeComment(c);
        break;
      case State.InCommentLike:
        stateInCommentLike(c);
        break;
      case State.InDeclaration:
        stateInDeclaration(c);
        break;
      case State.InProcessingInstruction:
        stateInProcessingInstruction(c);
        break;
      case State.InSpecialComment:
        stateInSpecialComment(c);
        break;
    }

    index++;
  }

  // Emit trailing text
  if (state === State.Text && sectionStart < index) {
    events.push({ type: "Text", args: [sectionStart, index] });
  }

  events.push({ type: "End", args: [] });
  return events;
}

// Find vue files
function findVueFiles(dir: string): string[] {
  const files: string[] = [];

  function scan(d: string): void {
    try {
      const entries = fs.readdirSync(d, { withFileTypes: true });
      for (const entry of entries) {
        const fullPath = path.join(d, entry.name);
        if (entry.isDirectory()) {
          if (
            !entry.name.startsWith(".") &&
            entry.name !== "node_modules" &&
            entry.name !== "dist"
          ) {
            scan(fullPath);
          }
        } else if (entry.name.endsWith(".vue")) {
          files.push(fullPath);
        }
      }
    } catch (e) {}
  }

  scan(dir);
  return files;
}

// Extract template
function extractTemplate(content: string): string | null {
  const start = content.indexOf("<template");
  if (start === -1) return null;
  const tagEnd = content.indexOf(">", start);
  if (tagEnd === -1) return null;
  const end = content.lastIndexOf("</template>");
  if (end === -1 || end <= tagEnd) return null;
  return content.slice(start, end + "</template>".length);
}

// Compare events
function compareEvents(vue: TokenEvent[], slim: TokenEvent[]): string[] {
  const diffs: string[] = [];
  const maxLen = Math.max(vue.length, slim.length);

  for (let i = 0; i < maxLen; i++) {
    const v = vue[i];
    const s = slim[i];

    if (!v) {
      diffs.push(`[${i}] SLIM extra: ${JSON.stringify(s)}`);
    } else if (!s) {
      diffs.push(`[${i}] VUE extra: ${JSON.stringify(v)}`);
    } else if (v.type !== s.type || JSON.stringify(v.args) !== JSON.stringify(s.args)) {
      diffs.push(`[${i}] MISMATCH: VUE=${JSON.stringify(v)} SLIM=${JSON.stringify(s)}`);
    }
  }

  return diffs;
}

// Main
const folders = ["D:\\dev\\accioresearch\\WLS\\avava\\src"];

let totalDiffs = 0;
let filesWithDiffs = 0;
let totalFiles = 0;

for (const folder of folders) {
  if (!fs.existsSync(folder)) continue;

  const files = findVueFiles(folder);
  console.log(`Processing ${files.length} files from ${folder}`);

  for (const file of files.slice(0, 10)) {
    // Limit to first 10 for analysis
    totalFiles++;
    const content = fs.readFileSync(file, "utf-8");
    const template = extractTemplate(content);
    if (!template) continue;

    const vueEvents = tokenizeVue(template);

    // We don't have slim tokenizer in TS, so we'll just output Vue events for now
    // The comparison will be done by looking at the counts
    console.log(`\n=== ${path.basename(file)} ===`);
    console.log(`Template length: ${template.length}`);
    console.log(`Vue events: ${vueEvents.length}`);

    // Show first few events
    console.log("First 20 events:");
    for (const e of vueEvents.slice(0, 20)) {
      console.log(`  ${e.type}(${e.args.join(", ")})`);
    }
  }
}
