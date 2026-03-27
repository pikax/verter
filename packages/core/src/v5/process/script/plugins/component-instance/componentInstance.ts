import { definePlugin } from "../../types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import { createHelperImport } from "../../../utils";
import { type AvailableExports } from "@verter/types/string";
import { ProcessItemType } from "../../../types";

export const ComponentInstancePlugin = definePlugin({
  name: "VerterComponentInstance",
  enforce: "post",

  pre(s, ctx) {
    if (ctx.isSetup) {
      ctx.items.push(
        createHelperImport(
          [
            "PublicInstanceFromMacro",
            "ExtractComponentProps",
            "OmitConstructorSignature",
            "Prettify",
          ],
          ctx.prefix,
        ),
      );
    } else {
      ctx.items.push(createHelperImport(["OmitConstructorSignature", "Prettify"], ctx.prefix));
    }
  },

  post(s, ctx) {
    if (ctx.isSetup) {
      const macroToInstance = ctx.prefix("PublicInstanceFromMacro" as AvailableExports);
      const attributes = ctx.prefix("attributes");
      // TODO resolve the first element in template and use its type
      // NOTE that if inheritedAttrs is false, then it should be {}
      // if allowDev is true it will export a type to be imported in test components
      const allowDev = true;

      const noInheritAttrs =
        ctx.items.find((i) => i.type === ProcessItemType.InheritAttrs)?.value === false;
      const inheritAttrs = noInheritAttrs ? "false" : "true";

      const componentName = ctx.prefix("Component");
      const templateBinding = ctx.prefix("TemplateBinding");
      const RootElement = ctx.prefix("RootElement");
      const defaultOptionsName = ctx.prefix("default_Component");
      const getRootComponentName = ctx.prefix("getRootComponent");
      const ExtractComponentProps = ctx.prefix("ExtractComponentProps");
      const getRootComponentPassedProps = ctx.prefix("getRootComponentPassedProps");
      const OmitConstructorSignature = ctx.prefix("OmitConstructorSignature" as AvailableExports);
      const Prettify = ctx.prefix("Prettify" as AvailableExports);

      const exportStr = ctx.isSingleFile ? "" : "export";

      const genericDeclaration = ctx.generic ? `<${ctx.generic.declaration}>` : "";
      const sanitisedNames = ctx.generic ? `<${ctx.generic.sanitisedNames.join(",")}>` : "";

      const instanceName = ctx.prefix("Instance");

      const publicConstructor = `new${genericDeclaration}(props?: ${instanceName}${sanitisedNames}['$props']): ${Prettify}<${instanceName}${sanitisedNames}>`;

      const rootElementStr = `type ${RootElement}${
        ctx.generic ? `<${ctx.generic.source}>` : ""
      }=ReturnType<typeof ${getRootComponentName}${
        ctx.generic ? `<${ctx.generic.names.join(",")}>` : ""
      }>`;
      const RootElementProps = `${RootElement}Props`;
      const RootElementPropsStr = `type ${RootElementProps}${
        ctx.generic ? `<${ctx.generic.source}>` : ""
      }=${Prettify}<Omit<${ExtractComponentProps}<${RootElement}${
        ctx.generic ? `<${ctx.generic.names.join(",")}>` : ""
      }>,keyof ReturnType<typeof ${getRootComponentPassedProps}${
        ctx.generic ? `<${ctx.generic.names.join(",")}>` : ""
      }>>>`;
      const PatchedInstanceKeys = [
        "$",
        "$data",
        "$props",
        "$attrs",
        "$refs",
        "$options",
        "$emit",
        "$el",
        "$slots",
      ]
        .map((x) => `"${x}"`)
        .join("|");

      const declaration = [
        rootElementStr,
        RootElementPropsStr,
        `${exportStr} type ${instanceName}${genericDeclaration} = Omit<InstanceType<typeof ${defaultOptionsName}>,${PatchedInstanceKeys}> & ${macroToInstance}<${templateBinding}${sanitisedNames},{}&${attributes}${
          noInheritAttrs
            ? ""
            : "&" +
              RootElementProps +
              (ctx.generic ? `<${ctx.generic.sanitisedNames.join(",")}>` : "")
        },${RootElement}, false,true>;`,
        allowDev &&
          `${exportStr} type ${instanceName}_TEST${genericDeclaration} = Omit<InstanceType<typeof ${defaultOptionsName}>,${PatchedInstanceKeys}> & ${macroToInstance}<${templateBinding}${sanitisedNames},{}&${attributes}${
            noInheritAttrs ? "" : "&" + RootElementProps
          },${RootElement}, true,true>;`,
        `${exportStr} declare const ${componentName}: ${OmitConstructorSignature}<typeof ${defaultOptionsName}> & {${publicConstructor}};`,
        ctx.isSingleFile ? `export default ${componentName};` : "",
      ];

      s.append(declaration.filter(Boolean).join("\n"));
    } else {
      const defaultOptionsName = ctx.prefix("default_Component");
      const instanceName = ctx.prefix("Instance");
      const componentName = ctx.prefix("Component");
      const OmitConstructorSignature = ctx.prefix("OmitConstructorSignature" as AvailableExports);
      const Prettify = ctx.prefix("Prettify" as AvailableExports);
      const exportStr = ctx.isSingleFile ? "" : "export";

      const publicConstructor = `new(props?: ${instanceName}['$props']): ${Prettify}<${instanceName}>`;

      const declaration = [
        `${exportStr} type ${instanceName} = InstanceType<typeof ${defaultOptionsName}>;`,
        `${exportStr} declare const ${componentName}: ${OmitConstructorSignature}<typeof ${defaultOptionsName}> & {${publicConstructor}};`,
        ctx.isSingleFile ? `export default ${componentName};` : "",
      ];

      s.append(declaration.filter(Boolean).join("\n"));
    }
  },
});
