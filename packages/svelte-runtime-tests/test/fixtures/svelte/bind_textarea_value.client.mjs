import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<textarea></textarea> <p> </p>`, 1);

export default function App($$anchor) {
  let v = $.state("");
  var fragment = root();
  var textarea = $.first_child(fragment);
  $.remove_textarea_child(textarea);
  var p = $.sibling(textarea, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(v)));
  $.bind_value(
    textarea,
    () => $.get(v),
    ($$value) => $.set(v, $$value),
  );
  $.append($$anchor, fragment);
}
