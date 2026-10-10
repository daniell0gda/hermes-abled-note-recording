/**
 * Live sketch window: tabs per diagram, rendering, and the pointer / click / tab / mic / copy events sent back to Python
 * through `window.pywebview.api.event`. Python calls `SketchApp.receive(message)`.
 * Ctrl + mouse wheel zooms at the mouse, dragging moves the view, Ctrl+0 resets it; a new tab starts unzoomed.
 * A middle-click on a tab closes that diagram.
 */
(function (global) {
  "use strict";

  const POINTER_INTERVAL_MS = 100;
  const NOTICE_MS = 3000;
  const BOXES_DELAY_MS = 150;
  const MIN_ZOOM = 0.25;
  const MAX_ZOOM = 8;
  const ZOOM_PER_WHEEL_DELTA = 0.0015;
  const DRAG_THRESHOLD_PX = 4;
  const MIDDLE_BUTTON = 1;
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
  let camera = { scale: 1, x: 0, y: 0 };
  let drag = null;
  let dragged = false;
  let boxesTimer = null;

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
      tab.title = "Middle-click to close";
      tab.addEventListener("click", () => activate(view.number));
      tab.addEventListener("mousedown", preventMiddleButtonScroll);
      tab.addEventListener("auxclick", (event) => {
        if (event.button === MIDDLE_BUTTON) send({ type: "close", number: view.number });
      });
      return tab;
    }));
  }

  function preventMiddleButtonScroll(event) {
    if (event.button === MIDDLE_BUTTON) event.preventDefault();
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

  function sendBoxesSoon() {
    clearTimeout(boxesTimer);
    boxesTimer = setTimeout(sendBoxes, BOXES_DELAY_MS);
  }

  function applyCamera() {
    svg.style.transform = `translate(${camera.x}px, ${camera.y}px) scale(${camera.scale})`;
    sendBoxesSoon();
  }

  function resetCamera() {
    camera = { scale: 1, x: 0, y: 0 };
    applyCamera();
  }

  function zoomAt(clientX, clientY, factor) {
    const rect = svg.getBoundingClientRect();
    const scale = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, camera.scale * factor));
    const ratio = scale / camera.scale;
    camera = {
      scale,
      x: camera.x + (clientX - rect.left) * (1 - ratio),
      y: camera.y + (clientY - rect.top) * (1 - ratio),
    };
    applyCamera();
  }

  function startDrag(event) {
    if (event.button !== 0) return;
    drag = { clientX: event.clientX, clientY: event.clientY, x: camera.x, y: camera.y };
    dragged = false;
  }

  function moveDrag(event) {
    if (!drag) return;
    const dx = event.clientX - drag.clientX;
    const dy = event.clientY - drag.clientY;
    if (!dragged && Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return;
    dragged = true;
    canvas.classList.add("is-dragging");
    camera = { scale: camera.scale, x: drag.x + dx, y: drag.y + dy };
    applyCamera();
  }

  function endDrag() {
    drag = null;
    canvas.classList.remove("is-dragging");
  }

  function activate(number) {
    if (number === active) return;
    active = number;
    selection = new Set(current().selected);
    drawTabs();
    resetCamera();
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
      if (switched) resetCamera();
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

  canvas.addEventListener("wheel", (event) => {
    if (!event.ctrlKey) return;
    event.preventDefault();
    zoomAt(event.clientX, event.clientY, Math.exp(-event.deltaY * ZOOM_PER_WHEEL_DELTA));
  }, { passive: false });
  canvas.addEventListener("mousedown", startDrag);
  global.addEventListener("mousemove", moveDrag);
  global.addEventListener("mouseup", endDrag);
  canvas.addEventListener("click", (event) => {
    if (!dragged) return;
    dragged = false;
    event.stopPropagation();
  }, true);
  global.addEventListener("keydown", (event) => {
    if (!(event.ctrlKey && event.key === "0")) return;
    event.preventDefault();
    resetCamera();
  });

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

  global.addEventListener("resize", sendBoxesSoon);

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
