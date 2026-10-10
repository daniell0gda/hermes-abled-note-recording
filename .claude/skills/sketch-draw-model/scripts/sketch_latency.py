"""Phase 0 prototype: routing + drawing + styling latency, parse errors, invented nodes.

usage: python sketch_latency.py <lines|mermaid|svg> [runs] [--style] [--model M]
"""
import json
import re
import statistics
import subprocess
import sys
import time
import winreg
from pathlib import Path

import httpx

from listening_app.config import load_config, resolve_config_path
from listening_app.sketch.drawer import Credentials, credentials_provider, litellm_completion

GEN = Path(__file__).resolve().parents[4] / ".gen"
ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
FORMAT = ARGS[0] if ARGS else "lines"
RUNS = int(ARGS[1]) if len(ARGS) > 1 else 1
STYLE = "--style" in sys.argv
DRAW_MODEL = sys.argv[sys.argv.index("--model") + 1] if "--model" in sys.argv else "xai/grok-4-1-fast-non-reasoning"
ROUTE_CONFIDENCE = 0.4

SEGMENTS = [
    "So let me walk you through how a request gets into our system.",
    "First the browser calls the API gateway.",
    "The gateway checks the token with the auth service.",
    "If the token is valid, the request goes to the order service.",
    "The order service writes the order into Postgres.",
    "Then it publishes an order created event to Kafka.",
    "Yeah, sorry, can you hear me okay? Good.",
    "The billing service listens to that event and charges the card.",
    "And the notification service also listens and sends an email.",
    "Now this one here, the auth service, actually caches tokens in Redis.",
    "Okay, let's start a new diagram for the order lifecycle.",
    "An order starts as draft.",
    "When the customer pays it becomes paid.",
    "From paid it can go to shipped, or to refunded if payment is reversed.",
    "Shipped goes to delivered when the courier confirms.",
    "Let me go back to diagram 1 for a second.",
    "Between the gateway and the order service there is a rate limiter.",
    "Back to the lifecycle one, a draft can also expire after 24 hours.",
    "Expired is a final state, same as delivered and refunded.",
    "That's basically it, any questions?",
]
INVENTED = {"request", "entry point", "submitted", "start", "end", "client request", "user"}

TYPES = {
    "flow": "How components or steps connect: system architecture, data flow, pipelines, processes",
    "sequence": "The back-and-forth order of messages between a few participants, e.g. a protocol or handshake",
    "state": "The lifecycle or statuses of one thing and the transitions between them",
    "tree": "Hierarchy, parent and child decomposition",
    "er": "Entities and their relationships",
}
KIND_HINTS = {
    "flow": "Nodes are components or steps; edges are calls or data flow.",
    "sequence": "Nodes are participants; every edge is one message, listed in the order the messages happen.",
    "state": "Nodes are states; edges are transitions labelled with their trigger.",
    "tree": "Edges go from parent to child.",
    "er": "Nodes are entities; edges are relationships labelled with their meaning and cardinality.",
}
SHAPES = {
    "service": "An application service, server, API or backend component that runs code",
    "database": "A database or persistent data store, e.g. Postgres, MySQL, MongoDB, S3",
    "queue": "A message queue, topic, stream or event bus, e.g. Kafka, RabbitMQ, SQS",
    "cache": "An in-memory cache, e.g. Redis, Memcached",
    "actor": "A person or role, e.g. user, customer, admin, courier",
    "external": "A third-party or external system outside the speaker's own system",
    "client": "A client application or UI, e.g. browser, mobile app, web frontend",
    "state": "A state or status of a thing, e.g. draft, paid, shipped",
    "decision": "A decision or condition with alternative outcomes",
    "note": "A remark or annotation rather than a component",
}
EDGE_STYLES = {
    "sync": "A synchronous call or request that expects a reply",
    "async": "An asynchronous event or message, published or consumed, fire-and-forget",
    "data": "Reading or writing data to a store",
    "transition": "A transition from one state to another",
    "dependency": "A dependency, ownership or structural relationship",
}

