import pytest

from listening_app.sketch.structure import Edge, Group, Node, StructureError, parse_structure

DIAGRAM = '''title "Request path"
auth "Auth service"
redis "Redis"
auth -> redis "caches tokens"
group backend "Backend": auth, redis
'''


def test_nodes_edges_groups_and_title_are_parsed() -> None:
    structure = parse_structure(DIAGRAM)

    assert structure.title == "Request path"
    assert structure.nodes == (Node("auth", "Auth service"), Node("redis", "Redis"))
    assert structure.edges == (Edge("auth->redis", "auth", "redis", "caches tokens"),)
    assert structure.groups == (Group("backend", "Backend", ("auth", "redis")),)


def test_edge_label_is_optional() -> None:
    structure = parse_structure('a "A"\nb "B"\na -> b')

    assert structure.edges == (Edge("a->b", "a", "b", ""),)


def test_group_without_label_is_named_by_its_id() -> None:
    structure = parse_structure('a "A"\ngroup backend: a')

    assert structure.groups == (Group("backend", "backend", ("a",)),)


def test_edge_order_is_kept_and_repeated_messages_get_their_own_ids() -> None:
    structure = parse_structure('client "Client"\nserver "Server"\n'
                                'client -> server "request"\nserver -> client "response"\nclient -> server "retry"')

    assert [edge.label for edge in structure.edges] == ["request", "response", "retry"]
    assert [edge.id for edge in structure.edges] == ["client->server", "server->client", "client->server#2"]


def test_undeclared_edge_ends_become_nodes_labelled_from_their_id() -> None:
    structure = parse_structure("api_gateway -> order_service")

    assert structure.nodes == (Node("api_gateway", "Api gateway"), Node("order_service", "Order service"))


def test_a_redeclared_node_keeps_its_place_and_takes_the_new_label() -> None:
    structure = parse_structure('a "A"\nb "B"\na "Renamed"')

    assert structure.nodes == (Node("a", "Renamed"), Node("b", "B"))


def test_unknown_group_members_are_dropped_and_empty_groups_removed() -> None:
    structure = parse_structure('a "A"\ngroup one: a, ghost\ngroup two: ghost')

    assert structure.groups == (Group("one", "one", ("a",)),)


def test_blank_lines_and_code_fences_are_ignored() -> None:
    structure = parse_structure('```text\n\na "A"\n```\n')

    assert structure.nodes == (Node("a", "A"),)


def test_title_only_is_a_valid_empty_diagram() -> None:
    structure = parse_structure('title "Order lifecycle"')

    assert structure.title == "Order lifecycle"
    assert structure.nodes == ()


@pytest.mark.parametrize("text", [
    "",
    "  \n```\n```",
    "Here is the updated diagram:",
    'Auth "Auth service"',
    'a -> "B"',
    "a --> b",
    'graph LR\n  a[Auth] --> b[Redis]',
    '<svg viewBox="0 0 10 10"></svg>',
])
def test_invalid_text_is_rejected(text: str) -> None:
    with pytest.raises(StructureError):
        parse_structure(text)


def test_the_error_names_the_offending_line() -> None:
    with pytest.raises(StructureError, match="line 2: oops"):
        parse_structure('a "A"\noops')


def test_a_stray_line_in_a_long_answer_is_skipped() -> None:
    structure = parse_structure('a "A"\nb "B"\nc "C"\na -> b\nb -> c "next" when the loop ends\nc -> a')

    assert [edge.id for edge in structure.edges] == ["a->b", "c->a"]


def test_group_lines_with_the_same_label_or_id_are_one_group() -> None:
    structure = parse_structure('a "A"\nb "B"\nc "C"\ngroup lead_a "Team leader": a\ngroup lead_b "team leader": b\n'
                                'group lead_a: c, a')

    assert structure.groups == (Group("lead_a", "Team leader", ("a", "b", "c")),)


def test_text_form_round_trips() -> None:
    structure = parse_structure(DIAGRAM)

    assert parse_structure(structure.to_text()) == structure


def test_element_ids_cover_nodes_and_edges() -> None:
    assert parse_structure(DIAGRAM).element_ids() == {"auth", "redis", "auth->redis"}
