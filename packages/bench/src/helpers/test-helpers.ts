import { URI } from "vscode-uri";
import { expect } from "vitest";

export interface Position {
  line: number;
  character: number;
}

export interface CompletionItem {
  label: string;
  kind?: any;
  sortText?: string;
  data?: any;
}

export interface CompletionList {
  items: CompletionItem[];
  isIncomplete?: boolean;
}

/**
 * Parse content with cursor marker '|' and return position and cleaned content
 */
export function parseContentWithCursor(content: string): {
  content: string;
  position: Position;
  offset: number;
} {
  const offset = content.indexOf("|");
  expect(offset).toBeGreaterThanOrEqual(0);

  const cleanContent = content.slice(0, offset) + content.slice(offset + 1);
  const beforeCursor = content.slice(0, offset);

  const position: Position = {
    line: beforeCursor.split("\n").length - 1,
    character: beforeCursor.split("\n").pop()!.length,
  };

  return { content: cleanContent, position, offset };
}

/**
 * Create file URI for test workspace
 */
export function createTestUri(
  testWorkspacePath: string,
  fileName: string
): string {
  return URI.file(`${testWorkspacePath}/${fileName}`).toString();
}

/**
 * Find completion item by label
 */
export function findCompletionItem(
  completions: CompletionList | any,
  label: string
): CompletionItem | undefined {
  const items = completions.items || completions;
  return items.find((item: any) => item.label === label || item.name === label);
}

/**
 * Assert completion item exists
 */
export function assertCompletionExists(
  completions: CompletionList | any,
  label: string
): CompletionItem {
  const item = findCompletionItem(completions, label);
  expect(item, `Expected completion "${label}" to exist`).toBeDefined();
  return item!;
}

/**
 * Get completion labels
 */
export function getCompletionLabels(completions: CompletionList | any): string[] {
  const items = completions.items || completions;
  return items.map((item: any) => item.label || item.name);
}
