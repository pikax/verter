import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(
  `<select><option>none</option><option>a</option><option>b</option></select> <p> </p> <button>clear</button>`,
  1,
);

export default function App($$anchor) {
  let v = $.state("a");
  var fragment = root();
  var select = $.first_child(fragment);
  var option = $.child(select);
  option.value = option.__value = "";
  var option_1 = $.sibling(option);
  option_1.value = option_1.__value = "a";
  var option_2 = $.sibling(option_1);
  option_2.value = option_2.__value = "b";
  $.reset(select);
  var p = $.sibling(select, 2);
  var text = $.child(p, true);
  $.reset(p);
  var button = $.sibling(p, 2);
  $.template_effect(() => $.set_text(text, $.get(v)));
  $.bind_select_value(
    select,
    () => $.get(v),
    ($$value) => $.set(v, $$value),
  );
  $.delegated("click", button, () => $.set(v, ""));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
