import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<p> </p>`);

export default function App($$anchor, $$props) {
  var p = root();
  var text_1 = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text_1, $$props.text));
  $.append($$anchor, p);
}
