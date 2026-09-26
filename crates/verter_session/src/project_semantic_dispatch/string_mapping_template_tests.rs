//! Template literal types and the intrinsic string mappings over the
//! operand they denote where written. The checker settles an alias, a
//! `keyof` whose keys are known (`keyof Box<string>` is `"value" | "size"`)
//! and an intersection (`keyof T & string`) before it distributes a template
//! over each union among its holes (`getTemplateLiteralType`) or maps each
//! constituent (`getStringMappingType`): a string literal maps its text,
//! `string` / `number` / `bigint` / `any` stay holes, `number` maps through
//! its pattern template, and a template literal type maps its texts and
//! holes. `keyof` over a `string` index signature is `string | number`.
//!
//! Every expected answer is TypeScript 7.0.2's, read off the TS2322 message
//! of `declare const p: <probe>; export const s: null = p;` under each
//! `strictNullChecks` × `noImplicitAny` setting; the four settings agree.

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
interface Box<T> { value: T; size: number }
interface Plain { a: 1; b: 2 }
type K = \"a\" | \"b\";
type Rec = { [k: string]: number };
type NumRec = { [k: number]: number };
type Mixed = { a: 1; 0: 2 };
type Sym = { [k: symbol]: 1; a: 2 };
type G<T> = `get${Capitalize<keyof T & string>}`;
";

