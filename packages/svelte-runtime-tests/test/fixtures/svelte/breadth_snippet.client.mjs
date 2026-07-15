import "svelte/internal/disclose-version";
import "svelte/internal/flags/legacy";
import * as $ from "svelte/internal/client";

const item = ($$anchor, value = $.noop) => {
  var p = root();
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, value()));
  $.append($$anchor, p);
};
var root = $.from_html(`<p> </p>`);

export default function App($$anchor) {
  item($$anchor, () => "rendered");
}
