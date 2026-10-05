import threading
from typing import Any

import pytest

from listening_app.sketch.diagrams import (
    Changes,
    Destination,
    Diagram,
    DiagramKind,
    DiagramSet,
    EdgeStyle,
    NodeStyle,
    Overview,
    Shape,
)
from listening_app.sketch.drawer import Revision, RoutedDrawing, SpokenLine
from listening_app.sketch.jev_client import JevError, Routing
from listening_app.sketch.pipeline import MAX_ATTEMPTS, SketchPipeline, SketchState

FLOW = 'title "Request path"\nauth "Auth service"\nredis "Redis"\nauth -> redis "caches tokens"'
STRUCTURAL = Routing(structural=0.93, new_requested=0.02, target=1, confidence=0.9, kind=DiagramKind.FLOW)
FIRST = Routing(structural=0.93, new_requested=0.02, target=None, confidence=1.0, kind=DiagramKind.FLOW)
CHATTER = Routing(structural=0.02, new_requested=0.02, target=1, confidence=0.9, kind=DiagramKind.FLOW)
NEW_REQUEST = Routing(structural=0.2, new_requested=0.96, target=None, confidence=0.7, kind=DiagramKind.STATE)
UNSURE = Routing(structural=0.93, new_requested=0.02, target=1, confidence=0.2, kind=DiagramKind.FLOW)


class FakeJudge:
    def __init__(self, *routings: Routing) -> None:
        self.routings = list(routings)
        self.route_calls: list[tuple[str, list[str], Overview]] = []
        self.style_calls: list[Changes] = []
        self.fail_route = False
        self.fail_style = False

    def route(self, segment: str, previous: list[str], overview: Overview) -> Routing:
        self.route_calls.append((segment, previous, overview))
        if self.fail_route:
            raise JevError("503 overloaded")
        return self.routings.pop(0) if len(self.routings) > 1 else self.routings[0]

    def style(self, diagram: Diagram, changes: Changes) -> tuple[dict[str, NodeStyle], dict[str, EdgeStyle]]:
        self.style_calls.append(changes)
        if self.fail_style:
            raise JevError("timeout")
        return ({node: NodeStyle(Shape.CACHE if node == "redis" else Shape.SERVICE) for node in changes.nodes},
                {edge: EdgeStyle.DATA for edge in changes.edges})


class FakeDrawer:
    def __init__(self, *answers: str) -> None:
        self.answers = list(answers)
        self.draw_calls: list[tuple[int, list[SpokenLine]]] = []
        self.routed_calls: list[list[SpokenLine]] = []
        self.routed_answer = RoutedDrawing(Destination.new(DiagramKind.FLOW), FLOW)
        self.summaries: list[list[str]] = []
        self.retrospect_calls: list[int] = []
        self.retrospect_answers: list[str] = []
        self.gate: threading.Event | None = None
        self.entered = threading.Event()

    def draw(self, diagram: Diagram, lines: list[SpokenLine]) -> str:
        self.draw_calls.append((diagram.number, lines))
        self.entered.set()
        if self.gate is not None:
            self.gate.wait(5)
        answer = self.answers.pop(0) if len(self.answers) > 1 else self.answers[0]
        if answer == "raise":
            raise ConnectionError("Grok unreachable")
        return answer

    def draw_routed(self, diagrams: DiagramSet, previous: list[str], lines: list[SpokenLine]) -> RoutedDrawing:
        self.routed_calls.append(lines)
        return self.routed_answer

    def summarize(self, diagram: Diagram, lines: list[str]) -> str:
        self.summaries.append(lines)
        return "Summary of " + str(len(lines))

    def retrospect(self, diagram: Diagram) -> Revision:
        self.retrospect_calls.append(diagram.number)
        answer = self.retrospect_answers.pop(0) if len(self.retrospect_answers) > 1 else (
            self.retrospect_answers[0] if self.retrospect_answers else diagram.structure.to_text()
        )
        if isinstance(answer, Revision):
            return answer
        return Revision(answer)


