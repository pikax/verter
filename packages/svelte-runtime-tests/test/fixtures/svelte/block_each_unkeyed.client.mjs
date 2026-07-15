import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<p> </p>`);

export default function App($$anchor, $$props) {
  var fragment = $.comment();
  var node = $.first_child(fragment);
  $.each(
    node,
    17,
    () => $$props.rows,
    $.index,
    ($$anchor, row) => {
      var p = root();
      var text = $.child(p, true);
      $.reset(p);
      $.template_effect(() => $.set_text(text, $.get(row)));
      $.append($$anchor, p);
    },
  );
  $.append($$anchor, fragment);
}
