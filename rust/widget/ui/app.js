// The widget's page. All the wording, numbers and colours are decided in Rust
// (src-tauri/src/view.rs); this file draws that view model and forwards clicks.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

/** A new element: el("div", "small wrap", "text", "#f79009"). */
function el(tag, className, text, color) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  if (color) node.style.color = color;
  return node;
}

/** A row of a coloured dot and a small grey title, with something on the right if given. */
function head(color, title, right) {
  const row = el("div", "head small");
  if (color) row.append(el("span", "dot", "●", color));
  row.append(el("span", "", title));
  if (right) row.append(right);
  return row;
}

function drawCosts(costs) {
  const nodes = [];
  for (const cost of costs) {
    nodes.push(head(cost.color, cost.title));
    if (cost.hint != null) {
      nodes.push(el("div", "small", cost.hint));
    } else {
      const row = el("div", "cost");
      row.append(
        el("span", "month", cost.month),
        el("span", "small spaced", " this month"),
        el("span", "today right", cost.today),
      );
      nodes.push(row);
    }
    if (cost.warning != null) nodes.push(el("div", "small wrap", cost.warning, "var(--warn)"));
  }
  $("costs").replaceChildren(...nodes);
}

function drawLimits(limits, notes) {
  const nodes = [];
  for (const block of limits) {
    nodes.push(head(block.color, block.title, block.plan && el("span", "right", block.plan)));
    if (block.empty != null) nodes.push(el("div", "small", block.empty));
    for (const bar of block.windows) {
      const row = el("div", "limit");
      row.append(el("span", "", bar.label), el("span", "percent right", bar.percent));
      const track = el("div", "bar");
      const fill = el("div");
      fill.style.width = bar.fill + "%";
      fill.style.background = bar.color;
      track.append(fill);
      nodes.push(row, track, el("div", "small", bar.reset));
      if (bar.pace) nodes.push(el("div", "small", bar.pace.text, bar.pace.color));
    }
    for (const detail of block.details) nodes.push(el("div", "small wrap", detail));
    if (block.note != null) nodes.push(el("div", "small italic source", block.note));
    if (block.error != null) nodes.push(el("div", "small wrap", block.error, "var(--warn)"));
  }
  for (const note of notes) nodes.push(el("div", "small wrap lines note", note));
  $("limits").replaceChildren(...nodes);
}

function drawApi(api) {
  const nodes = [];
  for (const block of api) {
    nodes.push(head(block.color, block.title, block.right && el("span", "spend right", block.right)));
    for (const line of block.lines) {
      nodes.push(el("div", line.kind === "balance" ? "balance" : "small wrap", line.text, line.color));
    }
  }
  $("api").replaceChildren(...nodes);
}

function draw({ view, working, status }) {
  $("working").textContent = working;
  $("today").textContent = view.today;
  $("today-split").replaceChildren(
    ...view.today_split.flatMap((part) => [el("span", "dot", "●", part.color), el("span", "spaced", part.text)]),
  );
  $("month").textContent = view.month;
  $("month-sub").textContent = view.month_sub;
  drawCosts(view.costs);
  drawLimits(view.limits, view.notes);
  drawApi(view.api);
  $("status").textContent = status.text;
  $("status").style.color = status.color;
}

// The window is as big as the card, whatever the card currently holds.
const card = $("card");
new ResizeObserver(() => {
  invoke("fit", { width: card.offsetWidth, height: card.offsetHeight });
}).observe(card);

// Left-drag anywhere except the buttons moves the window; right-click opens the menu.
document.addEventListener("mousedown", (event) => {
  if (event.button === 0 && !event.target.closest("button")) invoke("drag");
});
document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  invoke("menu");
});
for (const button of document.querySelectorAll("[data-press]")) {
  button.addEventListener("click", () => invoke("press", { button: button.dataset.press }));
}

// Hovering "● N working" opens the card of working sessions; Rust closes it again.
$("working").addEventListener("mouseenter", () => {
  const { left, top, right, bottom } = $("working").getBoundingClientRect();
  invoke("working_enter", { left, top, right, bottom });
});

listen("screen", (event) => draw(event.payload));
invoke("screen").then(draw);
