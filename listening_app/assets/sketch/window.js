/**
 * Live sketch window: tabs per diagram, rendering, and the pointer / click / tab / mic / copy events sent back to Python
 * through `window.pywebview.api.event`. Python calls `SketchApp.receive(message)`.
 */
(function (global) {
  "use strict";

  const POINTER_INTERVAL_MS = 100;
  const NOTICE_MS = 3000;
  const svg = document.getElementById("sketch");
  const tabs = document.getElementById("tabs");
  const title = document.getElementById("title");
  const eyebrow = document.getElementById("eyebrow");
  const notice = document.getElementById("notice");
  const canvas = document.getElementById("canvas");
  const mic = document.getElementById("mic");
  const micLabel = document.getElementById("mic-label");
  const copyPath = document.getElementById("copy-path");
  const empty = { number: 0, title: "", kind: "flow", nodes: [], edges: [], groups: [], selected: [], pointed: null };

  let views = [];
  let active = null;
  let pointed = null;
  let selection = new Set();
  let mouse = null;
  let ready = false;
  const outbox = [];

  function send(event) {
    if (ready) global.pywebview.api.event(JSON.stringify(event));
    else outbox.push(event);
  }

  function current() {
    return views.find((view) => view.number === active) || empty;
  }

  function drawTabs() {
    tabs.replaceChildren(...views.map((view) => {
      const tab = document.createElement("button");
      tab.className = "sk-tab";
      tab.setAttribute("role", "tab");
      tab.setAttribute("aria-selected", String(view.number === active));
      const number = document.createElement("span");
      number.className = "sk-tab-number";
      number.textContent = String(view.number);
      tab.append(number, view.title || "New diagram");
      tab.addEventListener("click", () => activate(view.number));
      return tab;
    }));
  }

  function draw() {
    const view = current();
    eyebrow.textContent = view.number ? `Diagram ${view.number} · ${view.kind}` : "Live sketch";
    title.textContent = view.title || (view.number ? "New diagram" : "Listening…");
    copyPath.disabled = !view.number;
    SketchRenderer.render(svg, Object.assign({}, view, { selected: [...selection], pointed }));
    sendBoxes();
  }

  function sendBoxes() {
    send({ type: "boxes", boxes: SketchRenderer.boxes(svg) });
  }

  function activate(number) {
    if (number === active) return;
    active = number;
    selection = new Set(current().selected);
    drawTabs();
    draw();
    send({ type: "activate", number });
  }

  function showNotice(text) {
    notice.textContent = text;
    notice.classList.add("is-shown");
    clearTimeout(showNotice.timer);
    showNotice.timer = setTimeout(() => notice.classList.remove("is-shown"), NOTICE_MS);
  }

  function showMicrophone(on) {
    mic.setAttribute("aria-pressed", String(on));
    micLabel.textContent = on ? "Mic on" : "Mic off";
    mic.title = on ? "Live sketch is listening. Click to stop listening." : "Click to listen again.";
    mic.disabled = false;
  }

  function receive(message) {
    if (message.type === "state") {
      const switched = message.active !== active;
      views = message.views;
      active = message.active;
      if (switched || message.updated === active) selection = new Set(current().selected);
      drawTabs();
      if (switched || message.updated === active || message.updated === null) draw();
    } else if (message.type === "point") {
      pointed = message.element;
      SketchRenderer.mark(svg, [...selection], pointed);
    } else if (message.type === "notice") {
      showNotice(message.text);
    } else if (message.type === "microphone") {
      showMicrophone(message.on);
    }
  }

  mic.addEventListener("click", () => {
    mic.disabled = true;
    send({ type: "microphone" });
  });
  copyPath.addEventListener("click", () => send({ type: "copy-path", number: active }));

  svg.addEventListener("click", (event) => {
    if (!current().number) return;
    const element = event.target.closest("[data-id]");
    if (element) {
      const id = element.getAttribute("data-id");
      if (selection.has(id)) selection.delete(id);
      else selection.add(id);
    } else {
      selection.clear();
    }
    SketchRenderer.mark(svg, [...selection], pointed);
    send({ type: "select", number: active, ids: [...selection] });
  });

  canvas.addEventListener("mousemove", (event) => {
    mouse = { x: event.clientX, y: event.clientY };
  });
  canvas.addEventListener("mouseleave", () => {
    mouse = null;
    send({ type: "leave", t: Date.now() / 1000 });
  });
  setInterval(() => {
    if (mouse) send({ type: "pointer", t: Date.now() / 1000, x: mouse.x, y: mouse.y });
  }, POINTER_INTERVAL_MS);

  let resizeTimer = null;
  global.addEventListener("resize", () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(sendBoxes, 150);
  });

  global.addEventListener("pywebviewready", () => {
    ready = true;
    while (outbox.length) send(outbox.shift());
  });

  function warmUp() {
    const sample = { kind: "flow", groups: [], selected: [], pointed: null,
      nodes: [{ id: "a", label: "Warm up", shape: "service" }, { id: "b", label: "Layout", shape: "database" }],
      edges: [{ id: "a->b", source: "a", target: "b", label: "once", style: "sync" }] };
    SketchRenderer.render(document.getElementById("warmup"), sample);
  }

  warmUp();
  draw();
  global.SketchApp = { receive };
})(window);
