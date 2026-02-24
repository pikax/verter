//! Accessibility lint rules (WCAG 2.1).

mod alt_text;
mod anchor_has_content;
mod aria_role;
mod click_events_have_key_events;
mod form_control_has_label;
mod heading_has_content;
mod iframe_has_title;
mod no_autofocus;
mod no_distracting_elements;
mod tabindex_no_positive;

pub use alt_text::AltText;
pub use anchor_has_content::AnchorHasContent;
pub use aria_role::AriaRole;
pub use click_events_have_key_events::ClickEventsHaveKeyEvents;
pub use form_control_has_label::FormControlHasLabel;
pub use heading_has_content::HeadingHasContent;
pub use iframe_has_title::IframeHasTitle;
pub use no_autofocus::NoAutofocus;
pub use no_distracting_elements::NoDistractingElements;
pub use tabindex_no_positive::TabindexNoPositive;
