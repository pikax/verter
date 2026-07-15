import "svelte/internal/disclose-version";
import "svelte/internal/flags/legacy";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<p> </p>`);
var root_1 = $.from_html(`<p>pending</p>`);

export default function App($$anchor) {
  let promise = Promise.resolve("ready");
  var fragment = $.comment();
  var node = $.first_child(fragment);
  $.await(
    node,
    () => promise,
    ($$anchor) => {
      var p_1 = root_1();
      $.append($$anchor, p_1);
    },
    ($$anchor, value) => {
      var p = root();
      var text = $.child(p, true);
      $.reset(p);
      $.template_effect(() => $.set_text(text, $.get(value)));
      $.append($$anchor, p);
    },
  );
  $.append($$anchor, fragment);
}
