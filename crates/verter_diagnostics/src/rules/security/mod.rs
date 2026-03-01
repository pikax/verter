//! Security lint rules.

mod no_unsafe_url;
mod no_v_html;

pub use no_unsafe_url::NoUnsafeUrl;
pub use no_v_html::NoVHtml;
