export const AI_GENERATED_FOOTER = "AI-Generated";

const MODEL_LINE = /^Model:\s*/u;
const FOOTER_LINE = /^AI-Generated\s*$/u;
const EFFORT_FIELD =
  /^(?:implementation_|review_|verification_|confirmation_)?effort(?:_(?:min|default))?\s*[:=]/iu;

export function countAiGeneratedFooters(body) {
  if (typeof body !== "string" || body.length === 0) return 0;
  return body.split(/\r?\n/u).filter((line) => FOOTER_LINE.test(line)).length;
}

export function ensureAiGeneratedFooter(body) {
  const original = typeof body === "string" ? body : "";
  const originalLines = original.split(/\r?\n/u);
  const meaningful = [...originalLines];
  while (meaningful.length > 0 && meaningful.at(-1) === "") meaningful.pop();
  const alreadyCompliant =
    meaningful.at(-1) === AI_GENERATED_FOOTER &&
    originalLines.filter((line) => line === AI_GENERATED_FOOTER).length === 1 &&
    !originalLines.some((line) => MODEL_LINE.test(line) || EFFORT_FIELD.test(line));
  if (alreadyCompliant) return original;

  const lines = original.replaceAll("\r\n", "\n").split("\n");
  const kept = lines.filter(
    (line) => !MODEL_LINE.test(line) && !FOOTER_LINE.test(line) && !EFFORT_FIELD.test(line),
  );
  while (kept.length > 0 && kept[kept.length - 1].trim() === "") kept.pop();
  kept.push("", AI_GENERATED_FOOTER);
  return `${kept.join("\n")}\n`;
}
