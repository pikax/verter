use super::*;

mod children_flag_tests {
    use super::*;

    #[test]
    fn empty_flag() {
        let f = ChildrenFlag::empty();
        assert_eq!(f.0, 0);
        assert!(!f.has_children());
        assert!(!f.is_text_only());
        assert!(!f.has_dynamic());
        assert!(!f.needs_array());
    }

    #[test]
    fn build_flags_fluent() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation);

        assert!(f.has(ChildrenFlags::HasText));
        assert!(f.has(ChildrenFlags::HasInterpolation));
        assert!(!f.has(ChildrenFlags::HasElement));
    }

    #[test]
    fn text_only() {
        let f = ChildrenFlags::HasText.into_flag();
        assert!(f.is_text_only());
        assert!(f.has_children());
        assert!(!f.needs_array());
        assert!(!f.has_dynamic());
    }

    #[test]
    fn text_with_interpolation() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation);
        assert!(f.is_text_only());
        assert!(f.has_dynamic());
        assert!(!f.needs_array());
    }

    #[test]
    fn interpolation_only_is_text_only() {
        let f = ChildrenFlags::HasInterpolation.into_flag();
        assert!(f.is_text_only());
        assert!(f.has_dynamic());
    }

    #[test]
    fn element_children_need_array() {
        let f = ChildrenFlags::HasElement.into_flag();
        assert!(f.needs_array());
        assert!(!f.is_text_only());
    }

    #[test]
    fn mixed_text_and_element() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasElement);
        assert!(!f.is_text_only());
        assert!(f.needs_array());
        assert!(f.has_children());
    }

    #[test]
    fn single_child_flag() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::SingleChild);
        assert!(f.has(ChildrenFlags::SingleChild));
        assert!(f.needs_array());
    }

    #[test]
    fn structural_directive_flags() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::HasVIf)
            .add(ChildrenFlags::HasVFor);
        assert!(f.has(ChildrenFlags::HasVIf));
        assert!(f.has(ChildrenFlags::HasVFor));
    }

    #[test]
    fn remove_flag() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasComment)
            .remove(ChildrenFlags::HasComment);
        assert!(f.has(ChildrenFlags::HasText));
        assert!(!f.has(ChildrenFlags::HasComment));
    }

    #[test]
    fn union_flags() {
        let a = ChildrenFlags::HasText
            .into_flag()
            .union(ChildrenFlags::HasInterpolation.into_flag());
        let b = ChildrenFlags::HasElement
            .into_flag()
            .union(ChildrenFlags::HasVIf.into_flag());
        let combined = a.union(b);

        assert!(combined.has(ChildrenFlags::HasText));
        assert!(combined.has(ChildrenFlags::HasInterpolation));
        assert!(combined.has(ChildrenFlags::HasElement));
        assert!(combined.has(ChildrenFlags::HasVIf));
    }

    #[test]
    fn clear_resets_all() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .union(ChildrenFlags::HasElement.into_flag())
            .union(ChildrenFlags::HasVIf.into_flag())
            .clear();
        assert_eq!(f.0, 0);
        assert!(!f.has_children());
    }

    #[test]
    fn comment_only_has_children_but_not_text_only() {
        let f = ChildrenFlags::HasComment.into_flag();
        assert!(f.has_children());
        assert!(!f.is_text_only());
        assert!(!f.needs_array());
    }

    #[test]
    fn grouped_masks_work() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation)
            .add(ChildrenFlags::HasVIf);

        assert!(f.has_any(ChildrenFlag::TEXT_LIKE_MASK));
        assert!(
            f.has_all((ChildrenFlags::HasText as u16) | (ChildrenFlags::HasInterpolation as u16))
        );
        assert!(f.has_structural());
        assert!(f.has_dynamic());
    }

    #[test]
    fn mode_derivation() {
        assert_eq!(ChildrenFlag::empty().mode(), ChildrenMode::Empty);

        let comments_only = ChildrenFlags::HasComment.into_flag();
        assert_eq!(comments_only.mode(), ChildrenMode::CommentsOnly);

        let text_static = ChildrenFlags::HasText.into_flag();
        assert_eq!(text_static.mode(), ChildrenMode::TextOnlyStatic);

        let text_dynamic = ChildrenFlags::HasInterpolation.into_flag();
        assert_eq!(text_dynamic.mode(), ChildrenMode::TextOnlyDynamic);

        let single_element = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::SingleChild);
        assert_eq!(single_element.mode(), ChildrenMode::SingleElement);

        let multi_element = ChildrenFlags::HasElement.into_flag();
        assert_eq!(multi_element.mode(), ChildrenMode::MultiElement);

        let mixed = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasElement);
        assert_eq!(mixed.mode(), ChildrenMode::Mixed);
    }

    // @ai-generated - Tests SingleElement mode when element + comment (comment not significant)
    #[test]
    fn mode_single_element_with_comment() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::SingleChild)
            .add(ChildrenFlags::HasComment);
        assert_eq!(f.mode(), ChildrenMode::SingleElement);
    }

    // @ai-generated - Tests MultiElement mode with structural flags
    #[test]
    fn mode_multi_element_with_structural() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::HasVIf)
            .add(ChildrenFlags::HasVFor);
        assert_eq!(f.mode(), ChildrenMode::MultiElement);
        assert!(f.has_structural());
    }

    // @ai-generated - Tests ChildrenFlags::name()
    #[test]
    fn children_flags_name() {
        assert_eq!(ChildrenFlags::HasText.name(), "HAS_TEXT");
        assert_eq!(ChildrenFlags::HasInterpolation.name(), "HAS_INTERPOLATION");
        assert_eq!(ChildrenFlags::HasElement.name(), "HAS_ELEMENT");
        assert_eq!(ChildrenFlags::HasComment.name(), "HAS_COMMENT");
        assert_eq!(ChildrenFlags::SingleChild.name(), "SINGLE_CHILD");
        assert_eq!(ChildrenFlags::HasVIf.name(), "HAS_V_IF");
        assert_eq!(ChildrenFlags::HasVFor.name(), "HAS_V_FOR");
        assert_eq!(
            ChildrenFlags::HasChildWithVSlot.name(),
            "HAS_CHILD_WITH_V_SLOT"
        );
        assert_eq!(
            ChildrenFlags::HasDynamicSlotChild.name(),
            "HAS_DYNAMIC_SLOT_CHILD"
        );
        assert_eq!(ChildrenFlags::HasChildWithKey.name(), "HAS_CHILD_WITH_KEY");
    }

    // @ai-generated - Tests TextOnlyDynamic with both HasText and HasInterpolation
    #[test]
    fn mode_text_only_dynamic_both_flags() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation);
        assert_eq!(f.mode(), ChildrenMode::TextOnlyDynamic);
        assert!(f.is_text_only());
        assert!(f.has_dynamic());
    }
}

