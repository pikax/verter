//! Abstract construct signatures: `abstract new () => T` and an abstract
//! class's construct signatures carry the checker's `SignatureFlags.Abstract`,
//! and an abstract constructor type is not assignable to a non-abstract one.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `export const a: null = null! as <probe>;` read off the
//! TS2322 message (`tsc --noEmit --strict --ignoreConfig`), the same under
//! all four `strictNullChecks` × `noImplicitAny` settings.

use super::checker_probe_lane_tests::mismatches;

const SOURCE: &str = "\
export abstract class AB { x = 1; }
export class CC extends AB { }
export abstract class AD extends CC { }
export class CE extends AD { }
export type AC = abstract new () => { x: number };
export type NC = new () => { x: number };
";

/// Measured on TypeScript 7.0.2 (all four settings): `typeof AB extends
/// new () => any` is `0` and `typeof AB extends abstract new () => any`
/// `1`; a concrete class extending an abstract one (`typeof CC extends new
/// () => AB`, `typeof CE extends …`) is `1`, an abstract class extending a
/// concrete one (`typeof AD extends …`) `0`; `NC extends AC` is `1`, `AC
/// extends NC` `0`, `{ new (): { x: number } } extends AC` `1`, `AC extends
/// { new (): { x: number } }` `0`, `AC extends Function` `1`; `typeof AD
/// extends typeof AB` is `1`, `typeof AB extends typeof CC` `0` and `typeof
/// CC extends typeof AB` `1`; `InstanceType<AC>` is `{ x: number; }`,
/// `InstanceType<typeof AB>` `AB` and `ConstructorParameters<typeof AB>`
/// `[]`.
#[test]
fn an_abstract_constructor_is_not_a_concrete_one() {
    let failures = mismatches(
        SOURCE,
        &[
            ("typeof AB extends new () => any ? 1 : 0", "0"),
            ("typeof AB extends abstract new () => any ? 1 : 0", "1"),
            ("typeof CC extends new () => AB ? 1 : 0", "1"),
            ("typeof AD extends new () => AB ? 1 : 0", "0"),
            ("typeof CE extends new () => AB ? 1 : 0", "1"),
            ("NC extends AC ? 1 : 0", "1"),
            ("AC extends NC ? 1 : 0", "0"),
            ("{ new (): { x: number } } extends AC ? 1 : 0", "1"),
            ("AC extends { new (): { x: number } } ? 1 : 0", "0"),
            ("AC extends Function ? 1 : 0", "1"),
            ("typeof AD extends typeof AB ? 1 : 0", "1"),
            ("typeof AB extends typeof CC ? 1 : 0", "0"),
            ("typeof CC extends typeof AB ? 1 : 0", "1"),
            ("InstanceType<AC>", "{ x: number; }"),
            ("InstanceType<typeof AB>", "AB"),
            ("ConstructorParameters<typeof AB>['length']", "0"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
