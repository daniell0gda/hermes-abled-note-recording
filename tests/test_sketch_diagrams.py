from listening_app.sketch.diagrams import (
    Diagram,
    DiagramKind,
    DiagramSet,
    EdgeStyle,
    NodeStyle,
    Shape,
)
from listening_app.sketch.structure import parse_structure

FIRST = parse_structure('title "Request path"\nauth "Auth service"\nredis "Redis"\nauth -> redis "caches tokens"')


def test_no_number_opens_a_new_numbered_diagram_and_makes_it_active() -> None:
    diagrams = DiagramSet()

    first = diagrams.route(None, DiagramKind.FLOW)
    second = diagrams.route(None, DiagramKind.STATE)

    assert first is not None and second is not None
    assert (first.number, second.number) == (1, 2)
    assert second.kind is DiagramKind.STATE
    assert diagrams.active is second


def test_an_existing_number_routes_to_that_diagram_and_activates_it() -> None:
    diagrams = DiagramSet()
    first = diagrams.create(DiagramKind.FLOW)
    diagrams.create(DiagramKind.STATE)

    routed = diagrams.route(1, DiagramKind.STATE)

    assert routed is first
    assert first.kind is DiagramKind.FLOW
    assert diagrams.active is first


def test_an_unknown_number_routes_nowhere() -> None:
    diagrams = DiagramSet()
    diagrams.route(None, DiagramKind.FLOW)

    assert diagrams.route(7, DiagramKind.FLOW) is None
    assert len(diagrams.diagrams) == 1


def test_update_reports_new_and_changed_elements_only() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)

    changes = diagram.update(parse_structure(
        'auth "Auth"\nredis "Redis"\norders "Orders"\nauth -> redis "caches tokens"\nauth -> orders'))

    assert changes.nodes == ("auth", "orders")
    assert changes.edges == ("auth->orders",)


def test_styles_are_kept_per_id_across_updates() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)
    diagram.set_styles({"redis": NodeStyle(Shape.CACHE)}, {"auth->redis": EdgeStyle.DATA})

    diagram.update(parse_structure('auth "Auth service"'))
    diagram.update(FIRST)

    assert diagram.node_style("redis") == NodeStyle(Shape.CACHE)
    assert diagram.edge_style("auth->redis") is EdgeStyle.DATA


def test_unstyled_elements_get_the_default_style_of_their_diagram_kind() -> None:
    flow, state = Diagram(1, DiagramKind.FLOW), Diagram(2, DiagramKind.STATE)

    assert flow.node_style("x") == NodeStyle(Shape.SERVICE)
    assert flow.edge_style("x->y") is EdgeStyle.SYNC
    assert state.node_style("x") == NodeStyle(Shape.STATE)
    assert state.edge_style("x->y") is EdgeStyle.TRANSITION


def test_selection_drops_elements_that_were_removed() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)
    diagram.select(["auth", "auth->redis", "ghost"])

    diagram.update(parse_structure('auth "Auth service"\norders "Orders"'))

    assert diagram.selection == {"auth"}


def test_view_carries_styles_selection_and_the_pointed_element() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)
    diagram.set_styles({"redis": NodeStyle(Shape.CACHE, focal=True)}, {})
    diagram.select(["auth"])

    view = diagram.view(pointed="redis")

    assert view == {
        "number": 1, "title": "Request path", "kind": "flow",
        "nodes": [{"id": "auth", "label": "Auth service", "shape": "service", "focal": False},
                  {"id": "redis", "label": "Redis", "shape": "cache", "focal": True}],
        "edges": [{"id": "auth->redis", "source": "auth", "target": "redis", "label": "caches tokens", "style": "sync"}],
        "groups": [], "selected": ["auth"], "pointed": "redis",
    }


def test_view_shows_at_most_two_focal_nodes() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(parse_structure('a "A"\nb "B"\nc "C"'))
    diagram.set_styles({node: NodeStyle(Shape.SERVICE, focal=True) for node in "abc"}, {})

    assert [node["focal"] for node in diagram.view()["nodes"]] == [True, True, False]


def test_routing_summary_lists_number_title_and_node_labels() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)

    assert diagram.routing_summary() == "Diagram 1: Request path (nodes: Auth service, Redis)"
    assert Diagram(2, DiagramKind.STATE).routing_summary() == "Diagram 2: untitled (nodes: none)"


def test_recent_lines_are_the_transcript_not_yet_folded_into_the_summary() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.transcript += ["one", "two", "three"]

    diagram.fold_into_summary("One happened.", 2)

    assert diagram.summary == "One happened."
    assert diagram.recent_lines() == ["three"]