/// A template or a string mapping over a settled key union distributes
/// over its keys.
///
/// Measured on TypeScript 7.0.2:
///
/// | probe | checker |
/// | --- | --- |
/// | `` `get${Capitalize<keyof Box<string>>}` `` | `"getSize" \| "getValue"` |
/// | `Uppercase<keyof Plain>`, `Uppercase<K>` | `"A" \| "B"` |
/// | `` `on${K}` ``, `` `on${keyof Plain}` `` | `"ona" \| "onb"` |
/// | `` `on${keyof Box<string>}` `` | `"onsize" \| "onvalue"` |
/// | `` `k${keyof Mixed}` `` | `"k0" \| "ka"` |
/// | `` `k${keyof Sym & string}` `` | `"ka"` |
/// | ``Lowercase<`A${keyof Plain}`>`` | `"aa" \| "ab"` |
/// | `` `${keyof Plain}-${keyof Plain}` `` | `"a-a" \| "a-b" \| "b-a" \| "b-b"` |
/// | `G<Plain>` | `"getA" \| "getB"` |
/// | `` `k${boolean}` `` | `"kfalse" \| "ktrue"` |
/// | `` `k${null}${undefined}` `` | `"knullundefined"` |
#[test]
fn a_template_and_a_string_mapping_distribute_over_settled_keys() {
    let failures = mismatches(
        FIXTURE,
        &[
            (
                "`get${Capitalize<keyof Box<string>>}`",
                "\"getSize\" | \"getValue\"",
            ),
            ("Uppercase<keyof Plain>", "\"A\" | \"B\""),
            ("Uppercase<K>", "\"A\" | \"B\""),
            ("`on${K}`", "\"ona\" | \"onb\""),
            ("`on${keyof Plain}`", "\"ona\" | \"onb\""),
            ("`on${keyof Box<string>}`", "\"onsize\" | \"onvalue\""),
            ("`k${keyof Mixed}`", "\"k0\" | \"ka\""),
            ("`k${keyof Sym & string}`", "\"ka\""),
            ("Lowercase<`A${keyof Plain}`>", "\"aa\" | \"ab\""),
            (
                "`${keyof Plain}-${keyof Plain}`",
                "\"a-a\" | \"a-b\" | \"b-a\" | \"b-b\"",
            ),
            ("G<Plain>", "\"getA\" | \"getB\""),
            ("`k${boolean}`", "\"kfalse\" | \"ktrue\""),
            ("`k${null}${undefined}`", "\"knullundefined\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `string`, `number`, `bigint` and `any` stay holes of a template literal
/// type, a string mapping keeps its application over them, and a mapping
/// over a template literal type maps its texts and holes.
///
/// Measured on TypeScript 7.0.2:
///
/// | probe | checker |
/// | --- | --- |
/// | `` `k${keyof Rec}` `` | `` `k${string}` \| `k${number}` `` |
/// | `` `k${keyof NumRec}` `` | `` `k${number}` `` |
/// | `` `k${keyof Rec}x` `` | `` `k${string}x` \| `k${number}x` `` |
/// | `Uppercase<keyof Rec>` | ``Uppercase<string> \| Uppercase<`${number}`>`` |
/// | `Lowercase<keyof Rec>` | ``Lowercase<string> \| Lowercase<`${number}`>`` |
/// | `` `${string}` `` | `string` |
/// | `` `${number}` ``, `` `${bigint}` `` | `` `${number}` ``, `` `${bigint}` `` |
/// | ``Uppercase<`k${string}`>`` | ``` `K${Uppercase<string>}` ``` |
/// | ``Capitalize<`${string}x`>`` | ``` `${Capitalize<string>}x` ``` |
/// | ``Capitalize<`ab${string}`>`` | `` `Ab${string}` `` |
/// | `` `a${`b${string}`}c` `` | `` `ab${string}c` `` |
/// | `` `${Uppercase<string>}` ``, `Uppercase<Uppercase<string>>` | `Uppercase<string>` |
/// | ``Uppercase<`${number}`>`` | ``Uppercase<`${number}`>`` |
/// | `Uppercase<any>` | `Uppercase<any>` |
/// | `` `k${any}` `` | `` `k${any}` `` |
#[test]
fn a_template_keeps_string_number_and_any_holes() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("`k${keyof Rec}`", "`k${string}` | `k${number}`"),
            ("`k${keyof NumRec}`", "`k${number}`"),
            ("`k${keyof Rec}x`", "`k${string}x` | `k${number}x`"),
            (
                "Uppercase<keyof Rec>",
                "Uppercase<string> | Uppercase<`${number}`>",
            ),
            (
                "Lowercase<keyof Rec>",
                "Lowercase<string> | Lowercase<`${number}`>",
            ),
            ("`${string}`", "string"),
            ("`${number}`", "`${number}`"),
            ("`${bigint}`", "`${bigint}`"),
            ("Uppercase<`k${string}`>", "`K${Uppercase<string>}`"),
            ("Capitalize<`${string}x`>", "`${Capitalize<string>}x`"),
            ("Capitalize<`ab${string}`>", "`Ab${string}`"),
            ("`a${`b${string}`}c`", "`ab${string}c`"),
            ("`${Uppercase<string>}`", "Uppercase<string>"),
            ("Uppercase<Uppercase<string>>", "Uppercase<string>"),
            ("Uppercase<`${number}`>", "Uppercase<`${number}`>"),
            ("Uppercase<any>", "Uppercase<any>"),
            ("`k${any}`", "`k${any}`"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keyof` over an index signature reads its key type — a `string` one
/// `string | number` — beside the members' literal keys, which a `string`
/// key absorbs. A key list of one `string` index signature prints as the
/// `string | number` union; any other keeps the `keyof` origin.
///
/// Measured on TypeScript 7.0.2: `keyof Rec` and `keyof I2` (`{ [k:
/// string]: 1 }`) print `string | number`; `keyof NumRec` prints `number`;
/// `keyof IRec` (`{ [k: string]: number; a: number }`), `keyof Sym`,
/// `keyof I3` (`string` and `symbol` indexes), `keyof T4` (`number` index
/// and `a`) and `keyof T5` (`` `x${string}` `` and `number` indexes) print
/// their `keyof` origin, and hold (`[A] extends [B] ? 1 : 2` is `1` both
/// ways) `string | number`, `symbol | "a"`, `string | number | symbol`
/// and `number | "a"` (`keyof T5` holds `` number | `x${string}` ``).
#[test]
fn a_keyof_over_an_index_signature_reads_its_key_type() {
    const INDEXED: &str = "\
type Rec = { [k: string]: number };
interface I2 { [k: string]: 1 }
interface IRec { [k: string]: number; a: number }
type NumRec = { [k: number]: number };
type Sym = { [k: symbol]: 1; a: 2 };
interface I3 { [k: string]: 1; [k: symbol]: 2 }
type T4 = { [k: number]: 1; a: 1 };
type T5 = { [k: `x${string}`]: 1; [k: number]: 2 };
";
    let mut failures = mismatches(
        INDEXED,
        &[
            ("keyof Rec", "string | number"),
            ("keyof I2", "string | number"),
            ("keyof NumRec", "number"),
            ("keyof IRec", "keyof IRec"),
            ("keyof Sym", "keyof Sym"),
            ("keyof I3", "keyof I3"),
            ("keyof T4", "keyof T4"),
            ("keyof T5", "keyof T5"),
        ],
    );
    failures.extend(mismatches(
        INDEXED,
        &[
            ("[keyof IRec] extends [string | number] ? 1 : 2", "1"),
            ("[string | number] extends [keyof IRec] ? 1 : 2", "1"),
            ("[keyof Sym] extends [symbol | \"a\"] ? 1 : 2", "1"),
            ("[symbol | \"a\"] extends [keyof Sym] ? 1 : 2", "1"),
            ("[keyof I3] extends [string | number | symbol] ? 1 : 2", "1"),
            ("[string | number | symbol] extends [keyof I3] ? 1 : 2", "1"),
            ("[keyof T4] extends [number | \"a\"] ? 1 : 2", "1"),
            ("[number | \"a\"] extends [keyof T4] ? 1 : 2", "1"),
        ],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
