import {
  // core entry-points for each language flavor:
  getCSSLanguageService,
  getSCSSLanguageService,
  getLESSLanguageService,
  getDefaultCSSDataProvider,
  LanguageService,
} from "vscode-css-languageservice";
import { normalisePath } from "../documents";

const cssServices = new Map<
  string,
  {
    css: LanguageService;
    scss: LanguageService;
    less: LanguageService;
  }
>();

export function getStyleLanguageService(
  uri: string,
  languageId: "css" | "scss" | "less" = "css"
): LanguageService {
  const root = normalisePath(uri);
  let entry = cssServices.get(root);
  if (!entry) {
    entry = {
      css: getCSSLanguageService(),
      scss: getSCSSLanguageService(),
      less: getLESSLanguageService(),
    };
    cssServices.set(root, entry);
  }

  switch (languageId) {
    case "css": {
      return entry.css;
    }
    case "less": {
      return entry.less;
    }
    case "scss": {
      return entry.scss;
    }
    default: {
      console.warn(
        `Language ${languageId} not recognised as a supported style`
      );
      return entry.css;
    }
  }
}