class Recorder:
    def __init__(self) -> None:
        self.states: list[SketchState] = []
        self.skipped: list[tuple[int | None, str]] = []
        self.jev_problems: list[str] = []
        self.updated = threading.Event()

    def sketch_updated(self, state: SketchState) -> None:
        self.states.append(state)
        self.updated.set()

    def update_skipped(self, number: int | None, reason: str) -> None:
        self.skipped.append((number, reason))

    def jev_unavailable(self, reason: str) -> None:
        self.jev_problems.append(reason)


def segment(text: str, pointed: str | None = None) -> SpokenLine:
    return SpokenLine(text, pointed)


def pipeline(judge: FakeJudge | None, drawer: FakeDrawer, **sketch_options: object) -> tuple[SketchPipeline, Recorder]:
    recorder = Recorder()
    return SketchPipeline(drawer, judge, recorder, **sketch_options), recorder  # type: ignore[arg-type]


def latest_view(recorder: Recorder, number: int = 1) -> dict[str, Any]:
    return next(view for view in recorder.states[-1].views if view["number"] == number)


def test_a_structural_segment_is_drawn_styled_and_published() -> None:
    sketch, recorder = pipeline(FakeJudge(FIRST), FakeDrawer(FLOW))

    sketch.route(segment("Auth caches tokens in Redis."))
    sketch.draw_next()

    view = latest_view(recorder)
    assert [node["id"] for node in view["nodes"]] == ["auth", "redis"]
    assert view["nodes"][1]["shape"] == "cache"
    assert view["edges"][0]["style"] == "data"
    assert recorder.states[-1].active == 1
    assert recorder.states[-1].updated == 1


def test_small_talk_is_not_drawn() -> None:
    drawer = FakeDrawer(FLOW)
    sketch, recorder = pipeline(FakeJudge(CHATTER), drawer)

    sketch.route(segment("Can you hear me?"))

    assert not sketch.draw_next()
    assert drawer.draw_calls == []


def test_previous_segments_and_the_overview_go_to_routing() -> None:
    judge = FakeJudge(FIRST, CHATTER, CHATTER, CHATTER, CHATTER)
    sketch, _ = pipeline(judge, FakeDrawer(FLOW))
    sketch.route(segment("one"))
    sketch.draw_next()

    for text in ("two", "three", "four", "five"):
        sketch.route(segment(text))

    text, previous, overview = judge.route_calls[-1]
    assert (text, previous) == ("five", ["two", "three", "four"])
    assert overview == Overview(1, ((1, "Diagram 1: Request path (nodes: Auth service, Redis)"),))


def test_an_explicit_new_diagram_request_opens_an_empty_tab_at_once() -> None:
    drawer = FakeDrawer(FLOW, 'title "Order lifecycle"')
    sketch, recorder = pipeline(FakeJudge(FIRST, NEW_REQUEST), drawer)
    sketch.route(segment("Requests go to auth."))
    sketch.draw_next()

    sketch.route(segment("Let's start a new diagram for the order lifecycle."))

    assert [view["number"] for view in recorder.states[-1].views] == [1, 2]
    assert latest_view(recorder, 2)["kind"] == "state"
    assert recorder.states[-1].active == 2
    sketch.draw_next()
    assert drawer.draw_calls[-1][0] == 2
    assert latest_view(recorder, 2)["title"] == "Order lifecycle"


def test_segments_arriving_while_a_draw_is_in_flight_are_batched_into_the_next_draw() -> None:
    drawer = FakeDrawer(FLOW)
    drawer.gate = threading.Event()
    sketch, recorder = pipeline(FakeJudge(FIRST, STRUCTURAL), drawer)
    sketch.start()
    try:
        sketch.add(segment("First."))
        assert drawer.entered.wait(5)
        sketch.add(segment("Second."))
        sketch.add(segment("Third."))
        sketch.wait_until_routed(5)
        drawer.gate.set()
        sketch.wait_until_idle(5)
    finally:
        sketch.stop()

    assert [[line.text for line in lines] for _, lines in drawer.draw_calls] == [["First."], ["Second.", "Third."]]


