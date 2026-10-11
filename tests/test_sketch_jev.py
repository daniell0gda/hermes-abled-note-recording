import json
from typing import Any

import httpx
import pytest

from listening_app.sketch.diagrams import (
    Changes,
    Destination,
    Diagram,
    DiagramKind,
    DiagramSet,
    EdgeStyle,
    NodeStyle,
    Shape,
)
from listening_app.sketch.jev_client import JEV_URL, NO_FOCUS, JevClient, JevError, Routing
from listening_app.sketch.structure import parse_structure


class FakeJev:
    def __init__(self, answers: dict[str, Any], status: int = 200) -> None:
        self.answers = answers
        self.status = status
        self.requests: list[httpx.Request] = []

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        return httpx.Response(self.status, json={"model": "jev-1.13.0", "answers": self.answers})

    def body(self) -> dict[str, Any]:
        result: dict[str, Any] = json.loads(self.requests[-1].content)
        return result


def client(fake: FakeJev) -> JevClient:
    return JevClient("ts-key", "jev-latest", httpx.MockTransport(fake))


def routing_answers(structural: float = 0.9, new: float = 0.02, target: str = "d1", confidence: float = 0.9,
                    kind: str = "flow", returns: float = 0.04) -> dict[str, Any]:
    return {"structural": {"type": "noul", "noul": structural},
            "new_diagram": {"type": "noul", "noul": new},
            "returns": {"type": "noul", "noul": returns},
            "target": {"type": "choice", "choice": target, "confidence": confidence, "probabilities": {}},
            "kind": {"type": "choice", "choice": kind, "confidence": 0.8, "probabilities": {}}}


def two_diagrams() -> DiagramSet:
    diagrams = DiagramSet()
    first = diagrams.route(None, DiagramKind.FLOW)
    assert first is not None
    first.update(parse_structure('title "Request path"\nauth "Auth service"'))
    diagrams.route(None, DiagramKind.STATE)
    return diagrams


def test_route_request_sends_the_segment_context_and_one_option_per_diagram() -> None:
    fake = FakeJev(routing_answers())

    client(fake).route("It caches tokens in Redis.", ["The gateway calls auth."], two_diagrams().overview())

    request = fake.requests[-1]
    body = fake.body()
    assert str(request.url) == JEV_URL
    assert request.headers["Authorization"] == "Bearer ts-key"
    assert body["model"] == "jev-latest"
    assert body["state"] == {"new_segment": "It caches tokens in Redis.",
                             "previous_segments": ["The gateway calls auth."], "current_diagram": "d2"}
    assert set(body["questions"]) == {"structural", "new_diagram", "returns", "target", "kind"}
    assert body["questions"]["structural"]["type"] == "noul"
    assert body["questions"]["target"]["criteria"]["d1"] == "Diagram 1: Request path (nodes: Auth service)"
    assert body["questions"]["target"]["criteria"]["d2"] == "Diagram 2: untitled (nodes: none)"
    assert "new" in body["questions"]["target"]["criteria"]
    assert set(body["questions"]["kind"]["criteria"]) == {kind.value for kind in DiagramKind}


def test_route_answers_become_a_typed_routing() -> None:
    fake = FakeJev(routing_answers(structural=0.93, new=0.03, target="d2", confidence=0.71, kind="state", returns=0.08))

    routing = client(fake).route("A draft can expire.", [], two_diagrams().overview())

    assert routing == Routing(structural=0.93, new_requested=0.03, target=2, confidence=0.71, kind=DiagramKind.STATE,
                              returns=0.08)


def test_new_target_means_no_diagram_number() -> None:
    routing = client(FakeJev(routing_answers(target="new"))).route("x", [], two_diagrams().overview())

    assert routing.target is None


@pytest.mark.parametrize(("routing", "destination"), [
    (Routing(0.9, 0.02, 2, 0.9, DiagramKind.FLOW), Destination.existing(2)),
    (Routing(0.9, 0.02, 1, 0.9, DiagramKind.FLOW), Destination.existing(2)),
    (Routing(0.9, 0.02, 1, 0.9, DiagramKind.FLOW, returns=0.9), Destination.existing(1)),
    (Routing(0.9, 0.02, None, 0.9, DiagramKind.STATE), Destination.new(DiagramKind.STATE)),
    (Routing(0.9, 0.02, None, 0.6, DiagramKind.STATE), Destination.by_drawer()),
    (Routing(0.2, 0.96, 1, 0.7, DiagramKind.STATE), Destination.new(DiagramKind.STATE)),
    (Routing(0.9, 0.02, 1, 0.3, DiagramKind.FLOW), Destination.existing(2)),
    (Routing(0.2, 0.02, 1, 0.9, DiagramKind.FLOW), None),
])
def test_routing_decides_the_destination(routing: Routing, destination: Destination | None) -> None:
    assert routing.destination(active=2) == destination


@pytest.mark.parametrize(("routing", "switch"), [
    (Routing(0.05, 0.02, 1, 0.92, DiagramKind.FLOW, returns=0.9), 1),
    (Routing(0.05, 0.02, 1, 0.92, DiagramKind.FLOW), None),
    (Routing(0.05, 0.02, 2, 0.92, DiagramKind.FLOW, returns=0.9), None),
    (Routing(0.05, 0.02, 1, 0.45, DiagramKind.FLOW, returns=0.9), None),
    (Routing(0.05, 0.02, None, 0.92, DiagramKind.FLOW, returns=0.9), None),
])
def test_only_an_explicit_return_to_another_diagram_switches_to_it(routing: Routing, switch: int | None) -> None:
    assert routing.switches_to(active=2) == switch


