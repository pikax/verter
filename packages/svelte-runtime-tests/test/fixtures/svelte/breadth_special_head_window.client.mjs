import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<p> </p>`);

export default function App($$anchor) {
  let key = $.state("");
  var p = root();
  $.event("keydown", $.window, (event) => $.set(key, event.key, true));
  $.head("n50uah", ($$anchor) => {
    $.deferred_template_effect(() => {
      $.document.title = ($.get(key) || "initial") ?? "";
    });
  });
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(key)));
  $.append($$anchor, p);
}
