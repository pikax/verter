#[inline(always)]
pub fn is_tag_name_component(name: &[u8]) -> bool {
    // starts with an uppercase letter or contains a hyphen (custom element)
    name[0] >= b'A' && name[0] <= b'Z' || name.contains(&b'-')
}

// from https://github.com/vuejs/core/blob/40193696b3564202173ac0367e4b3ae48c4ffb84/packages/shared/src/domTagConfig.ts
// Fast, allocation-free tag membership checks for &[u8] using compile-time PHF sets.

use phf::phf_set;

static HTML_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"html", b"body", b"base", b"head", b"link", b"meta", b"style", b"title", b"address", b"article",
  b"aside", b"footer", b"header", b"hgroup", b"h1", b"h2", b"h3", b"h4", b"h5", b"h6", b"nav",
  b"section", b"div", b"dd", b"dl", b"dt", b"figcaption", b"figure", b"picture", b"hr", b"img",
  b"li", b"main", b"ol", b"p", b"pre", b"ul", b"a", b"b", b"abbr", b"bdi", b"bdo", b"br", b"cite",
  b"code", b"data", b"dfn", b"em", b"i", b"kbd", b"mark", b"q", b"rp", b"rt", b"ruby", b"s",
  b"samp", b"small", b"span", b"strong", b"sub", b"sup", b"time", b"u", b"var", b"wbr", b"area",
  b"audio", b"map", b"track", b"video", b"embed", b"object", b"param", b"source", b"canvas",
  b"script", b"noscript", b"del", b"ins", b"caption", b"col", b"colgroup", b"table", b"thead",
  b"tbody", b"td", b"th", b"tr", b"button", b"datalist", b"fieldset", b"form", b"input", b"label",
  b"legend", b"meter", b"optgroup", b"option", b"output", b"progress", b"select", b"textarea",
  b"details", b"dialog", b"menu", b"summary", b"template", b"blockquote", b"iframe", b"tfoot",
};

static SVG_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"svg", b"animate", b"animateMotion", b"animateTransform", b"circle", b"clipPath",
  b"color-profile", b"defs", b"desc", b"discard", b"ellipse", b"feBlend", b"feColorMatrix",
  b"feComponentTransfer", b"feComposite", b"feConvolveMatrix", b"feDiffuseLighting",
  b"feDisplacementMap", b"feDistantLight", b"feDropShadow", b"feFlood", b"feFuncA", b"feFuncB",
  b"feFuncG", b"feFuncR", b"feGaussianBlur", b"feImage", b"feMerge", b"feMergeNode",
  b"feMorphology", b"feOffset", b"fePointLight", b"feSpecularLighting", b"feSpotLight", b"feTile",
  b"feTurbulence", b"filter", b"foreignObject", b"g", b"hatch", b"hatchpath", b"image", b"line",
  b"linearGradient", b"marker", b"mask", b"mesh", b"meshgradient", b"meshpatch", b"meshrow",
  b"metadata", b"mpath", b"path", b"pattern", b"polygon", b"polyline", b"radialGradient", b"rect",
  b"set", b"solidcolor", b"stop", b"switch", b"symbol", b"text", b"textPath", b"title", b"tspan",
  b"unknown", b"use", b"view",
};

static MATH_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"annotation", b"annotation-xml", b"maction", b"maligngroup", b"malignmark", b"math", b"menclose",
  b"merror", b"mfenced", b"mfrac", b"mfraction", b"mglyph", b"mi", b"mlabeledtr", b"mlongdiv",
  b"mmultiscripts", b"mn", b"mo", b"mover", b"mpadded", b"mphantom", b"mprescripts", b"mroot",
  b"mrow", b"ms", b"mscarries", b"mscarry", b"msgroup", b"msline", b"mspace", b"msqrt", b"msrow",
  b"mstack", b"mstyle", b"msub", b"msubsup", b"msup", b"mtable", b"mtd", b"mtext", b"mtr",
  b"munder", b"munderover", b"none", b"semantics",
};

static VOID_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"area", b"base", b"br", b"col", b"embed", b"hr", b"img", b"input", b"link", b"meta", b"param",
  b"source", b"track", b"wbr",
};

static FORMATTING_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"a", b"b", b"big", b"code", b"em", b"font", b"i", b"nobr", b"s", b"small", b"strike", b"strong",
  b"tt", b"u",
};

static ALWAYS_CLOSE_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"title", b"style", b"script", b"noscript", b"template", b"object", b"table", b"button",
  b"textarea", b"select", b"iframe", b"fieldset",
};

static INLINE_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"a", b"abbr", b"acronym", b"b", b"bdi", b"bdo", b"big", b"br", b"button", b"canvas", b"cite",
  b"code", b"data", b"datalist", b"del", b"dfn", b"em", b"embed", b"i", b"iframe", b"img",
  b"input", b"ins", b"kbd", b"label", b"map", b"mark", b"meter", b"noscript", b"object",
  b"output", b"picture", b"progress", b"q", b"ruby", b"s", b"samp", b"script", b"select", b"small",
  b"span", b"strong", b"sub", b"sup", b"svg", b"textarea", b"time", b"u", b"tt", b"var", b"video",
};

static BLOCK_TAGS_B: phf::Set<&'static [u8]> = phf_set! {
  b"address", b"article", b"aside", b"blockquote", b"dd", b"details", b"dialog", b"div", b"dl",
  b"dt", b"fieldset", b"figcaption", b"figure", b"footer", b"form", b"h1", b"h2", b"h3", b"h4",
  b"h5", b"h6", b"header", b"hgroup", b"hr", b"li", b"main", b"menu", b"nav", b"ol", b"p", b"pre",
  b"section", b"table", b"ul",
};

#[inline(always)]
pub fn is_html_tag(key: &[u8]) -> bool {
    HTML_TAGS_B.contains(key)
}

#[inline(always)]
pub fn is_svg_tag(key: &[u8]) -> bool {
    SVG_TAGS_B.contains(key)
}

#[inline(always)]
pub fn is_mathml_tag(key: &[u8]) -> bool {
    MATH_TAGS_B.contains(key)
}

#[inline(always)]
pub fn is_void_tag(key: &[u8]) -> bool {
    VOID_TAGS_B.contains(key)
}

#[inline(always)]
pub fn is_formatting_tag(key: &[u8]) -> bool {
    FORMATTING_TAGS_B.contains(key)
}

#[inline(always)]
pub fn is_always_close_tag(key: &[u8]) -> bool {
    ALWAYS_CLOSE_TAGS_B.contains(key)
}

#[inline(always)]
pub fn is_inline_tag(key: &[u8]) -> bool {
    INLINE_TAGS_B.contains(key)
}

#[inline(always)]
pub fn is_block_tag(key: &[u8]) -> bool {
    BLOCK_TAGS_B.contains(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanity_checks_bytes() {
        assert!(is_html_tag(b"div"));
        assert!(is_svg_tag(b"animateTransform"));
        assert!(is_mathml_tag(b"msubsup"));
        assert!(is_void_tag(b"img"));
        assert!(is_formatting_tag(b"strong"));
        assert!(is_always_close_tag(b"textarea"));
        assert!(is_inline_tag(b"span"));
        assert!(is_block_tag(b"section"));

        assert!(!is_html_tag(b"animate"));
        assert!(!is_svg_tag(b"div"));
        assert!(!is_void_tag(b"div"));
    }
}
