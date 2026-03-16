//! Accessibility lint rules (WCAG 2.1).

mod alt_text;
mod anchor_has_content;
mod aria_props;
mod aria_role;
mod click_events_have_key_events;
mod form_control_has_label;
mod heading_has_content;
mod iframe_has_title;
mod interactive_supports_focus;
mod media_has_caption;
mod no_aria_hidden_on_focusable;
mod no_autofocus;
mod no_distracting_elements;
mod role_has_required_aria_props;
mod tabindex_no_positive;

pub use alt_text::AltText;
pub use anchor_has_content::AnchorHasContent;
pub use aria_props::AriaProps;
pub use aria_role::AriaRole;
pub use click_events_have_key_events::ClickEventsHaveKeyEvents;
pub use form_control_has_label::FormControlHasLabel;
pub use heading_has_content::HeadingHasContent;
pub use iframe_has_title::IframeHasTitle;
pub use interactive_supports_focus::InteractiveSupportsFocus;
pub use media_has_caption::MediaHasCaption;
pub use no_aria_hidden_on_focusable::NoAriaHiddenOnFocusable;
pub use no_autofocus::NoAutofocus;
pub use no_distracting_elements::NoDistractingElements;
pub use role_has_required_aria_props::RoleHasRequiredAriaProps;
pub use tabindex_no_positive::TabindexNoPositive;
