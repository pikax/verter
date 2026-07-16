/**
 * @ts-expect-error negatives for Svelte Snippet parameter shapes.
 */
import type { Snippet } from "svelte";
import SnippetTypedHost from "./SnippetTypedHost.svelte";

type HeaderArgs = [{ title: string; count: number }];
type RowArgs = [{ body: string; flag: boolean }];
type FooterArgs = [{ ok: boolean }];

// @ts-expect-error title is string
export const badHeaderArgs: HeaderArgs = [{ title: 1, count: 2 }];

// @ts-expect-error count is number
export const badHeaderCount: HeaderArgs = [{ title: "t", count: "n" }];

// @ts-expect-error body is string
export const badRowBody: RowArgs = [{ body: 1, flag: true }];

// @ts-expect-error flag is boolean
export const badRowFlag: RowArgs = [{ body: "b", flag: "yes" }];

// @ts-expect-error ok is boolean
export const badFooter: FooterArgs = [{ ok: "no" }];

type HeaderSnippet = Snippet<HeaderArgs>;

// @ts-expect-error Snippet parameter tuple must match HeaderArgs
export const badSnippet: HeaderSnippet = ((p: { title: number; count: string }) => {
  void p;
}) as HeaderSnippet;

void SnippetTypedHost;
void badHeaderArgs;
void badHeaderCount;
void badRowBody;
void badRowFlag;
void badFooter;
void badSnippet;
