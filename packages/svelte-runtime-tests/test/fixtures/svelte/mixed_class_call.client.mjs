import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<button>go</button>`);

export default function App($$anchor) {
  let c = $.state("x");
  var button = root();
  $.template_effect(($0) => $.set_class(button, 1, `a${$0 ?? ""}b`), [() => String($.get(c))]);
  $.delegated("click", button, () => $.set(c, $.get(c) + "!"));
  $.append($$anchor, button);
}

$.delegate(["click"]);