def test_the_first_structural_segment_opens_a_diagram_even_with_low_confidence() -> None:
    routing = Routing(0.9, 0.02, None, 0.2, DiagramKind.SEQUENCE)

    assert routing.destination(active=None) == Destination.new(DiagramKind.SEQUENCE)


def test_style_asks_shape_and_focus_per_new_node_and_style_per_new_edge() -> None:
    diagram = Diagram(1, DiagramKind.FLOW, transcript=["Orders are written to Postgres."])
    diagram.update(parse_structure('orders "Order service"\npg "Postgres"\norders -> pg "writes"'))
    fake = FakeJev({"shape:pg": {"type": "choice", "choice": "database", "confidence": 1.0, "probabilities": {}},
                    "focus": {"type": "choice", "choice": "pg", "confidence": 0.82, "probabilities": {}},
                    "edge:orders->pg": {"type": "choice", "choice": "data", "confidence": 0.9, "probabilities": {}}})

    nodes, edges, focus = client(fake).style(diagram, Changes(("pg",), ("orders->pg",)))

    body = fake.body()
    assert set(body["questions"]) == {"shape:pg", "focus", "edge:orders->pg"}
    assert body["state"]["transcript"] == ["Orders are written to Postgres."]
    assert body["state"]["nodes"] == [{"id": "orders", "label": "Order service"}, {"id": "pg", "label": "Postgres"}]
    assert body["state"]["edges"] == [{"source": "orders", "target": "pg", "label": "writes"}]
    assert "`edges[0]`" in body["questions"]["edge:orders->pg"]["instructions"]
    assert set(body["questions"]["shape:pg"]["criteria"]) == {shape.value for shape in Shape}
    assert set(body["questions"]["edge:orders->pg"]["criteria"]) == {style.value for style in EdgeStyle}
    assert nodes == {"pg": NodeStyle(Shape.DATABASE)}
    assert edges == {"orders->pg": EdgeStyle.DATA}
    assert focus == ("pg",)


def test_nothing_to_style_needs_no_request() -> None:
    fake = FakeJev({})

    assert client(fake).style(Diagram(1, DiagramKind.FLOW), Changes((), ())) == ({}, {}, None)
    assert fake.requests == []


@pytest.mark.parametrize(("kind", "candidates"), [
    (DiagramKind.FLOW, {"browser", "server"}),
    (DiagramKind.SEQUENCE, {"browser->server", "server->browser"}),
    (DiagramKind.SWIMLANE, {"browser", "server", "browser->server", "server->browser"}),
])
def test_the_focus_question_offers_the_elements_that_may_carry_the_accent(kind: DiagramKind,
                                                                          candidates: set[str]) -> None:
    diagram = Diagram(1, kind)
    diagram.update(parse_structure('browser -> server "request"\nserver -> browser "response"'))
    fake = FakeJev({"edge:browser->server": {"type": "choice", "choice": "sync", "confidence": 1.0},
                    "focus": {"type": "choice", "choice": NO_FOCUS, "confidence": 0.9}})

    client(fake).style(diagram, Changes((), ("browser->server",)))

    assert set(fake.body()["questions"]["focus"]["criteria"]) == candidates | {NO_FOCUS}


@pytest.mark.parametrize(("answer", "focus"), [
    ({"choice": "pg", "confidence": 0.6}, ("pg",)),
    ({"choice": NO_FOCUS, "confidence": 0.6}, ()),
    ({"choice": "pg", "confidence": 0.2}, None),
])
def test_an_unsure_focus_answer_keeps_the_current_focus(answer: dict[str, object],
                                                        focus: tuple[str, ...] | None) -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(parse_structure('pg "Postgres"'))
    fake = FakeJev({"shape:pg": {"type": "choice", "choice": "database", "confidence": 1.0},
                    "focus": {"type": "choice", **answer}})

    assert client(fake).style(diagram, Changes(("pg",), ()))[2] == focus


@pytest.mark.parametrize("kind", [DiagramKind.DEPENDENCY, DiagramKind.JOURNEY, DiagramKind.DB_SCHEMA])
def test_a_kind_whose_accent_the_renderer_derives_gets_no_focus_question(kind: DiagramKind) -> None:
    diagram = Diagram(1, kind)
    diagram.update(parse_structure("api -> core"))
    fake = FakeJev({"edge:api->core": {"type": "choice", "choice": "dependency", "confidence": 1.0}})

    assert client(fake).style(diagram, Changes((), ("api->core",))) == ({}, {"api->core": EdgeStyle.DEPENDENCY}, None)
    assert "focus" not in fake.body()["questions"]


@pytest.mark.parametrize("status", [401, 422, 429, 500, 529])
def test_http_errors_raise_jev_error(status: int) -> None:
    with pytest.raises(JevError):
        client(FakeJev({}, status=status)).route("x", [], two_diagrams().overview())


def test_network_errors_raise_jev_error() -> None:
    def unreachable(request: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError("no route", request=request)

    with pytest.raises(JevError):
        JevClient("k", "jev-latest", httpx.MockTransport(unreachable)).route("x", [], two_diagrams().overview())


def test_unexpected_answers_raise_jev_error() -> None:
    with pytest.raises(JevError):
        client(FakeJev({"structural": {"type": "noul"}})).route("x", [], two_diagrams().overview())
