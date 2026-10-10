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


def test_a_closed_diagram_is_removed_and_its_number_is_not_used_again() -> None:
    diagrams = DiagramSet()
    diagrams.create(DiagramKind.FLOW)
    diagrams.create(DiagramKind.STATE)

    closed = diagrams.close(2)
    third = diagrams.create(DiagramKind.TREE)

    assert closed is True
    assert [diagram.number for diagram in diagrams.diagrams] == [1, 3]
    assert third.number == 3


def test_closing_the_active_diagram_activates_the_next_tab_else_the_previous_one() -> None:
    diagrams = DiagramSet()
    first, _, third = (diagrams.create(DiagramKind.FLOW) for _ in range(3))
    diagrams.activate(2)

    diagrams.close(2)
    after_middle = diagrams.active
    diagrams.close(3)

    assert after_middle is third
    assert diagrams.active is first


def test_closing_an_inactive_diagram_keeps_the_active_one() -> None:
    diagrams = DiagramSet()
    first = diagrams.create(DiagramKind.FLOW)
    second = diagrams.create(DiagramKind.FLOW)

    diagrams.close(1)

    assert diagrams.active is second
    assert first not in diagrams.diagrams


def test_closing_the_last_diagram_leaves_none_active() -> None:
    diagrams = DiagramSet()
    diagrams.create(DiagramKind.FLOW)

    diagrams.close(1)

    assert diagrams.active is None
    assert diagrams.diagrams == ()


def test_closing_an_unknown_diagram_changes_nothing() -> None:
    diagrams = DiagramSet()
    first = diagrams.create(DiagramKind.FLOW)

    assert diagrams.close(7) is False
    assert diagrams.diagrams == (first,)


def test_a_copy_does_not_reuse_the_number_of_a_closed_diagram() -> None:
    diagrams = DiagramSet()
    diagrams.create(DiagramKind.FLOW)
    diagrams.create(DiagramKind.FLOW)
    diagrams.close(2)

    copied = diagrams.copy()

    assert copied.create(DiagramKind.FLOW).number == 3


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
    diagram.set_styles({"redis": NodeStyle(Shape.CACHE)}, {}, ("redis",))
    diagram.select(["auth"])

    view = diagram.view(pointed="redis")

    assert view == {
        "number": 1, "title": "Request path", "kind": "flow",
        "nodes": [{"id": "auth", "label": "Auth service", "shape": "service", "focal": False},
                  {"id": "redis", "label": "Redis", "shape": "cache", "focal": True}],
        "edges": [{"id": "auth->redis", "source": "auth", "target": "redis", "label": "caches tokens", "style": "sync",
                   "focal": False}],
        "groups": [], "selected": ["auth"], "pointed": "redis",
    }


def test_view_shows_at_most_two_focal_elements() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(parse_structure('a "A"\nb "B"\nc "C"'))
    diagram.set_styles({}, {}, ("a", "b", "c"))

    assert [node["focal"] for node in diagram.view()["nodes"]] == [True, True, False]


def test_an_edge_can_be_the_focus() -> None:
    diagram = Diagram(1, DiagramKind.SEQUENCE)
    diagram.update(FIRST)
    diagram.set_styles({}, {}, ("auth->redis",))

    view = diagram.view()

    assert [edge["focal"] for edge in view["edges"]] == [True]
    assert [node["focal"] for node in view["nodes"]] == [False, False]


def test_styles_without_a_focus_keep_the_current_one() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)
    diagram.set_styles({}, {}, ("redis",))

    diagram.set_styles({"auth": NodeStyle(Shape.SERVICE)}, {}, None)

    assert diagram.focus == ("redis",)


def test_a_removed_element_loses_the_focus() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)
    diagram.set_styles({}, {}, ("redis",))

    diagram.update(parse_structure('auth "Auth service"'))

    assert [node["focal"] for node in diagram.view()["nodes"]] == [False]


def test_a_dependency_diagram_accents_the_edge_that_closes_a_cycle() -> None:
    diagram = Diagram(1, DiagramKind.DEPENDENCY)
    diagram.update(parse_structure("api -> core\ncore -> db\ndb -> core\napi -> db"))
    diagram.set_styles({}, {}, ("api",))

    view = diagram.view()

    assert [edge["id"] for edge in view["edges"] if edge["focal"]] == ["db->core"]
    assert not any(node["focal"] for node in view["nodes"])


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


def test_set_kind_changes_the_diagram_type_in_the_view() -> None:
    diagram = Diagram(1, DiagramKind.FLOW)
    diagram.update(FIRST)
    diagram.set_kind(DiagramKind.SWIMLANE)
    assert diagram.view()["kind"] == "swimlane"


def test_group_id_colliding_with_a_node_is_renamed() -> None:
    structure = parse_structure(
        'title "Team"\n'
        'team "Team leader"\n'
        'dev "Developer"\n'
        'group team "Team": team, dev'
    )
    assert [node.id for node in structure.nodes] == ["team", "dev"]
    assert [group.id for group in structure.groups] == ["team_group"]
    assert structure.groups[0].members == ("team", "dev")
