import type {
  SFCDescriptor,
  SFCBlock,
  MagicString,
  SFCTemplateBlock,
  SFCScriptBlock,
  SFCStyleBlock,
} from "@vue/compiler-sfc";

export type BlockPosition = {
  start: number;
  end: number;
};

// export type SFCInvalidBlock = SFCBlock & {
//   type: "invalid";
// };

export interface VerterSFCBlock<
  T extends SFCBlock =
    | SFCTemplateBlock
    | SFCScriptBlock
    | SFCStyleBlock
    | SFCBlock
  // | SFCInvalidBlock
> {
  block: T;

  tag: {
    type: string;
    /**
     * TAG Content not block content
     */
    content: string;

    pos: {
      // this is contains `content`
      open: BlockPosition;
      close: BlockPosition;
      content: BlockPosition;
    };
  };
}

const BLOCK_TAG_REGEX = /(?:<)(?<tag>\w+)(?<content>[^>]*)(?:>)/gm;
const BLOCK_END_TAG_REGEX = /(?:<\/)(?<tag>\w+)(?<content>[^>]*)(?:>)/gm;

const EMPTY_BLOCK_REGEX = /<(\w+)([^>]*>)(\s*)<\/\1>/gm;

const COMMENTED_BLOCKS_REGEX = /(<!--[\s\S]*?-->)/gm;
export function retrieveHTMLComments(source: string) {
  return Array.from(source.matchAll(COMMENTED_BLOCKS_REGEX)).map((x) => ({
    start: x.index,
    end: x.index + x[0].length,
  }));
}

export function cleanHTMLComments(source: string) {
  return source.replaceAll(COMMENTED_BLOCKS_REGEX, "").trimStart();
}

/**
 * Vue SFC Compiler does not provide information of emtpy blocks
 * This caches them and then provides an @type {SFCBlock}
 * @param descriptor From sfc parse
 */
export function catchEmptyBlocks(descriptor: SFCDescriptor): SFCBlock[] {
  const source = descriptor.source;

  const matches = source.matchAll(EMPTY_BLOCK_REGEX);
  const commentedBlocks = retrieveHTMLComments(source);

  const knownBlocks = [
    ...descriptor.customBlocks,
    descriptor.template,
    descriptor.script,
    descriptor.scriptSetup,
  ]
    .filter<SFCBlock>((x) => !!x)
    .map((x) => ({
      start: x.loc.start.offset,
      end: x.loc.end.offset,
    }))
    .concat(commentedBlocks);

  const blocks: SFCBlock[] = [];

  for (const it of matches) {
    const contentStartIndex = it.index + `<${it[1] + it[2]}`.length;
    const contentEndIndex = contentStartIndex + it[3].length;

    const isInComment = knownBlocks.some((block) => {
      return (
        (contentStartIndex >= block.start && contentStartIndex < block.end) ||
        (contentEndIndex > block.start && contentEndIndex <= block.end)
      );
    });
    if (isInComment) continue;

    blocks.push({
      type: "empty",
      attrs: {},
      content: it[3],
      // loc does not contain <{tag}> is only after
      loc: {
        source,
        start: {
          offset: contentStartIndex,
          // todo
          line: 0,
          column: 0,
        },
        end: {
          offset: contentEndIndex,
          // todo
          line: 0,
          column: 0,
        },
      },
    });
  }

  return blocks;
}

