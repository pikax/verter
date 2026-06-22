import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<button>go</button>`);

export default function App($$anchor) {
  let id = $.state("a");
  let cls = $.state("box");
  let color = $.state("red");
  var button = root();
  let styles;
  $.template_effect(() => {
    $.set_attribute(button, "id", $.get(id));
    $.set_class(button, 1, $.clsx($.get(cls)));
    styles = $.set_style(button, "font-weight:bold", styles, { color: $.get(color) });
  });
  $.delegated("click", button, () => {
    $.set(id, $.get(id) + "!");
    $.set(cls, $.get(cls) + " on");
    $.set(color, "blue");
  });
  $.append($$anchor, button);
}

$.delegate(["click"]);
