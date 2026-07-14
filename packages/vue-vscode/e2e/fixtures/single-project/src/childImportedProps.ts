// Imported prop type for the cross-file Vue-prop rename PARITY case
// (`defineProps<ImportedType>()`): the real prop DECLARATION lives in THIS THIRD
// file, not the child `.vue` macro. A cross-file rename of `headline` from the
// parent usage must edit the member declaration HERE.
export interface ChildImportedProps {
  headline: string;
  subtitle?: string;
}
