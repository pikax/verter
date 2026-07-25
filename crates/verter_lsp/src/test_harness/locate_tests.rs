//! Tests for the structural template-tag locator in [`super`].
//!
//! Every case is about the same question: when the locator cannot be certain
//! which element the caller named, it must REFUSE rather than answer. The
//! refusals are asserted by variant, and each has a discriminating twin that
//! resolves, so a blanket failure could not pass for discrimination.

use super::*;

/// The shape this locator exists for: a tag whose attributes sit on their
/// own lines — ordinary authored style, and what a formatter emits once the
/// tag exceeds the print width — beside a SECOND tag of the same name.
const REFLOWED_SFC: &str = "<template>\n  \
    <div>\n    \
    <GlobalCountComp\n      :count=\"42\"\n      @ping=\"handler\"\n    />\n    \
    <global-count-comp :count=\"7\" />\n    \
    <GlobalCountComp :count=\"'mistyped'\" />\n  \
    </div>\n\
    </template>\n";

/// Byte offset of the first character of the `nth` `<GlobalCountComp` name.
fn pascal_name_start(nth: usize) -> usize {
    REFLOWED_SFC
        .match_indices("<GlobalCountComp")
        .nth(nth)
        .expect("the authored tag")
        .0
        + 1
}

#[test]
fn a_reflowed_tag_resolves_through_the_parse() {
    assert_eq!(
        REFLOWED_SFC.find("<GlobalCountComp :count=\"42\""),
        None,
        "precondition: the one-line byte string is NOT in the document, so a \
         byte search cannot answer this at all — which is exactly why the \
         locator is structural"
    );

    let offset = template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", (":count", "42"), 2)
        .expect("the parser reports the reflowed tag's span");
    assert_eq!(
        offset,
        pascal_name_start(0) + 2,
        "the offset must land inside the FIRST (reflowed) tag's name, measured \
         from the name's first byte"
    );
    assert!(
        REFLOWED_SFC[offset..].starts_with("obalCountComp\n"),
        "offset 2 must sit inside the reflowed tag name, got: {:?}",
        &REFLOWED_SFC[offset..offset + 14]
    );
}

#[test]
fn the_attribute_selects_among_identically_named_tags() {
    let first = template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", (":count", "42"), 0)
        .expect("the reflowed tag");
    let second =
        template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", (":count", "'mistyped'"), 0)
            .expect("the mistyped tag");

    assert_eq!(first, pascal_name_start(0));
    assert_eq!(second, pascal_name_start(1));
    assert!(
        first < second,
        "the two same-named tags must resolve to DIFFERENT elements — the \
         attribute is what selects, so a locator ignoring it would answer with \
         the same offset twice"
    );
}

#[test]
fn the_authored_tag_name_must_match_exactly() {
    let kebab = template_tag_name_offset(REFLOWED_SFC, "global-count-comp", (":count", "7"), 0)
        .expect("the kebab tag is its own element under its own authored name");
    assert_eq!(
        kebab,
        REFLOWED_SFC
            .find("<global-count-comp")
            .expect("the kebab tag")
            + 1,
    );
    assert_eq!(
        template_tag_name_offset(REFLOWED_SFC, "globalcountcomp", (":count", "42"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 0 }),
        "tag names are compared byte for byte — a case-folding locator would \
         silently address a DIFFERENT element than the author wrote"
    );
}

#[test]
fn an_attribute_value_no_element_carries_refuses() {
    assert_eq!(
        template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", (":count", "43"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 2 }),
        "two elements carry the tag name and neither carries this value, so the \
         lookup must refuse rather than settle for the nearest tag"
    );
    assert_eq!(
        template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", ("count", "42"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 2 }),
        "the attribute name is the AUTHORED one — `:count` is a v-bind and \
         `count` would be a static attribute; they are not interchangeable"
    );
}

#[test]
fn an_attribute_matching_several_elements_refuses() {
    let source = "<template>\n  <A :c=\"1\" />\n  <A\n    :c=\"1\" />\n</template>\n";
    assert_eq!(
        template_tag_name_offset(source, "A", (":c", "1"), 0),
        Err(TagLocateError::Ambiguous { matches: 2 }),
        "two elements answer to the same description, so there is no single \
         position to return and picking one would be a coin flip"
    );
}