NODE_LINE = re.compile(r'^([a-z][a-z0-9_]*)\s+"([^"]+)"$')
EDGE_LINE = re.compile(r'^([a-z][a-z0-9_]*)\s*->\s*([a-z][a-z0-9_]*)(?:\s+"([^"]*)")?$')
GROUP_LINE = re.compile(r'^group\s+([a-z][a-z0-9_]*)(?:\s+"([^"]+)")?\s*:\s*([a-z0-9_,\s]+)$')
TITLE_LINE = re.compile(r'^title\s+"([^"]+)"$')


def parse_lines(text: str) -> dict:
    nodes, edges, groups, title, errors = {}, [], [], "", []
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("```"):
            continue
        if m := TITLE_LINE.match(line):
            title = m[1]
        elif m := GROUP_LINE.match(line):
            groups.append({"id": m[1], "label": m[2] or m[1], "members": [p.strip() for p in m[3].split(",") if p.strip()]})
        elif m := EDGE_LINE.match(line):
            edges.append({"source": m[1], "target": m[2], "label": m[3] or ""})
        elif m := NODE_LINE.match(line):
            nodes[m[1]] = m[2]
        else:
            errors.append(line)
    for edge in edges:
        for end in (edge["source"], edge["target"]):
            nodes.setdefault(end, end.replace("_", " ").capitalize())
    return {"title": title, "nodes": nodes, "edges": edges, "groups": groups, "errors": errors}


def typesafe_key() -> str:
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        return winreg.QueryValueEx(key, "TYPESAFE_API_KEY")[0]


def ask_jev(client: httpx.Client, state: dict, questions: dict) -> dict:
    response = client.post("https://api.typesafe.ai/v1/systemone",
                           json={"model": "jev-latest", "state": state, "questions": questions})
    response.raise_for_status()
    return response.json()["answers"]


def route(client: httpx.Client, state: dict, diagrams: list[dict]) -> dict:
    target = {f"d{d['number']}": f"Diagram {d['number']}: {d['title']} (nodes: {', '.join(d['labels']) or 'none'})"
              for d in diagrams}
    target["new"] = "A different subject that none of the existing diagrams covers"
    return ask_jev(client, state, {
        "structural": {"type": "noul",
                       "instructions": "Does `new_segment` describe components, steps, states or relationships that belong in a diagram?",
                       "criteria": {"true": "Adds or changes structure", "false": "Small talk, filler, navigation or questions to the audience"}},
        "new_diagram": {"type": "noul",
                        "instructions": "In `new_segment`, does the speaker explicitly ask to start a new, separate diagram?",
                        "criteria": {"true": "Explicitly asks for a new or another diagram",
                                     "false": "Continues or returns to an existing diagram, or does not mention diagrams"}},
        "target": {"type": "choice",
                   "instructions": "Which diagram is the speaker talking about in `new_segment`? They keep talking about "
                                   "`current_diagram` unless they name another diagram, return to one, or clearly change the subject.",
                   "criteria": target},
        "kind": {"type": "choice",
                 "instructions": "If `new_segment` starts a new diagram, which diagram type fits what the speaker explains "
                                 "in `previous_segments` and `new_segment`?",
                 "criteria": TYPES},
    })


def style(client: httpx.Client, diagram: dict, new_nodes: list[str], new_edges: list[int]) -> dict:
    structure = diagram["structure"]
    state = {"diagram_type": diagram["kind"], "transcript": diagram["transcript"],
             "nodes": [{"id": k, "label": v} for k, v in structure["nodes"].items()],
             "edges": [{"index": i, **e} for i, e in enumerate(structure["edges"])]}
    questions = {}
    for node in new_nodes:
        label = structure["nodes"][node]
        questions[f"shape:{node}"] = {"type": "choice", "criteria": SHAPES,
                                      "instructions": f"What kind of element is the node `{node}` (\"{label}\") in this diagram?"}
        questions[f"focal:{node}"] = {"type": "noul",
                                      "instructions": f"Does the speaker in `transcript` stress the node `{node}` (\"{label}\") as the central or most important element of the diagram?",
                                      "criteria": {"true": "Explicitly stressed as central or most important", "false": "One element among others"}}
    for index in new_edges:
        edge = structure["edges"][index]
        questions[f"edge:{index}"] = {"type": "choice", "criteria": EDGE_STYLES,
                                      "instructions": f"What kind of connection is `edges[{index}]` from `{edge['source']}` to `{edge['target']}` (\"{edge['label']}\")?"}
    return ask_jev(client, state, questions) if questions else {}