mod prop_flag_tests {
    use super::*;

    // @ai-generated - Tests empty PropFlag
    #[test]
    fn empty_flag() {
        let f = PropFlag::empty();
        assert!(f.is_empty());
        assert!(!f.has(PropFlags::HasDynamicKey));
        assert!(!f.has_any(0xFFFF));
    }

    // @ai-generated - Tests adding and checking individual flags
    #[test]
    fn add_and_has_individual_flags() {
        let all_flags = [
            PropFlags::HasDynamicKey,
            PropFlags::HasDynamicClass,
            PropFlags::HasDynamicStyle,
            PropFlags::HasRef,
            PropFlags::HasEventListener,
            PropFlags::HasCustomDirective,
            PropFlags::HasStaticClass,
            PropFlags::HasStaticStyle,
            PropFlags::HasBindSpread,
            PropFlags::HasOnSpread,
            PropFlags::HasModel,
            PropFlags::HasShow,
            PropFlags::HasVHtml,
            PropFlags::HasVText,
            PropFlags::HasDynamicBinding,
        ];

        for &flag in &all_flags {
            let f = PropFlag::empty().add(flag);
            assert!(!f.is_empty(), "flag {:?} should make non-empty", flag);
            assert!(f.has(flag), "flag {:?} should be present", flag);
        }
    }

    // @ai-generated - Tests combining multiple flags
    #[test]
    fn combined_flags() {
        let f = PropFlag::empty()
            .add(PropFlags::HasDynamicKey)
            .add(PropFlags::HasRef)
            .add(PropFlags::HasEventListener);

        assert!(f.has(PropFlags::HasDynamicKey));
        assert!(f.has(PropFlags::HasRef));
        assert!(f.has(PropFlags::HasEventListener));
        assert!(!f.has(PropFlags::HasDynamicClass));
        assert!(!f.has(PropFlags::HasModel));
        assert!(!f.is_empty());
    }

