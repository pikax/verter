import path from "node:path";

const windowsPath = (value) => /^[A-Za-z]:[\\/]|^\\\\|^\/\//.test(value);

/** Recorded Windows roots keep their dialect even when replayed on POSIX. */
export function resolveRoot(root) {
  return (windowsPath(root) ? path.win32 : path).resolve(root);
}

/** Make a printed path relative without depending on the host's path dialect. */
function relativeFile(file, root) {
  const paths = windowsPath(root) ? path.win32 : path.posix;
  const normalized = file.replaceAll("\\", "/");
  return paths.relative(root, paths.resolve(root, normalized)).replaceAll("\\", "/");
}

function relativeMessage(message, root) {
  // Quoted paths may contain spaces and punctuation. Keep their quote marks.
  return message.replace(
    /(['"`])((?:[A-Za-z]:[\\/]|\/|\\\\)[^\r\n]*?)\1|(?:[A-Za-z]:[\\/]|\\\\|\/)[^\s'"`,;:()<>\[\]{}]+/g,
    (match, quote, quotedPath, offset, text) => {
      if (quote) return `${quote}${relativeFile(quotedPath, root)}${quote}`;
      // A slash inside a word (including a URL) is not an absolute path.
      if (offset > 0 && /[\w/\\:]/.test(text[offset - 1])) return match;
      return relativeFile(match, root);
    },
  );
}

/**
 * Read the reporter / tsc --pretty false line format into comparison rows.
 * A diagnostic printed without a position (`file: error CODE: …`) has a null
 * line and column; a global one (`error CODE: …`) also has a null file.
 */
export function normalize(output, root) {
  const rows = [];
  for (const text of output.split(/\r?\n/)) {
    const match =
      /^(.*?)\((\d+),(\d+)\): (?:error|warning) ((?:TS|VTER)\d+): (.*)$/.exec(text) ??
      /^(?:(.*?): )?()()(?:error|warning) ((?:TS|VTER)\d+): (.*)$/.exec(text);
    if (!match) continue;
    const [, file, line, col, code, message] = match;
    rows.push({
      file: file === undefined ? null : relativeFile(file, root),
      line: line ? Number(line) : null,
      col: col ? Number(col) : null,
      code,
      messageHead: relativeMessage(message, root),
    });
  }
  return rows;
}