export function extractBlocksFromDescriptor(
  descriptor: SFCDescriptor
): Array<VerterSFCBlock> {
  const sfcBlocks = [
    descriptor.script,
    descriptor.scriptSetup,
    descriptor.template,
    ...descriptor.styles,
    ...descriptor.customBlocks,
    // ...catchEmptyBlocks(descriptor),
  ]
    .filter<SFCBlock>((x) => !!x)
    .sort((a, b) => {
      return a.loc.start.offset - b.loc.start.offset;
    });

  const source = descriptor.source;

  const blocks: VerterSFCBlock[] = [];

  let lastEnding = 0;
  for (let i = 0; i < sfcBlocks.length; i++) {
    const block = sfcBlocks[i];

    const initTagOffset = lastEnding;
    const initTagOffsetEnd = block.loc.start.offset;

    const endTagOffset = block.loc.end.offset;
    const nextOffsetEnd = sfcBlocks[i + 1]?.loc.start.offset ?? source.length;

    // let tagInit = cleanHTMLComments(
    //   source.slice(initTagOffset, initTagOffsetEnd)
    // );

    const tagInit = source.slice(initTagOffset, initTagOffsetEnd);

    const cleanTag = cleanHTMLComments(tagInit);

    const tagMatch = cleanTag.matchAll(BLOCK_TAG_REGEX).next()
      .value as RegExpMatchArray;

    const hasClosingTagOnContent = cleanTag.indexOf(">") < cleanTag.length - 1;

    const tag = tagMatch.groups.tag;
    // const content = tagInit.slice(tag.length+ 1, -1)// tagMatch.groups.content
    const content =
      //   tagMatch.groups.content.length >= tagInit.length - "<script>".length
      // if there's many `>`
      hasClosingTagOnContent
        ? cleanTag.slice(tag.length + 1, -1)
        : tagMatch.groups.content;

    const tagStartIndex =
      tagInit !== cleanTag
        ? tagInit.indexOf(cleanTag) + initTagOffset
        : tagMatch.index + initTagOffset;

    const startTagPos: BlockPosition = {
      start: tagStartIndex,
      end:
        tagStartIndex +
        (hasClosingTagOnContent ? tagInit.length : tagMatch[0].length),
    };

    const tagEnd = source.slice(endTagOffset, nextOffsetEnd);
    const tagEndMatch = tagEnd.matchAll(BLOCK_END_TAG_REGEX).next().value;
    const tagCloseIndex = endTagOffset + (tagEndMatch?.index ?? 0);

    const endTagPos: BlockPosition = {
      start: tagCloseIndex,
      end: tagCloseIndex + (tagEndMatch?.[0].length ?? 0),
    };

    const contentStartIndex = tagStartIndex + `<${tag}`.length;

    blocks.push({
      block,

      tag: {
        type: tag,
        content,
        pos: {
          open: startTagPos,
          close: endTagPos,
          content: {
            start: contentStartIndex,
            end: contentStartIndex + content.length,
          },
        },
      },
    });

    const nextBlock = sfcBlocks.at(i + 1);
    lastEnding = endTagPos.end;
    if (nextBlock) {
      const strToNextTag = source.slice(lastEnding, nextBlock.loc.start.offset);
      // if blocks are commented between other blocks this tagInit will catch that
      // we need to remove it
      const commentedBlocks = Array.from(
        strToNextTag.matchAll(COMMENTED_BLOCKS_REGEX)
      );
      // const firstCommentIndex = commentedBlocks.at(0)?.index ?? 0
      const lastComment = commentedBlocks.at(-1);
      if (lastComment) {
        lastEnding += lastComment.index + lastComment[0].length;
      }
    }
  }
  return blocks;
}
export function findBlockLanguage(block: VerterSFCBlock) {
  const langAttr = block.block.attrs.lang;
  const lang = langAttr === true ? undefined : langAttr?.toString();

  if (block.tag.type === "script") {
    return lang || "js";
  }
  if (block.tag.type === "template") {
    return lang || "vue";
  }
  if (block.tag.type === "style") {
    return lang || "css";
  }
  // TODO maybe add some way to dynamically get the language
  // for example for router
  return lang || block.tag.type;
}

export function keepBlocks(
  allBlocks: VerterSFCBlock[],
  keepBlocks: string[],
  s: MagicString
) {
  // remove unknown blocks
  const toKeep = new Set(keepBlocks);
  const blocks = [] as VerterSFCBlock[];
  for (const block of allBlocks) {
    if (toKeep.has(block.tag.type)) {
      blocks.push(block);
    } else {
      s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
    }
  }
  return blocks;
}

export function removeBlockTag(block: VerterSFCBlock, s: MagicString) {
  s.remove(block.tag.pos.open.start, block.tag.pos.open.end);
  s.remove(block.tag.pos.close.start, block.tag.pos.close.end);
}
