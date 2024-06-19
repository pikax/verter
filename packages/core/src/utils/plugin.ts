import { Statement } from "@babel/types";
import {
  LocationByType,
  ParseScriptContext,
  PluginOption,
  TypeLocation,
  WalkResult,
} from "../plugins";

export function* walkPlugins(
  plugins: PluginOption[],
  context: ParseScriptContext
) {
  yield* runPlugins((plugin) => plugin.walk, plugins, context);
}
export function* processPlugins(
  plugins: PluginOption[],
  context: ParseScriptContext
) {
  for (const plugin of plugins) {
    if (!plugin.process) continue;
    const result = plugin.process(context);
    if (!result) continue;
    if (Array.isArray(result)) {
      yield* result;
    } else {
      yield result;
    }
  }
}

export function pluginsToLocations(
  plugins: PluginOption[],
  context: ParseScriptContext
): LocationByType {
  const items = [
    ...processPlugins(plugins, context),
    ...walkPlugins(plugins, context),
  ];

  const locations = {} as LocationByType;

  function handleResult(locations: LocationByType, result: WalkResult) {
    if (!result) return;

    if (Array.isArray(result)) {
      result.forEach((x) => handleResult(locations, x));
      return;
    }
    if (!locations[result.type]) {
      locations[result.type] = [];
    }
    // is quite hard to infer the correct type
    const arr = locations[result.type] as TypeLocation[];
    arr.push(result);
  }

  handleResult(locations, items);



  return locations;
}

export function* runPlugins(
  cb: (
    plugin: PluginOption
  ) =>
    | undefined
    | ((state: Statement, context: ParseScriptContext) => WalkResult),
  plugins: PluginOption[],
  context: ParseScriptContext
) {
  if (!context.script) return;
  for (const [isSetup, ast] of [
    [false, context.script.scriptAst],
    [true, context.script.scriptSetupAst],
  ] as const) {
    if (!ast) continue;
    const ctx = {
      ...context,
      isSetup,
      // source: isSetup
    };
    for (const statement of ast) {
      for (const plugin of plugins) {
        const result = cb(plugin)?.(statement, ctx);
        if (!result) continue;
        if (Array.isArray(result)) {
          for (const it of result) {
            if (it) {
              // TODO move this somewhere else, we might

              if (!Array.isArray(it)) {
                it.isSetup = isSetup;
              }
              yield it;
            }
          }

          // // TODO move this somewhere else
          // result.forEach((x) => x.isSetup = isSetup)
          // yield* result;
        } else {
          // TODO move this somewhere else
          result.isSetup = isSetup;
          yield result;
        }
      }
    }
  }
}
