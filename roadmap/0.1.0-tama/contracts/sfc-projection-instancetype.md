# Vue projection InstanceType contract

Companion to `sfc-typescript-projection.md`. STP0 source contract for the generated Vue default export.

## Constructor shape

The generated Vue SFC default export is a constructable component. Public instance access is `InstanceType<typeof Comp>` and, for generics, `InstanceType<typeof Comp<...>>`.

This matches the shipped `packages/types` constructor extract (`GetVueComponent` construct signature with `$props`) and the documented generic-component example.

## RequiredCurrent row

`instancetype-typeof-comp` is RequiredCurrent. Removing it, marking it optional, or moving it to an external/untyped lane is rejected (`STP0-required-current`).

## Forbidden replacements

- Public callable replacement for the SFC constructor.
- Broad `any` constructor that makes `InstanceType` meaningless.
- Vacuous `never` success in place of a failed construct.
- Synthetic runtime property used only to hold checker metadata (including a fake instance field that exists solely so the checker can recover props).

## Production owner

STP0 ratifies the shape. STP16 produces the public constructor/instance. STP8 ratifies the ABI against evidence. STP0 does not emit constructor code.
