use smallvec::SmallVec;

use crate::parser::types::{RootNodeTemplate, RootNodeTemplateContent};
use crate::types::NodeTag;

pub fn make_root() -> RootNodeTemplate {
    RootNodeTemplate {
        tag_open: NodeTag {
            start: 0,
            end: 0,
            name_end: 0,
        },
        tag_close: None,
        lang: None,
        attributes: Vec::new(),
        content: Some(RootNodeTemplateContent {
            start: 0,
            end: 0,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    }
}

pub fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
    NodeTag {
        start,
        end,
        name_end,
    }
}
