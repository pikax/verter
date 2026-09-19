import type { PatchHidden, PartialUndefined } from "@verter/types";

type Props = { name: string; label: string | undefined };
export type PublicProps = PartialUndefined<Props>;
export type WithMeta = PatchHidden<PublicProps, { source: "reference" }>;