/// The constructions that broke every byte-string tolerance: whitespace
/// inside an attribute value, an apostrophe, and operators a quote model
/// would have to reason about. None is modelled here — the parser reports
/// the value's span and the value is compared verbatim — so a value the
/// document does not carry simply does not match, and no position is
/// invented for it.
///
/// A BACKSLASH before a quote is deliberately absent. Verter's tokenizer and
/// the HTML attribute-value grammar disagree about where such a value ends,
/// and that disagreement is a question about the PARSER, not about this
/// locator. Asserting either answer here would turn a test of the locator
/// into a ruling on the parser, so the case is left unasserted until the
/// parser's behaviour is settled on its own terms.
#[test]
fn attribute_values_are_compared_verbatim_never_normalised() {
    let reflowed_value = "<template>\n  <Card\n    headline=\"hi\nthere\" />\n</template>\n";
    assert_eq!(
        template_tag_name_offset(reflowed_value, "Card", ("headline", "hi there"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 1 }),
        "the document's value contains a NEWLINE; a locator that softened it \
         would report a confident position for a value the document never held"
    );
    assert_eq!(
        template_tag_name_offset(reflowed_value, "Card", ("headline", "hi\nthere"), 0),
        Ok(reflowed_value.find("Card").expect("the tag")),
        "the value AS AUTHORED matches, newline and all — that whitespace is \
         data here, not formatting"
    );

    // An expression value carrying the characters a hand-rolled matcher has
    // to model — `=`, `<`, spaces — spread across a reflowed tag. Nothing
    // here is interpreted, so the space-spelled variant does not match and
    // the authored one does; the refusal is discrimination, not a blanket
    // failure.
    let expression_value = "<template>\n  <C\n    :m=\"a === b\n&& c < d\" />\n</template>\n";
    assert_eq!(
        template_tag_name_offset(expression_value, "C", (":m", "a === b && c < d"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 1 }),
        "the document's value holds a NEWLINE between `b` and `&&`; nothing \
         here softens it, so the space-spelled value does not match"
    );
    assert_eq!(
        template_tag_name_offset(expression_value, "C", (":m", "a === b\n&& c < d"), 0),
        Ok(expression_value.find('C').expect("the tag")),
        "the value as the document actually holds it does match"
    );

    let apostrophe = "<template>\n  <C\n    :msg=\"it's ok\" />\n</template>\n";
    assert_eq!(
        template_tag_name_offset(apostrophe, "C", (":msg", "it's ok"), 0),
        Ok(apostrophe.find('C').expect("the tag")),
        "an apostrophe inside a double-quoted value is ordinary text, and the \
         space around it is part of the value"
    );
}

#[test]
fn the_offset_addresses_a_character_inside_the_tag_name() {
    let start = pascal_name_start(0);
    let len = "GlobalCountComp".len();
    assert_eq!(
        template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", (":count", "42"), 0),
        Ok(start),
        "offset 0 is the `G`, not the `<`"
    );
    assert_eq!(
        template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", (":count", "42"), len - 1),
        Ok(start + len - 1),
        "the last addressable offset is the name's FINAL byte — the `p`"
    );
    assert!(
        REFLOWED_SFC[start + len - 1..].starts_with('p'),
        "precondition: the last addressable offset really is the final letter"
    );

    // `len` itself is the boundary just past the name. In this reflowed tag
    // the character there is the NEWLINE the formatter inserted — outside
    // the tag token, and outside the span a strict position mapper accepts.
    assert!(
        REFLOWED_SFC[start + len..].starts_with('\n'),
        "precondition: the byte at `len` is already the formatter's newline, \
         not part of the tag name"
    );
    for past in [len, len + 1] {
        assert_eq!(
            template_tag_name_offset(REFLOWED_SFC, "GlobalCountComp", (":count", "42"), past),
            Err(TagLocateError::OffsetPastTagName {
                name_len: len,
                name_offset: past,
            }),
            "offset {past} is outside the {len}-byte tag name and must refuse \
             rather than address whatever follows it"
        );
    }
}

#[test]
fn an_offset_splitting_a_multibyte_tag_character_refuses() {
    // `é` occupies bytes 3..5 of `Café`, so byte 4 is inside it and names no
    // character at all. Handing that to the position mapper would produce a
    // column derived from half a codepoint.
    let source = "<template>\n  <Café :c=\"1\" />\n</template>\n";
    let start = source.find("Café").expect("the tag");
    assert_eq!(
        template_tag_name_offset(source, "Café", (":c", "1"), 3),
        Ok(start + 3),
        "byte 3 begins the `é` and is a real character position"
    );
    assert_eq!(
        template_tag_name_offset(source, "Café", (":c", "1"), 4),
        Err(TagLocateError::OffsetSplitsCharacter {
            name_len: "Café".len(),
            name_offset: 4,
        }),
        "byte 4 is the `é`'s continuation byte — a locator that returned it \
         would hand a mid-codepoint offset to the position mapper"
    );
}