    // @ai-generated - Tests has_any with a mask
    #[test]
    fn has_any_mask() {
        let f = PropFlag::empty().add(PropFlags::HasDynamicStyle);
        let mask = (PropFlags::HasDynamicClass as u16) | (PropFlags::HasDynamicStyle as u16);
        assert!(f.has_any(mask));

        let other_mask = (PropFlags::HasRef as u16) | (PropFlags::HasModel as u16);
        assert!(!f.has_any(other_mask));
    }

    // @ai-generated - Tests into_flag conversion
    #[test]
    fn into_flag_conversion() {
        let f = PropFlags::HasVHtml.into_flag();
        assert!(f.has(PropFlags::HasVHtml));
        assert!(!f.has(PropFlags::HasVText));
    }

    // @ai-generated - Tests new API parity methods: contains, has_all, with, remove, without, union, clear
    #[test]
    fn contains_alias() {
        let f = PropFlag::empty().add(PropFlags::HasRef);
        assert!(f.contains(PropFlags::HasRef));
        assert!(!f.contains(PropFlags::HasModel));
    }

    #[test]
    fn has_all_mask() {
        let f = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(f.has_all(PropFlag::CLASS_MASK));
        assert!(!f.has_all(PropFlag::STYLE_MASK));
    }

    #[test]
    fn with_alias() {
        let f = PropFlag::empty()
            .with(PropFlags::HasDynamicKey)
            .with(PropFlags::HasRef);
        assert!(f.has(PropFlags::HasDynamicKey));
        assert!(f.has(PropFlags::HasRef));
    }

    #[test]
    fn remove_flag() {
        let f = PropFlag::empty()
            .add(PropFlags::HasRef)
            .add(PropFlags::HasModel)
            .remove(PropFlags::HasModel);
        assert!(f.has(PropFlags::HasRef));
        assert!(!f.has(PropFlags::HasModel));
    }

    #[test]
    fn without_alias() {
        let f = PropFlag::empty()
            .add(PropFlags::HasShow)
            .without(PropFlags::HasShow);
        assert!(f.is_empty());
    }

    #[test]
    fn union_flags() {
        let a = PropFlags::HasDynamicClass
            .into_flag()
            .union(PropFlags::HasStaticClass.into_flag());
        let b = PropFlags::HasRef
            .into_flag()
            .union(PropFlags::HasModel.into_flag());
        let combined = a.union(b);

        assert!(combined.has(PropFlags::HasDynamicClass));
        assert!(combined.has(PropFlags::HasStaticClass));
        assert!(combined.has(PropFlags::HasRef));
        assert!(combined.has(PropFlags::HasModel));
    }

    #[test]
    fn clear_resets_all() {
        let f = PropFlags::HasDynamicKey
            .into_flag()
            .union(PropFlags::HasRef.into_flag())
            .union(PropFlags::HasModel.into_flag())
            .clear();
        assert_eq!(f.0, 0);
        assert!(f.is_empty());
    }

    #[test]
    fn new_from_raw() {
        let raw = (PropFlags::HasRef as u16) | (PropFlags::HasModel as u16);
        let f = PropFlag::new(raw);
        assert!(f.has(PropFlags::HasRef));
        assert!(f.has(PropFlags::HasModel));
        assert!(!f.has(PropFlags::HasShow));
    }

    // @ai-generated - Tests mask constants
    #[test]
    fn class_mask() {
        let f = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(f.has_any(PropFlag::CLASS_MASK));
        assert!(f.has_all(PropFlag::CLASS_MASK));

        let only_static = PropFlag::empty().add(PropFlags::HasStaticClass);
        assert!(only_static.has_any(PropFlag::CLASS_MASK));
        assert!(!only_static.has_all(PropFlag::CLASS_MASK));
    }

    #[test]
    fn style_mask() {
        let f = PropFlag::empty().add(PropFlags::HasDynamicStyle);
        assert!(f.has_any(PropFlag::STYLE_MASK));
        assert!(!f.has_all(PropFlag::STYLE_MASK));
    }

    #[test]
    fn spread_mask() {
        let f = PropFlag::empty()
            .add(PropFlags::HasBindSpread)
            .add(PropFlags::HasOnSpread);
        assert!(f.has_any(PropFlag::SPREAD_MASK));
        assert!(f.has_all(PropFlag::SPREAD_MASK));
    }

