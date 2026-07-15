import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div contenteditable=""></div> <p> </p>`, 1);

export default function App($$anchor) {
  let h = $.state("");
  var fragment = root();
  var div = $.first_child(fragment);
  var p = $.sibling(div, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(h)));
  $.bind_content_editable(
    "innerHTML",
    div,
    () => $.get(h),
    ($$value) => $.set(h, $$value),
  );
  $.append($$anchor, fragment);
}
