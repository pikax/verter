import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div> </div>`);

export default function App($$anchor, $$props) {
  let c = $.state(0);
  var div = root();
  var text = $.child(div, true);
  $.reset(div);
  $.action(div, ($$node) => $$props.act?.($$node));
  $.effect(() => $.event("click", div, () => $.update(c)));
  $.template_effect(() => $.set_text(text, $.get(c)));
  $.append($$anchor, div);
}
