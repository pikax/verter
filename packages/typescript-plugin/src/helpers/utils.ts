const DEFAULT_REGEXP = /\.vue$/;
const VUE_TS_REGEXP = /\.vue\.ts$/;
const RELATIVE_REGEXP = /^\.\.?($|[\\/])/;

const isRelative = (fileName: string) => RELATIVE_REGEXP.test(fileName);

export const isVue = (fileName: string) => DEFAULT_REGEXP.test(fileName);
export const isRelativeVue = (fileName: string) => isVue(fileName) && isRelative(fileName);

export const isVueTs = (fileName: string) => VUE_TS_REGEXP.test(fileName);
export const isRelativeVueTs = (fileName: string) => isVueTs(fileName) && isRelative(fileName);