def draw_prompt(diagram: dict, new: str) -> str:
    kind = diagram["kind"]
    transcript = "\n".join(diagram["transcript"]) or "(nothing yet)"
    if FORMAT == "mermaid":
        return (
            f"You keep a live {kind} diagram in sync with what a speaker explains out loud, in Mermaid syntax.\n"
            "Draw only what the speaker actually said: no nodes, steps, states or labels they did not mention, "
            "no invented start, entry or request nodes. Keep existing node ids. Max 12 nodes.\n"
            "Return ONLY the complete updated Mermaid code, no fences, no prose.\n\n"
            f"Current diagram:\n{diagram['text'] or '(empty)'}\n\nTranscript so far:\n{transcript}\n\nNew lines:\n{new}"
        )
    return (
        f"You keep a live {kind} diagram in sync with what a speaker explains out loud.\n"
        "Draw only what the speaker actually said: no nodes, steps, states or labels they did not mention, "
        "no invented start, entry or request nodes.\n"
        "Format, one item per line:\n"
        'title "Short diagram title"\n'
        '<id> "<Label>"\n'
        '<id> -> <id> "<edge label>"   (the label is optional)\n'
        'group <id> "<Label>": <id>, <id>\n'
        "Ids: short descriptive names in lowercase letters, digits and _, e.g. auth or order_db. "
        "Keep the ids of existing elements. Max 12 nodes. "
        f"{KIND_HINTS[kind]}\n"
        "Return ONLY the complete updated diagram, no fences, no prose.\n\n"
        f"Current diagram:\n{diagram['text'] or '(empty)'}\n\nTranscript so far:\n{transcript}\n\nNew lines:\n{new}"
    )


def draw(credentials: Credentials, diagram: dict, new: str) -> str:
    return litellm_completion(DRAW_MODEL, [{"role": "user", "content": draw_prompt(diagram, new)}], credentials)


def mermaid_errors(sources: list[str]) -> list[str | None]:
    result = subprocess.run(["node", str(GEN / "mermaid-check" / "check.mjs")], input=json.dumps(sources),
                            capture_output=True, text=True, encoding="utf-8", check=True)
    return json.loads(result.stdout)


def report(name: str, values: list[float]) -> str:
    if not values:
        return f"{name:6} n= 0"
    return f"{name:6} n={len(values):2} median={statistics.median(values):5.2f}s max={max(values):5.2f}s"


