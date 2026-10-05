from typing import Any

from listening_app.sketch.bucket_b import MAX_CHUNKS, chunk_clean_text, rebuild_from_clean
from listening_app.sketch.diagrams import (
    Changes,
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
from listening_app.sketch.pipeline import SketchPipeline, SketchState
from listening_app.sketch.structure import parse_structure

LIVE = 'title "Messy"\nworkflow "Workflow"\nteam_box "Team box"\nimplementer_utilities "Implementer utilities"'
CLEAN = ("The create issue skill files an issue on GitHub. Hmm. "
         "The start issue skill hands the issue to the team leader.\n"
         "The checker loops back to the implementer when not satisfied. The leader marks the issue done.")
STEP_1 = 'title "Issue workflow"\ncreate "Create issue skill"\ngithub "GitHub"\ncreate -> github "files issue"'
STEP_2 = STEP_1 + '\nstart "Start issue skill"\nleader "Team leader"\nstart -> leader'
STEP_3 = STEP_2 + ('\nimplementer "Implementer"\nchecker "Checker"\nchecker -> implementer "not satisfied"'
                   '\ngroup team "Team": implementer, checker')
STEP_4 = STEP_3 + '\ndone "Done"\nleader -> done'


def routing(structural: float = 0.93, kind: DiagramKind = DiagramKind.SWIMLANE, target: int | None = 1) -> Routing:
    return Routing(structural=structural, new_requested=0.02, target=target, confidence=1.0, kind=kind)


class FakeJudge:
    def __init__(self, *live: Routing, chunk: Routing | None = None) -> None:
        self.live = list(live)
        self.chunk = chunk or routing()
        self.route_calls: list[str] = []
        self.fail_on: str | None = None

    def route(self, segment: str, previous: list[str], overview: Overview) -> Routing:
        self.route_calls.append(segment)
        if self.fail_on is not None and self.fail_on in segment:
            raise JevError("503 overloaded")
        if self.live:
            return self.live.pop(0)
        return self.chunk

    def style(self, diagram: Diagram, changes: Changes) -> tuple[dict[str, NodeStyle], dict[str, EdgeStyle]]:
        return ({node: NodeStyle(Shape.EXTERNAL if node == "github" else Shape.SERVICE) for node in changes.nodes},
                {edge: EdgeStyle.SYNC for edge in changes.edges})


class FakeDrawer:
    def __init__(self, *answers: str, clean: str = CLEAN) -> None:
        self.answers = list(answers)
        self.clean = clean
        self.clean_calls: list[list[str]] = []
        self.drawn_on: list[str] = []
        self.on_clean: Any = None

    def clean_transcript(self, lines: list[str], summary: str = "") -> str:
        self.clean_calls.append(list(lines))
        if self.on_clean is not None:
            self.on_clean()
        if self.clean == "raise":
            raise ConnectionError("Grok unreachable")
        return self.clean

    def draw(self, diagram: Diagram, lines: list[SpokenLine]) -> str:
        self.drawn_on.append(diagram.structure.to_text())
        return self.answers.pop(0) if len(self.answers) > 1 else self.answers[0]

    def draw_routed(self, diagrams: DiagramSet, previous: list[str], lines: list[SpokenLine]) -> RoutedDrawing:
        raise AssertionError("not used")

    def summarize(self, diagram: Diagram, lines: list[str]) -> str:
        return ""

    def retrospect(self, diagram: Diagram) -> Revision:
        return Revision(diagram.structure.to_text())


class Recorder:
    def __init__(self) -> None:
        self.states: list[SketchState] = []

    def sketch_updated(self, state: SketchState) -> None:
        self.states.append(state)

    def update_skipped(self, number: int | None, reason: str) -> None:
        pass

    def jev_unavailable(self, reason: str) -> None:
        pass


def messy_diagram() -> Diagram:
    diagram = Diagram(1, DiagramKind.FLOW, transcript=["create issue skill to github uh", "team box implementer"])
    diagram.update(parse_structure(LIVE))
    return diagram


# --- chunking ---

def test_clean_text_is_split_into_one_sentence_chunks_without_filler() -> None:
    assert chunk_clean_text(CLEAN) == [
        "The create issue skill files an issue on GitHub.",
        "The start issue skill hands the issue to the team leader.",
        "The checker loops back to the implementer when not satisfied.",
        "The leader marks the issue done.",
    ]


def test_bullets_are_stripped_and_long_texts_fold_into_the_last_chunk() -> None:
    text = "\n".join(f"- Step number {index} happens." for index in range(MAX_CHUNKS + 5))
    chunks = chunk_clean_text(text)
    assert len(chunks) == MAX_CHUNKS
    assert chunks[0] == "Step number 0 happens."
    assert chunks[-1].endswith(f"Step number {MAX_CHUNKS + 4} happens.")


# --- rebuild ---

def test_rebuild_starts_from_an_empty_diagram_and_ignores_the_messy_live_graph() -> None:
    drawer = FakeDrawer(STEP_1, STEP_2, STEP_3, STEP_4)
    judge = FakeJudge()

    result = rebuild_from_clean(messy_diagram(), drawer, judge, generation=1)

    assert result is not None
    assert drawer.drawn_on[0] == ""
    assert "team_box" not in drawer.drawn_on[1]
    assert result.kind is DiagramKind.SWIMLANE
    assert [node.id for node in result.structure.nodes] == [
        "create", "github", "start", "leader", "implementer", "checker", "done"]
    assert [group.id for group in result.structure.groups] == ["team"]
    assert result.node_styles["github"].shape is Shape.EXTERNAL
    assert (result.chunk_count, result.drawn_chunks) == (4, 4)
    assert judge.route_calls[0] == CLEAN  # the kind is judged on the whole clean text first


def test_chunks_without_structure_are_not_drawn() -> None:
    judge = FakeJudge(routing(), routing(), routing(structural=0.1), routing(structural=0.1), routing())
    drawer = FakeDrawer(STEP_1, STEP_2)

    result = rebuild_from_clean(messy_diagram(), drawer, judge, generation=1)

    assert result is not None
    assert result.drawn_chunks == 2


def test_jev_failure_fails_the_rebuild_gracefully() -> None:
    judge = FakeJudge()
    judge.fail_on = "checker loops"

    assert rebuild_from_clean(messy_diagram(), FakeDrawer(STEP_1), judge, generation=1) is None


def test_grok_failure_fails_the_rebuild_gracefully() -> None:
    assert rebuild_from_clean(messy_diagram(), FakeDrawer(STEP_1, clean="raise"), FakeJudge(), generation=1) is None


def test_a_stale_generation_stops_the_rebuild() -> None:
    drawer = FakeDrawer(STEP_1)

    result = rebuild_from_clean(messy_diagram(), drawer, FakeJudge(), generation=1, is_current=lambda gen: False)

    assert result is None
    assert drawer.drawn_on == []


# --- pipeline swap ---

def live_pipeline(judge: FakeJudge | None, drawer: FakeDrawer) -> tuple[SketchPipeline, Recorder]:
    recorder = Recorder()
    sketch = SketchPipeline(drawer, judge, recorder, retrospect_every_n=0, bucket_b=True,
                            bucket_b_min_new_lines=2)
    return sketch, recorder


def speak_two_lines(sketch: SketchPipeline) -> None:
    for text in ("create issue skill to github uh", "team box implementer"):
        sketch.route(SpokenLine(text))
        sketch.draw_next()


def view(recorder: Recorder) -> dict[str, Any]:
    return next(v for v in recorder.states[-1].views if v["number"] == 1)


def node_ids(recorder: Recorder) -> list[str]:
    return [node["id"] for node in view(recorder)["nodes"]]


def test_bucket_b_soft_swaps_the_live_view_without_losing_the_transcript() -> None:
    judge = FakeJudge(routing(kind=DiagramKind.FLOW, target=None), routing(kind=DiagramKind.FLOW))
    drawer = FakeDrawer(LIVE, LIVE, STEP_1, STEP_2, STEP_3, STEP_4)
    sketch, recorder = live_pipeline(judge, drawer)
    speak_two_lines(sketch)
    assert "team_box" in node_ids(recorder)

    assert sketch.run_bucket_b() is True

    assert view(recorder)["kind"] == "swimlane"
    assert "team_box" not in node_ids(recorder)
    assert "done" in node_ids(recorder)
    assert recorder.states[-1].updated == 1
    assert drawer.clean_calls == [["create issue skill to github uh", "team box implementer"]]


def test_bucket_b_needs_enough_new_lines_before_it_runs_again() -> None:
    judge = FakeJudge(routing(kind=DiagramKind.FLOW, target=None), routing(kind=DiagramKind.FLOW))
    drawer = FakeDrawer(LIVE, LIVE, STEP_4)
    sketch, _ = live_pipeline(judge, drawer)
    speak_two_lines(sketch)

    assert sketch.run_bucket_b() is True
    assert sketch.run_bucket_b() is False  # nothing new requested
    assert len(drawer.clean_calls) == 1


def test_an_outdated_bucket_b_result_is_not_swapped_in() -> None:
    judge = FakeJudge(routing(kind=DiagramKind.FLOW, target=None), routing(kind=DiagramKind.FLOW))
    drawer = FakeDrawer(LIVE, LIVE, STEP_4)
    sketch, recorder = live_pipeline(judge, drawer)
    speak_two_lines(sketch)
    published = len(recorder.states)
    drawer.on_clean = sketch.cancel_bucket_b  # a newer run / cancel while B is in flight

    assert sketch.run_bucket_b() is False
    assert len(recorder.states) == published
    assert "team_box" in node_ids(recorder)


def test_without_jev_bucket_b_is_disabled_and_the_live_view_stays() -> None:
    drawer = FakeDrawer(LIVE)
    sketch, _ = live_pipeline(None, drawer)

    assert sketch.run_bucket_b(1) is False
    assert drawer.clean_calls == []


def test_a_live_draw_computed_before_a_swap_is_redone_on_top_of_it() -> None:
    judge = FakeJudge(routing(kind=DiagramKind.FLOW, target=None), routing(kind=DiagramKind.FLOW),
                      routing(kind=DiagramKind.FLOW))
    drawer = FakeDrawer(LIVE, LIVE, STEP_4)
    sketch, recorder = live_pipeline(judge, drawer)
    speak_two_lines(sketch)
    sketch.route(SpokenLine("and the leader reviews"))
    original_draw = drawer.draw

    def draw_while_b_swaps(diagram: Diagram, lines: list[SpokenLine]) -> str:
        drawer.draw = original_draw  # type: ignore[method-assign]
        assert sketch.run_bucket_b(1) is True
        return LIVE

    drawer.draw = draw_while_b_swaps  # type: ignore[method-assign]
    sketch.draw_next()  # stale: the batch goes back to pending
    assert "done" in node_ids(recorder)

    drawer.answers = [STEP_4 + '\nleader -> checker "reviews"']
    sketch.draw_next()
    assert "done" in node_ids(recorder)
    assert "team_box" not in node_ids(recorder)
