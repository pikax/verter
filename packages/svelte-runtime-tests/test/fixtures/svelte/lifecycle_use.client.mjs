import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div>x</div>`);

export default function App($$anchor, $$props) {
  var div = root();
  $.action(div, ($$node) => $$props.act?.($$node));
  $.append($$anchor, div);
}