def test_an_invalid_answer_keeps_the_last_good_diagram_and_retries_its_segments() -> None:
    drawer = FakeDrawer(FLOW, "Sure, here is your diagram!", FLOW)
    sketch, recorder = pipeline(FakeJudge(FIRST, STRUCTURAL), drawer)
    sketch.route(segment("First."))
    sketch.draw_next()
    published = len(recorder.states)

    sketch.route(segment("Second."))
    sketch.draw_next()

    assert len(recorder.states) == published
    assert recorder.skipped[-1][0] == 1
    assert "line 1" in recorder.skipped[-1][1]
    sketch.route(segment("Third."))
    sketch.draw_next()
    assert [line.text for line in drawer.draw_calls[-1][1]] == ["Second.", "Third."]
    assert latest_view(recorder)["title"] == "Request path"


def test_segments_are_dropped_after_the_last_attempt() -> None:
    drawer = FakeDrawer(FLOW, "raise")
    sketch, recorder = pipeline(FakeJudge(FIRST, STRUCTURAL), drawer)
    sketch.route(segment("First."))
    sketch.draw_next()

    sketch.route(segment("Second."))
    while sketch.draw_next():
        pass

    assert len(drawer.draw_calls) == 1 + MAX_ATTEMPTS
    assert "Grok unreachable" in recorder.skipped[-1][1]


def test_jev_failure_hands_routing_to_the_drawer_and_is_reported_once() -> None:
    judge = FakeJudge(FIRST)
    drawer = FakeDrawer(FLOW)
    sketch, recorder = pipeline(judge, drawer)
    judge.fail_route = True

    sketch.route(segment("Auth caches tokens."))
    sketch.route(segment("Redis holds them."))
    while sketch.draw_next():
        pass

    assert len(judge.route_calls) == 1
    assert recorder.jev_problems == ["503 overloaded"]
    assert [[line.text for line in lines] for lines in drawer.routed_calls] == [["Auth caches tokens.", "Redis holds them."]]
    assert [node["id"] for node in latest_view(recorder)["nodes"]] == ["auth", "redis"]


def test_without_jev_the_drawer_routes_and_default_styles_are_used() -> None:
    drawer = FakeDrawer(FLOW)
    drawer.routed_answer = RoutedDrawing(Destination.new(DiagramKind.STATE), 'draft "Draft"\npaid "Paid"\ndraft -> paid')
    sketch, recorder = pipeline(None, drawer)

    sketch.route(segment("An order starts as draft and becomes paid."))
    sketch.draw_next()

    view = latest_view(recorder)
    assert view["kind"] == "state"
    assert [node["shape"] for node in view["nodes"]] == ["state", "state"]
    assert view["edges"][0]["style"] == "transition"


def test_the_drawer_may_find_that_nothing_needs_drawing() -> None:
    drawer = FakeDrawer(FLOW)
    drawer.routed_answer = RoutedDrawing(None, "")
    sketch, recorder = pipeline(None, drawer)

    sketch.route(segment("Any questions?"))
    sketch.draw_next()

    assert recorder.states == []
    assert recorder.skipped == []


def test_low_routing_confidence_lets_the_drawer_choose() -> None:
    drawer = FakeDrawer(FLOW)
    sketch, _ = pipeline(FakeJudge(FIRST, UNSURE), drawer)
    sketch.route(segment("First."))
    sketch.draw_next()

    sketch.route(segment("Hmm, the other part."))
    sketch.draw_next()

    assert [[line.text for line in lines] for lines in drawer.routed_calls] == [["Hmm, the other part."]]