#[test]
fn a_source_with_no_template_refuses() {
    assert_eq!(
        template_tag_name_offset("<script>const a = 1;</script>\n", "A", (":c", "1"), 0),
        Err(TagLocateError::NoTemplate),
        "there is nothing to search, and that is a loud refusal rather than a \
         silent zero"
    );
}

/// A source the parser reports errors on has untrustworthy spans, so the
/// locator refuses instead of measuring against them.
#[test]
fn a_source_the_parser_rejects_refuses() {
    // `v-if` with no expression: the parser records a hard error and still
    // produces an element, so a locator that ignored the diagnostics would
    // happily answer from spans the parser itself does not stand behind.
    let malformed = "<template>\n  <A v-if :c=\"1\" />\n</template>\n";
    assert_eq!(
        template_tag_name_offset(malformed, "A", (":c", "1"), 0),
        Err(TagLocateError::SourceDidNotParse),
        "the parse reported errors, so the refusal comes BEFORE any span is \
         measured"
    );

    // The discriminator: the same shape WITH the expression parses, so the
    // refusal above is about the error, not about this tag shape.
    let well_formed = "<template>\n  <A v-if=\"ready\" :c=\"1\" />\n</template>\n";
    assert_eq!(
        template_tag_name_offset(well_formed, "A", (":c", "1"), 0),
        Ok(well_formed.find('A').expect("the tag")),
        "the well-formed twin resolves — the refusal is discrimination, not a \
         blanket failure"
    );
}

/// Modifiers are part of the authored name. Dropping them makes `@click` and
/// `@click.stop` the same selector: the shorter one then matches an element
/// whose source never contains it, and both elements answer to it at once.
#[test]
fn directive_modifiers_belong_to_the_authored_name() {
    let source = "<template>\n  <A @click.stop=\"go\" />\n  <A @click=\"go\" />\n</template>\n";
    let with_modifier = source.find("<A @click.stop").expect("the modified tag") + 1;
    let without_modifier = source.find("<A @click=").expect("the plain tag") + 1;

    assert_eq!(
        template_tag_name_offset(source, "A", ("@click.stop", "go"), 0),
        Ok(with_modifier),
        "the full authored name selects the element that carries the modifier"
    );
    assert_eq!(
        template_tag_name_offset(source, "A", ("@click", "go"), 0),
        Ok(without_modifier),
        "`@click` names the element authored WITHOUT the modifier — a locator \
         that stripped modifiers would find both and could not tell them apart"
    );
    assert_ne!(
        with_modifier, without_modifier,
        "precondition: the two selectors must resolve to different elements"
    );
    assert_eq!(
        template_tag_name_offset(source, "A", ("@click.prevent", "go"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 2 }),
        "a modifier no element carries refuses rather than falling back to the \
         bare event name"
    );
}

/// `v-if`, `v-for`, `v-slot`, `v-once` and `ref` never appear in
/// `ElementNode::props` — the parser lifts them into its own fields. A locator
/// reading only `props` answers `NoMatch` for every one of them, which is
/// exactly the silent-miss this locator exists to remove.
///
/// ALL FIVE cached slots are exercised here, each by a selector no other
/// element in the source answers, so deleting any single `.chain(...)` from
/// `authored_props` turns this test red on exactly that slot. `v-once` is
/// authored WITH a value on purpose: the parser caches the whole `NodeProp`
/// without rejecting one, and a value-less `v-once` carries no value span and
/// so cannot discriminate a `(name, value)` selector at all — which is why the
/// value-less case is a separate REFUSAL test and cannot stand in for this one.
#[test]
fn parser_cached_directives_are_searched_too() {
    let source = "<template>\n  \
        <A v-if=\"ready\" />\n  \
        <A v-for=\"x in xs\" />\n  \
        <A ref=\"anchor\" />\n  \
        <A v-once=\"pinned\" />\n  \
        <B><template v-slot:body=\"slotProps\">x</template></B>\n\
        </template>\n";

    for (selector, needle) in [
        (("v-if", "ready"), "<A v-if"),
        (("v-for", "x in xs"), "<A v-for"),
        (("ref", "anchor"), "<A ref"),
        (("v-once", "pinned"), "<A v-once"),
    ] {
        assert_eq!(
            template_tag_name_offset(source, "A", selector, 0),
            Ok(source.find(needle).expect("the authored tag") + 1),
            "`{}` is cached off `props` by the parser; the locator must still \
             see it",
            selector.0
        );
    }

    assert_eq!(
        template_tag_name_offset(source, "template", ("v-slot:body", "slotProps"), 0),
        Ok(source.find("<template v-slot").expect("the slot tag") + 1),
        "v-slot is cached too, and its authored name carries the slot argument"
    );

    assert_eq!(
        template_tag_name_offset(source, "A", ("v-if", "notReady"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 4 }),
        "seeing the cached directive must not mean matching any value — a \
         value no element carries still refuses"
    );
}

