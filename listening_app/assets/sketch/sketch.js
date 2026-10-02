/**
 * Live sketch renderer: lays out a diagram (dagre, or lifelines for sequence diagrams) and draws it
 * with diagram-design SVG elements. Exposes `SketchRenderer.render(svg, diagram)` and
 * `SketchRenderer.mark(svg, selected, pointed)`.
 */
(function (global) {
  "use strict";

  const SVG_NS = "http://www.w3.org/2000/svg";
  const GRID = 4;
  const LABEL_PX = 12;
  const SANS_EM = 0.6;
  const MONO_EM = 0.62;
  const EDGE_LABEL_PX = 8;
  const LINE_HEIGHT = 15;
  const WRAP_CHARS = 22;
  const NODE_PAD_X = 16;
  const NODE_MIN_WIDTH = 88;
  const MARGIN = 24;
  const CORNER = 8;
  const MAX_SCALE = 1.6;
  const EYEBROWS = { database: "DATABASE", queue: "QUEUE", cache: "CACHE", external: "EXTERNAL", client: "CLIENT", actor: "ACTOR" };
  const BASE_HEIGHT = { database: 56, decision: 56, actor: 48 };
  const RANK_DIRECTION = { tree: "TB" };

  function snap(value) {
    return Math.ceil(value / GRID) * GRID;
  }

  function create(tag, attributes, parent, text) {
    const element = document.createElementNS(SVG_NS, tag);
    for (const [name, value] of Object.entries(attributes || {})) element.setAttribute(name, value);
    if (text !== undefined) element.textContent = text;
    if (parent) parent.appendChild(element);
    return element;
  }

  function wrap(label) {
    const words = String(label).split(/\s+/).filter(Boolean);
    const lines = [""];
    for (const word of words) {
      const line = lines[lines.length - 1];
      if (line && (line + " " + word).length > WRAP_CHARS) lines.push(word);
      else lines[lines.length - 1] = line ? line + " " + word : word;
    }
    if (lines.length > 2) lines.splice(2, lines.length, lines.slice(1).join(" ").slice(0, WRAP_CHARS - 1) + "…");
    return lines;
  }

  function textWidth(text, px, em) {
    return text.length * px * em;
  }

  function measureNode(node) {
    const lines = wrap(node.label);
    const eyebrow = EYEBROWS[node.shape] || "";
    const widest = Math.max(...lines.map((line) => textWidth(line, LABEL_PX, SANS_EM)), textWidth(eyebrow, 7, MONO_EM * 1.3));
    let width = Math.max(NODE_MIN_WIDTH, snap(widest + 2 * NODE_PAD_X));
    let height = snap((BASE_HEIGHT[node.shape] || 40) + (lines.length - 1) * LINE_HEIGHT + (eyebrow ? 8 : 0));
    if (node.shape === "decision") {
      width = snap(width * 1.4);
      height = snap(height * 1.3);
    }
    return { lines, eyebrow, width, height };
  }

  function drawShape(group, shape, w, h) {
    const x = -w / 2, y = -h / 2;
    if (shape === "database") {
      const ry = 6;
      create("path", { class: "sk-shape", d: `M${x},${y + ry} A${w / 2},${ry} 0 0 1 ${x + w},${y + ry} V${y + h - ry} A${w / 2},${ry} 0 0 1 ${x},${y + h - ry} Z` }, group);
      create("path", { class: "sk-detail", d: `M${x},${y + ry} A${w / 2},${ry} 0 0 0 ${x + w},${y + ry}` }, group);
    } else if (shape === "queue") {
      create("rect", { class: "sk-shape", x, y, width: w, height: h, rx: 6 }, group);
      for (const offset of [10, 16, 22]) create("line", { class: "sk-detail", x1: x + w - offset, y1: y + 6, x2: x + w - offset, y2: y + h - 6 }, group);
    } else if (shape === "cache") {
      create("rect", { class: "sk-shape", x: x + 4, y: y - 4, width: w, height: h, rx: 6 }, group);
      create("rect", { class: "sk-shape", x, y, width: w, height: h, rx: 6 }, group);
    } else if (shape === "decision") {
      create("path", { class: "sk-shape", d: `M0,${y} L${x + w},0 L0,${y + h} L${x},0 Z` }, group);
    } else if (shape === "state") {
      create("rect", { class: "sk-shape", x, y, width: w, height: h, rx: 16 }, group);
    } else if (shape === "actor") {
      create("rect", { class: "sk-shape", x, y, width: w, height: h, rx: h / 2 }, group);
    } else if (shape === "client") {
      create("rect", { class: "sk-shape", x, y, width: w, height: h, rx: 6 }, group);
      create("line", { class: "sk-detail", x1: x, y1: y + 8, x2: x + w, y2: y + 8 }, group);
    } else if (shape === "note") {
      create("path", { class: "sk-shape", d: `M${x},${y} H${x + w - 10} L${x + w},${y + 10} V${y + h} H${x} Z` }, group);
    } else {
      create("rect", { class: "sk-shape", x, y, width: w, height: h, rx: 6 }, group);
    }
  }

  function drawNode(layer, node, box) {
    const group = create("g", { class: "sk-node" + (node.focal ? " is-focal" : ""), "data-id": node.id, "data-shape": node.shape, transform: `translate(${box.x},${box.y})` }, layer);
    create("rect", { class: "sk-halo", x: -box.width / 2 - 8, y: -box.height / 2 - 8, width: box.width + 16, height: box.height + 16, rx: 10 }, group);
    drawShape(group, node.shape, box.width, box.height);
    create("rect", { class: "sk-outline", x: -box.width / 2 - 4, y: -box.height / 2 - 4, width: box.width + 8, height: box.height + 8, rx: 8 }, group);
    const textTop = -((box.lines.length - 1) * LINE_HEIGHT) / 2 + (box.eyebrow ? 6 : 0);
    if (box.eyebrow) create("text", { class: "sk-eyebrow", x: 0, y: textTop - 13, "text-anchor": "middle" }, group, box.eyebrow);
    box.lines.forEach((line, index) => {
      create("text", { class: "sk-label", x: 0, y: textTop + index * LINE_HEIGHT + 4, "text-anchor": "middle" }, group, line);
    });
  }

  function defineMarkers(svg) {
    const defs = create("defs", {}, svg);
    const markers = [
      ["sk-arrow", "path", { class: "sk-marker-fill", d: "M0,0 L8,3 L0,6 Z" }],
      ["sk-arrow-ink", "path", { class: "sk-marker-ink", d: "M0,0 L8,3 L0,6 Z" }],
      ["sk-arrow-open", "polyline", { class: "sk-marker-open", points: "0 0, 8 3, 0 6" }],
      ["sk-arrow-soft", "polyline", { class: "sk-marker-soft", points: "0 0, 8 3, 0 6" }],
    ];
    for (const [id, tag, attributes] of markers) {
      const marker = create("marker", { id, markerWidth: 8, markerHeight: 6, refX: 7, refY: 3, orient: "auto", markerUnits: "userSpaceOnUse" }, defs);
      create(tag, attributes, marker);
    }
  }

  const MARKER_BY_STYLE = { async: "sk-arrow-open", dependency: "sk-arrow-soft", transition: "sk-arrow-ink" };

  function roundedPath(points) {
    let d = `M${points[0].x},${points[0].y}`;
    for (let i = 1; i < points.length - 1; i++) {
      const previous = points[i - 1], corner = points[i], next = points[i + 1];
      const before = Math.min(CORNER, distance(previous, corner) / 2);
      const after = Math.min(CORNER, distance(corner, next) / 2);
      const start = towards(corner, previous, before), end = towards(corner, next, after);
      d += ` L${start.x},${start.y} Q${corner.x},${corner.y} ${end.x},${end.y}`;
    }
    const last = points[points.length - 1];
    return d + ` L${last.x},${last.y}`;
  }

  function distance(a, b) {
    return Math.hypot(b.x - a.x, b.y - a.y);
  }

  function towards(from, to, length) {
    const total = distance(from, to) || 1;
    return { x: from.x + ((to.x - from.x) * length) / total, y: from.y + ((to.y - from.y) * length) / total };
  }

  function ports(a, b, vertical) {
    const gapX = Math.abs(b.x - a.x) - (a.width + b.width) / 2;
    const gapY = Math.abs(b.y - a.y) - (a.height + b.height) / 2;
    const horizontal = vertical ? gapY < 8 && gapX > 0 : gapX >= 8 || gapY < 8;
    if (horizontal) {
      const sign = b.x >= a.x ? 1 : -1;
      return { horizontal, start: { x: a.x + (sign * a.width) / 2, y: a.y }, end: { x: b.x - (sign * b.width) / 2, y: b.y } };
    }
    const sign = b.y >= a.y ? 1 : -1;
    return { horizontal, start: { x: a.x, y: a.y + (sign * a.height) / 2 }, end: { x: b.x, y: b.y - (sign * b.height) / 2 } };
  }

  function elbow(start, end, horizontal) {
    if (horizontal) {
      if (Math.abs(start.y - end.y) < 1) return [start, end];
      const mid = (start.x + end.x) / 2;
      return [start, { x: mid, y: start.y }, { x: mid, y: end.y }, end];
    }
    if (Math.abs(start.x - end.x) < 1) return [start, end];
    const mid = (start.y + end.y) / 2;
    return [start, { x: start.x, y: mid }, { x: end.x, y: mid }, end];
  }

  function routeEdge(a, b, waypoints, vertical) {
    const { horizontal, start, end } = ports(a, b, vertical);
    if (waypoints.length < 2) return elbow(start, end, horizontal);
    const lane = [start, ...waypoints, end];
    const points = [start];
    for (let i = 1; i < lane.length; i++) points.push(...elbow(points[points.length - 1], lane[i], horizontal).slice(1));
    return points;
  }

  function selfLoop(box) {
    const x = box.x, top = box.y - box.height / 2;
    return `M${x - 12},${top} C${x - 12},${top - 32} ${x + 12},${top - 32} ${x + 12},${top}`;
  }

  function drawEdge(layer, edge, d) {
    const group = create("g", { class: "sk-edge", "data-id": edge.id, "data-style": edge.style }, layer);
    create("path", { class: "sk-hit", d }, group);
    const line = create("path", { class: "sk-line", d, "marker-end": `url(#${MARKER_BY_STYLE[edge.style] || "sk-arrow"})` }, group);
    if (edge.label) drawEdgeLabel(group, edge.label, line.getPointAtLength(line.getTotalLength() / 2));
    return group;
  }

  function drawEdgeLabel(group, label, at) {
    const width = snap(textWidth(label, EDGE_LABEL_PX, MONO_EM) + 8);
    create("rect", { class: "sk-edge-label-bg", x: at.x - width / 2, y: at.y - 7, width, height: 12, rx: 2 }, group);
    create("text", { class: "sk-edge-label", x: at.x, y: at.y + 2, "text-anchor": "middle" }, group, label);
  }

  function drawZone(layer, group, box) {
    const label = group.label.toUpperCase();
    create("rect", { class: "sk-zone", x: box.x - box.width / 2, y: box.y - box.height / 2, width: box.width, height: box.height, rx: 8 }, layer);
    const width = snap(textWidth(label, 7, MONO_EM * 1.25) + 12);
    create("rect", { class: "sk-zone-label-bg", x: box.x - width / 2, y: box.y - box.height / 2 + 4, width, height: 12, rx: 2 }, layer);
    create("text", { class: "sk-zone-label", x: box.x, y: box.y - box.height / 2 + 13, "text-anchor": "middle" }, layer, label);
  }

  function enclosingBox(members) {
    const left = Math.min(...members.map((box) => box.x - box.width / 2)) - 16;
    const right = Math.max(...members.map((box) => box.x + box.width / 2)) + 16;
    const top = Math.min(...members.map((box) => box.y - box.height / 2)) - 28;
    const bottom = Math.max(...members.map((box) => box.y + box.height / 2)) + 16;
    return { x: (left + right) / 2, y: (top + bottom) / 2, width: right - left, height: bottom - top };
  }

  function layoutGraph(diagram) {
    const graph = new dagre.graphlib.Graph({ compound: true, multigraph: true });
    graph.setGraph({ rankdir: RANK_DIRECTION[diagram.kind] || "LR", nodesep: 28, ranksep: 48, edgesep: 16, marginx: MARGIN + 16, marginy: MARGIN + 28 });
    graph.setDefaultEdgeLabel(() => ({}));
    const boxes = new Map();
    for (const node of diagram.nodes) {
      const box = measureNode(node);
      boxes.set(node.id, box);
      graph.setNode(node.id, { width: box.width, height: box.height });
    }
    const grouped = new Set();
    for (const group of diagram.groups) {
      graph.setNode("zone:" + group.id, { paddingTop: 24 });
      for (const member of group.members) {
        if (boxes.has(member) && !grouped.has(member)) {
          graph.setParent(member, "zone:" + group.id);
          grouped.add(member);
        }
      }
    }
    for (const edge of diagram.edges) {
      if (edge.source === edge.target) continue;
      const label = edge.label ? { width: snap(textWidth(edge.label, EDGE_LABEL_PX, MONO_EM) + 8), height: 12, labelpos: "c" } : {};
      graph.setEdge(edge.source, edge.target, label, edge.id);
    }
    dagre.layout(graph);
    for (const [id, box] of boxes) Object.assign(box, { x: graph.node(id).x, y: graph.node(id).y });
    const zones = diagram.groups
      .map((group) => ({ group, members: graph.children("zone:" + group.id) || [] }))
      .filter(({ members }) => members.length)
      .map(({ group, members }) => ({ group, box: enclosingBox(members.map((id) => boxes.get(id))) }));
    const routes = new Map();
    for (const edge of diagram.edges) {
      const a = boxes.get(edge.source), b = boxes.get(edge.target);
      if (edge.source === edge.target) {
        routes.set(edge.id, selfLoop(a));
        continue;
      }
      const points = graph.edge({ v: edge.source, w: edge.target, name: edge.id }).points || [];
      const waypoints = points.length > 3 ? points.slice(1, -1) : [];
      routes.set(edge.id, roundedPath(routeEdge(a, b, waypoints, graph.graph().rankdir === "TB")));
    }
    return { boxes, zones, routes, width: graph.graph().width, height: graph.graph().height };
  }

  function renderGraph(svg, diagram) {
    const layout = layoutGraph(diagram);
    const zoneLayer = create("g", { class: "sk-zones" }, svg);
    const edgeLayer = create("g", { class: "sk-edges" }, svg);
    const nodeLayer = create("g", { class: "sk-nodes" }, svg);
    for (const { group, box } of layout.zones) drawZone(zoneLayer, group, box);
    for (const edge of diagram.edges) drawEdge(edgeLayer, edge, layout.routes.get(edge.id));
    for (const node of diagram.nodes) drawNode(nodeLayer, node, layout.boxes.get(node.id));
    return { width: layout.width, height: layout.height };
  }

  function renderSequence(svg, diagram) {
    const boxes = diagram.nodes.map((node) => Object.assign(measureNode(node), { node }));
    const column = Math.max(...boxes.map((box) => box.width), 96) + 48;
    const longestLabel = Math.max(0, ...diagram.edges.map((edge) => textWidth(edge.label || "", EDGE_LABEL_PX, MONO_EM)));
    const spacing = Math.max(column, snap(longestLabel + 48));
    const top = MARGIN, headHeight = Math.max(...boxes.map((box) => box.height));
    const firstMessage = top + headHeight + 36, step = 36;
    const bottom = firstMessage + Math.max(diagram.edges.length - 1, 0) * step + 32;
    const centers = new Map();
    boxes.forEach((box, index) => {
      Object.assign(box, { x: MARGIN + spacing / 2 + index * spacing, y: top + headHeight / 2 });
      centers.set(box.node.id, box.x);
    });
    const lifelines = create("g", { class: "sk-lifelines" }, svg);
    for (const box of boxes) create("line", { class: "sk-lifeline", x1: box.x, y1: box.y + box.height / 2, x2: box.x, y2: bottom }, lifelines);
    const edgeLayer = create("g", { class: "sk-edges" }, svg);
    diagram.edges.forEach((edge, index) => {
      const y = firstMessage + index * step, from = centers.get(edge.source), to = centers.get(edge.target);
      const d = edge.source === edge.target
        ? `M${from},${y - 8} H${from + 32} V${y + 8} H${from + 4}`
        : `M${from},${y} H${to + (to > from ? -2 : 2)}`;
      const group = drawEdge(edgeLayer, Object.assign({}, edge, { label: "" }), d);
      if (edge.label) drawEdgeLabel(group, edge.label, { x: edge.source === edge.target ? from + 32 + textWidth(edge.label, EDGE_LABEL_PX, MONO_EM) / 2 + 8 : (from + to) / 2, y: y - 9 });
    });
    const nodeLayer = create("g", { class: "sk-nodes" }, svg);
    for (const box of boxes) drawNode(nodeLayer, box.node, box);
    return { width: MARGIN * 2 + spacing * boxes.length, height: bottom + MARGIN };
  }

  function renderEmpty(svg) {
    create("text", { class: "sk-empty", x: 240, y: 135, "text-anchor": "middle" }, svg, "Start explaining — the diagram appears here.");
    return { width: 480, height: 270 };
  }

  /**
   * Draws `diagram` ({kind, nodes, edges, groups, selected, pointed}) into `svg`, replacing its content.
   * Returns the time the layout and drawing took in milliseconds.
   */
  function render(svg, diagram) {
    const started = performance.now();
    while (svg.firstChild) svg.removeChild(svg.firstChild);
    defineMarkers(svg);
    let size;
    if (!diagram.nodes.length) size = renderEmpty(svg);
    else if (diagram.kind === "sequence") size = renderSequence(svg, diagram);
    else size = renderGraph(svg, diagram);
    svg.setAttribute("viewBox", `0 0 ${Math.ceil(size.width)} ${Math.ceil(size.height)}`);
    svg.setAttribute("preserveAspectRatio", "xMidYMin meet");
    svg.style.maxWidth = `${Math.ceil(size.width * MAX_SCALE)}px`;
    svg.style.maxHeight = `${Math.ceil(size.height * MAX_SCALE)}px`;
    mark(svg, diagram.selected || [], diagram.pointed || null);
    return performance.now() - started;
  }

  /** Shows the selection (solid outline) and the pointed-at element (light highlight) without a re-layout. */
  function mark(svg, selected, pointed) {
    const chosen = new Set(selected);
    for (const element of svg.querySelectorAll("[data-id]")) {
      const id = element.getAttribute("data-id");
      element.classList.toggle("is-selected", chosen.has(id));
      element.classList.toggle("is-pointed", id === pointed);
    }
  }

  /** Client-pixel bounding boxes of every drawn element, keyed by element id. */
  function boxes(svg) {
    const result = {};
    for (const element of svg.querySelectorAll("[data-id]")) {
      const rect = (element.querySelector(".sk-shape, .sk-line") || element).getBoundingClientRect();
      result[element.getAttribute("data-id")] = { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
    }
    return result;
  }

  global.SketchRenderer = { render, mark, boxes };
})(window);