def test_a_style_failure_falls_back_to_default_styles() -> None:
    judge = FakeJudge(FIRST)
    judge.fail_style = True
    sketch, recorder = pipeline(judge, FakeDrawer(FLOW))

    sketch.route(segment("Auth caches tokens in Redis."))
    sketch.draw_next()

    assert [node["shape"] for node in latest_view(recorder)["nodes"]] == ["service", "service"]
    assert recorder.jev_problems == ["timeout"]


def test_only_new_or_changed_elements_are_styled() -> None:
    judge = FakeJudge(FIRST, STRUCTURAL)
    sketch, _ = pipeline(judge, FakeDrawer(FLOW, FLOW + '\norders "Orders"'))
    sketch.route(segment("First."))
    sketch.draw_next()

    sketch.route(segment("Second."))
    sketch.draw_next()

    assert judge.style_calls[-1] == Changes(("orders",), ())


def test_the_pointed_element_travels_with_its_line() -> None:
    drawer = FakeDrawer(FLOW)
    sketch, _ = pipeline(FakeJudge(FIRST), drawer)

    sketch.route(segment("This one caches tokens.", pointed="auth"))
    sketch.draw_next()

    assert drawer.draw_calls[-1][1] == [SpokenLine("This one caches tokens.", "auth")]


def test_long_transcripts_are_folded_into_the_summary_when_idle() -> None:
    drawer = FakeDrawer(FLOW)
    sketch, _ = pipeline(FakeJudge(FIRST, STRUCTURAL), drawer)
    for index in range(17):
        sketch.route(segment(f"line {index}"))
        sketch.draw_next()

    sketch.summarize_if_due()

    assert drawer.summaries == [[f"line {index}" for index in range(8)]]
    assert sketch.snapshot().views[0]["number"] == 1


def test_selection_survives_updates_for_elements_that_still_exist() -> None:
    sketch, recorder = pipeline(FakeJudge(FIRST, STRUCTURAL), FakeDrawer(FLOW, 'auth "Auth service"'))
    sketch.route(segment("First."))
    sketch.draw_next()

    sketch.select(1, ["auth", "redis"])
    sketch.route(segment("Remove Redis."))
    sketch.draw_next()

    assert latest_view(recorder)["selected"] == ["auth"]


def test_naming_another_diagram_switches_to_it_even_without_new_structure() -> None:
    back_to_first = Routing(structural=0.05, new_requested=0.02, target=1, confidence=0.92, kind=DiagramKind.FLOW)
    sketch, recorder = pipeline(FakeJudge(FIRST, NEW_REQUEST, back_to_first), FakeDrawer(FLOW))
    sketch.route(segment("First."))
    sketch.route(segment("New diagram please."))

    sketch.route(segment("Let me go back to diagram 1 for a second."))

    assert recorder.states[-1].active == 1
    assert recorder.states[-1].updated is None


def test_a_segment_for_another_diagram_switches_to_it_before_it_is_drawn() -> None:
    sketch, recorder = pipeline(FakeJudge(FIRST, NEW_REQUEST, STRUCTURAL), FakeDrawer(FLOW))
    sketch.route(segment("First."))
    sketch.route(segment("New diagram please."))

    sketch.route(segment("Between the gateway and orders there is a rate limiter."))

    assert recorder.states[-1].active == 1
    assert recorder.states[-1].updated is None


def test_activating_a_tab_makes_it_the_current_diagram_for_routing() -> None:
    judge = FakeJudge(FIRST, NEW_REQUEST, CHATTER)
    sketch, _ = pipeline(judge, FakeDrawer(FLOW))
    sketch.route(segment("First."))
    sketch.route(segment("New diagram please."))

    sketch.activate(1)
    sketch.route(segment("So."))

    assert judge.route_calls[-1][2].active == 1