    #[test]
    fn directive_mask() {
        let f = PropFlag::empty().add(PropFlags::HasModel);
        assert!(f.has_any(PropFlag::DIRECTIVE_MASK));

        let f2 = PropFlag::empty().add(PropFlags::HasRef);
        assert!(!f2.has_any(PropFlag::DIRECTIVE_MASK));
    }

    // @ai-generated - Tests convenience helpers
    #[test]
    fn has_class_helper() {
        assert!(!PropFlag::empty().has_class());
        assert!(PropFlag::empty().add(PropFlags::HasStaticClass).has_class());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicClass)
            .has_class());
    }

    #[test]
    fn has_style_helper() {
        assert!(!PropFlag::empty().has_style());
        assert!(PropFlag::empty().add(PropFlags::HasStaticStyle).has_style());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicStyle)
            .has_style());
    }

    #[test]
    fn has_spread_helper() {
        assert!(!PropFlag::empty().has_spread());
        assert!(PropFlag::empty().add(PropFlags::HasBindSpread).has_spread());
        assert!(PropFlag::empty().add(PropFlags::HasOnSpread).has_spread());
    }

    #[test]
    fn needs_class_merge_helper() {
        assert!(!PropFlag::empty().needs_class_merge());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .needs_class_merge());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasDynamicClass)
            .needs_class_merge());
        assert!(PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass)
            .needs_class_merge());
    }

    #[test]
    fn needs_style_merge_helper() {
        assert!(!PropFlag::empty().needs_style_merge());
        assert!(PropFlag::empty()
            .add(PropFlags::HasStaticStyle)
            .add(PropFlags::HasDynamicStyle)
            .needs_style_merge());
    }

    #[test]
    fn has_directive_helper() {
        assert!(!PropFlag::empty().has_directive());
        assert!(PropFlag::empty()
            .add(PropFlags::HasCustomDirective)
            .has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasModel).has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasShow).has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasVHtml).has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasVText).has_directive());
        // Non-directive flags should NOT trigger has_directive
        assert!(!PropFlag::empty().add(PropFlags::HasRef).has_directive());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasEventListener)
            .has_directive());
    }

    // @ai-generated - Tests into_flag round-trip
    #[test]
    fn into_flag_round_trip() {
        assert!(PropFlags::HasDynamicKey
            .into_flag()
            .has(PropFlags::HasDynamicKey));
        assert!(PropFlags::HasDynamicClass
            .into_flag()
            .has(PropFlags::HasDynamicClass));
        assert!(PropFlags::HasDynamicStyle
            .into_flag()
            .has(PropFlags::HasDynamicStyle));
        assert!(PropFlags::HasRef.into_flag().has(PropFlags::HasRef));
        assert!(PropFlags::HasEventListener
            .into_flag()
            .has(PropFlags::HasEventListener));
        assert!(PropFlags::HasCustomDirective
            .into_flag()
            .has(PropFlags::HasCustomDirective));
        assert!(PropFlags::HasStaticClass
            .into_flag()
            .has(PropFlags::HasStaticClass));
        assert!(PropFlags::HasStaticStyle
            .into_flag()
            .has(PropFlags::HasStaticStyle));
        assert!(PropFlags::HasBindSpread
            .into_flag()
            .has(PropFlags::HasBindSpread));
        assert!(PropFlags::HasOnSpread
            .into_flag()
            .has(PropFlags::HasOnSpread));
        assert!(PropFlags::HasModel.into_flag().has(PropFlags::HasModel));
        assert!(PropFlags::HasShow.into_flag().has(PropFlags::HasShow));
        assert!(PropFlags::HasVHtml.into_flag().has(PropFlags::HasVHtml));
        assert!(PropFlags::HasVText.into_flag().has(PropFlags::HasVText));
        assert!(PropFlags::HasDynamicBinding
            .into_flag()
            .has(PropFlags::HasDynamicBinding));
    }

    // @ai-generated - Tests PropFlags::name()
    #[test]
    fn prop_flags_name() {
        assert_eq!(PropFlags::HasDynamicKey.name(), "HAS_DYNAMIC_KEY");
        assert_eq!(PropFlags::HasDynamicClass.name(), "HAS_DYNAMIC_CLASS");
        assert_eq!(PropFlags::HasDynamicStyle.name(), "HAS_DYNAMIC_STYLE");
        assert_eq!(PropFlags::HasRef.name(), "HAS_REF");
        assert_eq!(PropFlags::HasEventListener.name(), "HAS_EVENT_LISTENER");
        assert_eq!(PropFlags::HasCustomDirective.name(), "HAS_CUSTOM_DIRECTIVE");
        assert_eq!(PropFlags::HasStaticClass.name(), "HAS_STATIC_CLASS");
        assert_eq!(PropFlags::HasStaticStyle.name(), "HAS_STATIC_STYLE");
        assert_eq!(PropFlags::HasBindSpread.name(), "HAS_BIND_SPREAD");
        assert_eq!(PropFlags::HasOnSpread.name(), "HAS_ON_SPREAD");
        assert_eq!(PropFlags::HasModel.name(), "HAS_MODEL");
        assert_eq!(PropFlags::HasShow.name(), "HAS_SHOW");
        assert_eq!(PropFlags::HasVHtml.name(), "HAS_V_HTML");
        assert_eq!(PropFlags::HasVText.name(), "HAS_V_TEXT");
        assert_eq!(PropFlags::HasDynamicBinding.name(), "HAS_DYNAMIC_BINDING");
    }
}