def run_once(client: httpx.Client, credentials: Credentials) -> dict:
    diagrams: list[dict] = []
    current: dict | None = None
    times = {"jev": [], "draw": [], "style": [], "total": []}
    outputs: list[str] = []
    log: list[str] = []
    for index, segment in enumerate(SEGMENTS):
        state = {"new_segment": segment, "previous_segments": SEGMENTS[max(0, index - 3):index],
                 "current_diagram": current["number"] if current else None}
        start = time.perf_counter()
        answers = route(client, state, diagrams)
        times["jev"].append(time.perf_counter() - start)
        structural, new_requested = answers["structural"]["noul"], answers["new_diagram"]["noul"]
        target, confidence = answers["target"]["choice"], answers["target"]["confidence"]
        line = (f"{index + 1:2} jev={times['jev'][-1]:.2f}s struct={structural:.2f} new={new_requested:.2f} "
                f"target={target}({confidence:.2f})")
        if new_requested < 0.5 and structural < 0.5:
            log.append(line + " skip")
            continue
        if new_requested >= 0.5 or not diagrams or (target == "new" and confidence >= ROUTE_CONFIDENCE):
            current = {"number": len(diagrams) + 1, "title": "", "kind": answers["kind"]["choice"], "labels": [],
                       "text": "", "transcript": [], "structure": {"nodes": {}, "edges": []}}
            diagrams.append(current)
        elif target != "new":
            current = next(d for d in diagrams if f"d{d['number']}" == target)
        low = " LOW" if confidence < ROUTE_CONFIDENCE else ""
        draw_start = time.perf_counter()
        text = draw(credentials, current, segment)
        times["draw"].append(time.perf_counter() - draw_start)
        outputs.append(text)
        current["transcript"].append(segment)
        current["text"] = text
        style_info = ""
        if FORMAT == "lines":
            previous = current["structure"]
            parsed = parse_lines(text)
            if parsed["errors"]:
                style_info = f" PARSE-ERR {parsed['errors'][:2]}"
            current["structure"] = parsed
            current["title"] = parsed["title"] or current["title"]
            current["labels"] = list(parsed["nodes"].values())
            if STYLE:
                new_nodes = [n for n, label in parsed["nodes"].items() if previous["nodes"].get(n) != label]
                old_edges = {(e["source"], e["target"], e["label"]) for e in previous["edges"]}
                new_edges = [i for i, e in enumerate(parsed["edges"]) if (e["source"], e["target"], e["label"]) not in old_edges]
                style_start = time.perf_counter()
                styled = style(client, current, new_nodes, new_edges)
                times["style"].append(time.perf_counter() - style_start)
                current.setdefault("styles", {}).update(styled)
                style_info += f" style={times['style'][-1]:.2f}s q={len(styled)}"
        else:
            current["labels"] = re.findall(r"\w+[\[\(\{]+\"?([^\]\)\}\"]+)", text)[:12]
        times["total"].append(time.perf_counter() - start)
        log.append(line + f"{low} -> d{current['number']} {current['kind']} draw={times['draw'][-1]:.2f}s{style_info}")
    if FORMAT == "mermaid":
        errors = mermaid_errors(outputs)
    else:
        errors = [("empty answer" if not o.strip() else "; ".join(parse_lines(o)["errors"]) or None) for o in outputs]
    return {"times": times, "errors": errors, "outputs": outputs, "log": log, "diagrams": diagrams}


def main() -> None:
    config = load_config(resolve_config_path())
    config = config.model_copy(update={"sketch": config.sketch.model_copy(update={"draw_model": DRAW_MODEL})})
    credentials = credentials_provider(config)()
    client = httpx.Client(headers={"Authorization": f"Bearer {typesafe_key()}"}, timeout=30)
    totals = {"jev": [], "draw": [], "style": [], "total": []}
    error_count = draw_count = 0
    for run in range(RUNS):
        result = run_once(client, credentials)
        print(f"--- run {run + 1} ({FORMAT}, {DRAW_MODEL})")
        print("\n".join(result["log"]))
        for key in totals:
            totals[key] += result["times"][key]
        error_count += sum(e is not None for e in result["errors"])
        draw_count += len(result["errors"])
        for error, output in zip(result["errors"], result["outputs"]):
            if error:
                print(f"ERROR: {error}\n{output}\n")
        for d in result["diagrams"]:
            labels = d["labels"]
            invented = [label for label in labels if label.lower() in INVENTED]
            print(f"d{d['number']} {d['kind']} '{d['title']}' nodes={labels} invented={invented}")
            for key, answer in d.get("styles", {}).items():
                value = answer.get("choice") or f"{answer.get('noul', 0):.2f}"
                print(f"   {key:28} {value} {answer.get('confidence', ''):}")
            suffix = {"mermaid": "mmd", "lines": "txt", "svg": "svg"}[FORMAT]
            (GEN / f"sketch_d{d['number']}.{suffix}").write_text(d["text"], encoding="utf-8")
            if FORMAT == "lines":
                (GEN / f"sketch_d{d['number']}.json").write_text(json.dumps(
                    {k: d.get(k) for k in ("number", "title", "kind", "structure", "styles", "transcript")}, indent=1),
                    encoding="utf-8")
    print(f"=== {FORMAT} x{RUNS}")
    for key, values in totals.items():
        print(report(key, values))
    print(f"parse errors: {error_count}/{draw_count}")


if __name__ == "__main__":
    main()