@pytest.mark.parametrize("failing", ["sketch_updated", "update_skipped"])
def test_listener_errors_do_not_stop_the_pipeline(failing: str) -> None:
    drawer = FakeDrawer("oops", FLOW)
    sketch, recorder = pipeline(FakeJudge(FIRST), drawer)

    def broken(*_: object) -> None:
        raise RuntimeError("window gone")

    setattr(recorder, failing, broken)
    sketch.route(segment("First."))

    while sketch.draw_next():
        pass


CORRECTED = '''title "Request path"
auth "Auth service"
cache "Cache"
auth -> cache "caches tokens"'''


def test_retrospect_runs_after_every_successful_draw() -> None:
    """With every_n=1, each draw schedules a retrospect — no keyword required."""
    drawer = FakeDrawer(FLOW, FLOW)
    drawer.retrospect_answers = [CORRECTED]
    sketch, recorder = pipeline(FakeJudge(FIRST, STRUCTURAL), drawer, retrospect_every_n=1)
    sketch.route(segment("Auth caches tokens in Redis."))
    sketch.draw_next()
    sketch.retrospect_if_due()

    assert drawer.retrospect_calls == [1]
    assert [node["id"] for node in latest_view(recorder)["nodes"]] == ["auth", "cache"]
    assert "redis" not in [node["id"] for node in latest_view(recorder)["nodes"]]


def test_retrospect_does_not_need_correction_keywords() -> None:
    """Ordinary speech like 'not satisfied' still gets a retrospect after a draw."""
    drawer = FakeDrawer(FLOW)
    drawer.retrospect_answers = [FLOW]
    sketch, recorder = pipeline(FakeJudge(FIRST), drawer, retrospect_every_n=1)
    sketch.route(segment("The checker is not satisfied and loops back."))
    sketch.draw_next()
    sketch.retrospect_if_due()

    assert drawer.retrospect_calls == [1]
    assert latest_view(recorder)["title"] == "Request path"


def test_periodic_retrospect_can_wait_for_every_n_draws() -> None:
    drawer = FakeDrawer(FLOW)
    drawer.retrospect_answers = [FLOW]
    sketch, recorder = pipeline(FakeJudge(FIRST, STRUCTURAL), drawer, retrospect_every_n=2)
    sketch.route(segment("First."))
    sketch.draw_next()
    sketch.retrospect_if_due()
    assert drawer.retrospect_calls == []

    sketch.route(segment("Second."))
    sketch.draw_next()
    sketch.retrospect_if_due()

    assert drawer.retrospect_calls == [1]
    assert latest_view(recorder)["title"] == "Request path"


def test_too_many_nodes_schedules_a_retrospect() -> None:
    many = 'title "Big"\n' + "\n".join(f'n{i} "Node {i}"' for i in range(10))
    drawer = FakeDrawer(many)
    drawer.retrospect_answers = ['title "Big"\nn0 "Node 0"\nn1 "Node 1"']
    sketch, recorder = pipeline(FakeJudge(FIRST), drawer, retrospect_every_n=0, retrospect_max_nodes=9)
    sketch.route(segment("Lots of nodes."))
    sketch.draw_next()
    sketch.retrospect_if_due()

    assert drawer.retrospect_calls == [1]
    assert len(latest_view(recorder)["nodes"]) == 2


def test_retrospect_can_change_diagram_kind() -> None:
    drawer = FakeDrawer(FLOW)
    drawer.retrospect_answers = [
        Revision(
            'title "Issue flow"\nuser "User"\nplanner "Planner"\nuser -> planner "starts"',
            DiagramKind.SWIMLANE,
        )
    ]
    sketch, recorder = pipeline(FakeJudge(FIRST), drawer, retrospect_every_n=1)
    sketch.route(segment("The user starts an issue with the planner."))
    sketch.draw_next()
    sketch.retrospect_if_due()

    assert drawer.retrospect_calls == [1]
    assert latest_view(recorder)["kind"] == "swimlane"
    assert [node["id"] for node in latest_view(recorder)["nodes"]] == ["user", "planner"]