mod element_node_tests {
    use super::*;
    use crate::types::NodeTag;
    use smallvec::SmallVec;

    fn make_plain_element() -> ElementNode {
        ElementNode {
            tag_open: NodeTag {
                start: 0,
                end: 5,
                name_end: 4,
            },
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: true,
            props: Vec::new(),
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
        }
    }

    // @ai-generated - Tests is_plain() with no props or directives
    #[test]
    fn is_plain_empty_element() {
        let el = make_plain_element();
        assert!(el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false when props exist
    #[test]
    fn is_plain_with_props() {
        let mut el = make_plain_element();
        el.props.push(crate::types::NodeProp {
            start: 5,
            name_end: 10,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_condition
    #[test]
    fn is_plain_with_v_condition() {
        let mut el = make_plain_element();
        el.v_condition = Some(ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: crate::types::NodeProp {
                start: 0,
                name_end: 4,
                is_directive: true,
                arg_start: None,
                arg_end: None,
                is_dynamic: None,
                value_start: None,
                value_end: None,
                modifiers: SmallVec::new(),
            },
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_for
    #[test]
    fn is_plain_with_v_for() {
        let mut el = make_plain_element();
        el.v_for = Some(crate::types::NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_slot
    #[test]
    fn is_plain_with_v_slot() {
        let mut el = make_plain_element();
        el.v_slot = Some(crate::types::NodeProp {
            start: 0,
            name_end: 6,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_once
    #[test]
    fn is_plain_with_v_once() {
        let mut el = make_plain_element();
        el.v_once = Some(crate::types::NodeProp {
            start: 0,
            name_end: 6,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_ref
    #[test]
    fn is_plain_with_v_ref() {
        let mut el = make_plain_element();
        el.v_ref = Some(crate::types::NodeProp {
            start: 0,
            name_end: 3,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(5),
            value_end: Some(8),
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests needs_expression_parsing() returns false for empty element
    #[test]
    fn needs_expression_parsing_empty_element() {
        let el = make_plain_element();
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Static class only does not need OXC parsing
    #[test]
    fn needs_expression_parsing_static_class_only() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasStaticClass);
        el.props.push(crate::types::NodeProp {
            start: 5,
            name_end: 10,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(12),
            value_end: Some(15),
            modifiers: SmallVec::new(),
        });
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Static class + static style does not need OXC parsing
    #[test]
    fn needs_expression_parsing_static_class_and_style() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasStaticStyle);
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Static ref does not need OXC parsing
    #[test]
    fn needs_expression_parsing_static_ref() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasRef);
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Dynamic class needs OXC parsing
    #[test]
    fn needs_expression_parsing_dynamic_class() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasDynamicClass);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - Event listener needs OXC parsing
    #[test]
    fn needs_expression_parsing_event_listener() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasEventListener);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - v-model needs OXC parsing
    #[test]
    fn needs_expression_parsing_v_model() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasModel);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - v-if needs OXC parsing (via cached directive)
    #[test]
    fn needs_expression_parsing_v_if() {
        let mut el = make_plain_element();
        el.v_condition = Some(ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: crate::types::NodeProp {
                start: 0,
                name_end: 4,
                is_directive: true,
                arg_start: None,
                arg_end: None,
                is_dynamic: None,
                value_start: Some(6),
                value_end: Some(10),
                modifiers: SmallVec::new(),
            },
        });
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - v-for needs OXC parsing (via cached directive)
    #[test]
    fn needs_expression_parsing_v_for() {
        let mut el = make_plain_element();
        el.v_for = Some(crate::types::NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(7),
            value_end: Some(20),
            modifiers: SmallVec::new(),
        });
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - Static class + dynamic class needs OXC parsing
    #[test]
    fn needs_expression_parsing_mixed_static_dynamic() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - Tests PropFlag::needs_oxc_parsing mask exhaustively
    #[test]
    fn prop_flag_needs_oxc_parsing() {
        // Static-only flags should NOT need OXC
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .needs_oxc_parsing());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticStyle)
            .needs_oxc_parsing());
        assert!(!PropFlag::empty().add(PropFlags::HasRef).needs_oxc_parsing());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasStaticStyle)
            .add(PropFlags::HasRef)
            .needs_oxc_parsing());

        // Dynamic flags SHOULD need OXC
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicKey)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicClass)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicStyle)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasEventListener)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasCustomDirective)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasBindSpread)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasOnSpread)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasModel)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasShow)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasVHtml)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasVText)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicBinding)
            .needs_oxc_parsing());
    }

    // @ai-generated - Tests is_component forwarding
    #[test]
    fn is_component_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.is_component());
        el.tag_type = TagType::Component;
        assert!(el.is_component());
    }

    // @ai-generated - Tests is_slot_outlet forwarding
    #[test]
    fn is_slot_outlet_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.is_slot_outlet());
        el.tag_type = TagType::SlotOutlet;
        assert!(el.is_slot_outlet());
    }

    // @ai-generated - Tests is_template forwarding
    #[test]
    fn is_template_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.is_template());
        el.tag_type = TagType::Template;
        assert!(el.is_template());
    }

    // @ai-generated - Tests has_class forwarding
    #[test]
    fn has_class_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.has_class());
        el.prop_flag = el.prop_flag.add(PropFlags::HasStaticClass);
        assert!(el.has_class());
    }

    // @ai-generated - Tests has_style forwarding
    #[test]
    fn has_style_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.has_style());
        el.prop_flag = el.prop_flag.add(PropFlags::HasDynamicStyle);
        assert!(el.has_style());
    }

    // @ai-generated - Tests has_spread forwarding
    #[test]
    fn has_spread_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.has_spread());
        el.prop_flag = el.prop_flag.add(PropFlags::HasBindSpread);
        assert!(el.has_spread());
    }

    // @ai-generated - Tests needs_class_merge forwarding
    #[test]
    fn needs_class_merge_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.needs_class_merge());
        el.prop_flag = el
            .prop_flag
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(el.needs_class_merge());
    }

    // @ai-generated - Tests needs_style_merge forwarding
    #[test]
    fn needs_style_merge_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.needs_style_merge());
        el.prop_flag = el
            .prop_flag
            .add(PropFlags::HasStaticStyle)
            .add(PropFlags::HasDynamicStyle);
        assert!(el.needs_style_merge());
    }
}