/// A cached directive slot holds ONE prop. A second `v-for` on the same
/// element is dropped with a warning, so the element that authored the
/// selector vanishes from the inventory the locator searches — and the
/// remaining element then looks like the unique answer. It is not: two
/// elements carry `v-for="c in d"`, so no single position is correct.
#[test]
fn a_dropped_duplicate_directive_refuses_rather_than_answering_uniquely() {
    let duplicated = "<template>\n  \
        <A v-for=\"a in b\" v-for=\"c in d\" />\n  \
        <A v-for=\"c in d\" />\n\
        </template>\n";
    let survivor = duplicated
        .find("<A v-for=\"c in d\"")
        .expect("the element whose only v-for is the duplicated value")
        + 1;

    assert_ne!(
        template_tag_name_offset(duplicated, "A", ("v-for", "c in d"), 0),
        Ok(survivor),
        "BOTH elements author `v-for=\"c in d\"`; answering with the survivor's \
         offset would point every downstream assertion at the one element the \
         author did NOT single out"
    );
    assert_eq!(
        template_tag_name_offset(duplicated, "A", ("v-for", "c in d"), 0),
        Err(TagLocateError::AuthoredDirectiveDropped { dropped: 1 }),
        "the parse threw an authored directive away, so its element inventory \
         is not the authored inventory and the lookup must refuse"
    );

    // The discriminator: the same two elements, same selector, minus the
    // duplicate. Now exactly one element carries it and the lookup answers.
    let single = "<template>\n  \
        <A v-for=\"a in b\" />\n  \
        <A v-for=\"c in d\" />\n\
        </template>\n";
    assert_eq!(
        template_tag_name_offset(single, "A", ("v-for", "c in d"), 0),
        Ok(single
            .find("<A v-for=\"c in d\"")
            .expect("the second element")
            + 1),
        "the refusal above is about the DROPPED directive, not about this \
         shape — without the duplicate the same selector resolves"
    );

    // The other cached field with the same one-slot shape is `ref`, and it
    // loses its second value through the ERROR channel instead (duplicate
    // ATTRIBUTE, not duplicate directive). Both channels must refuse; only
    // the reason differs.
    let duplicate_ref = "<template>\n  \
        <A ref=\"first\" ref=\"second\" />\n  \
        <A ref=\"second\" />\n\
        </template>\n";
    assert_eq!(
        template_tag_name_offset(duplicate_ref, "A", ("ref", "second"), 0),
        Err(TagLocateError::SourceDidNotParse),
        "the parser calls a repeated static attribute an ERROR, so this loss \
         is already refused — but it must be REFUSED, never answered with the \
         surviving element"
    );
}

/// A value-less attribute has no value span, so no `(name, value)` selector
/// can name it — not even the empty value. Matching on the name alone would
/// let `("v-once", "anything")` address an element whose source pairs that
/// name with nothing at all.
#[test]
fn a_value_less_cached_directive_answers_no_selector() {
    let source = "<template>\n  \
        <A v-once :c=\"1\" />\n  \
        <A :c=\"2\" />\n\
        </template>\n";
    let once_element = source.find("<A v-once").expect("the v-once element") + 1;

    assert_eq!(
        template_tag_name_offset(source, "A", ("v-once", ""), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 2 }),
        "`v-once` is authored with NO value; an empty-string selector must not \
         stand in for the absent value span"
    );
    assert_eq!(
        template_tag_name_offset(source, "A", ("v-once", "1"), 0),
        Err(TagLocateError::NoMatch { tag_occurrences: 2 }),
        "and a value-carrying selector must not match on the name alone — the \
         neighbouring `:c=\"1\"` value belongs to a different attribute"
    );
    assert_eq!(
        template_tag_name_offset(source, "A", (":c", "1"), 0),
        Ok(once_element),
        "the SAME element is reachable through its valued attribute, so the \
         refusals above are about the missing value span, not an invisible \
         element"
    );
}
