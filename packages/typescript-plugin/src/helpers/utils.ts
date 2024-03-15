const DEFAULT_REGEXP = /\.vue$/;
const RELATIVE_REGEXP = /^\.\.?($|[\\/])/;

const isRelative = (fileName: string) => RELATIVE_REGEXP.test(fileName);

export const isVue = (fileName: string) => DEFAULT_REGEXP.test(fileName);
export const isRelativeVue = (fileName: string) =>
  isVue(fileName) && isRelative(fileName);