mod tag_type_tests {
    use super::*;

    // @ai-generated - Tests TagType convenience methods
    #[test]
    fn is_element() {
        assert!(TagType::Element.is_element());
        assert!(!TagType::Component.is_element());
        assert!(!TagType::SlotOutlet.is_element());
        assert!(!TagType::Template.is_element());
    }

    #[test]
    fn is_component() {
        assert!(!TagType::Element.is_component());
        assert!(TagType::Component.is_component());
        assert!(!TagType::SlotOutlet.is_component());
        assert!(!TagType::Template.is_component());
    }

    #[test]
    fn is_slot_outlet() {
        assert!(!TagType::Element.is_slot_outlet());
        assert!(!TagType::Component.is_slot_outlet());
        assert!(TagType::SlotOutlet.is_slot_outlet());
        assert!(!TagType::Template.is_slot_outlet());
    }

    #[test]
    fn is_template() {
        assert!(!TagType::Element.is_template());
        assert!(!TagType::Component.is_template());
        assert!(!TagType::SlotOutlet.is_template());
        assert!(TagType::Template.is_template());
    }

    #[test]
    fn is_special() {
        assert!(!TagType::Element.is_special());
        assert!(TagType::Component.is_special());
        assert!(TagType::SlotOutlet.is_special());
        assert!(TagType::Template.is_special());
    }
}
