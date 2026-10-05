import pytest

from listening_app.config import AppConfig, Language
from listening_app.sketch import drawer as drawer_module
from listening_app.sketch.diagrams import Destination, Diagram, DiagramKind, DiagramSet
from listening_app.sketch.drawer import Credentials, Drawer, RoutedDrawing, SpokenLine, credentials_provider
from listening_app.sketch.structure import StructureError, parse_structure

CURRENT = 'title "Request path"\nauth "Auth service"\nredis "Redis"\nauth -> redis "caches tokens"'


class FakeCompletion:
    def __init__(self, answer: str) -> None:
        self.answer = answer
        self.calls: list[tuple[str, list[dict[str, str]], Credentials]] = []

    def __call__(self, model: str, messages: list[dict[str, str]], credentials: Credentials) -> str:
        self.calls.append((model, messages, credentials))
        return self.answer

    def prompt(self) -> str:
        return self.calls[-1][1][-1]["content"]


def make_drawer(answer: str, language: Language = Language.AUTO) -> tuple[Drawer, FakeCompletion]:
    fake = FakeCompletion(answer)
    return Drawer("xai/grok-4-fast-non-reasoning", lambda: Credentials("token", "https://api.x.ai/oauth"),
                  language, fake), fake


def request_path() -> Diagram:
    diagram = Diagram(1, DiagramKind.FLOW, transcript=["The gateway checks the token with auth."], summary="")
    diagram.update(parse_structure(CURRENT))
    return diagram


def test_draw_prompt_carries_the_diagram_its_transcript_and_the_new_lines() -> None:
    drawer, fake = make_drawer(CURRENT)
    diagram = request_path()
    diagram.select(["redis"])

    answer = drawer.draw(diagram, [SpokenLine("This one actually caches tokens.", pointed="auth")])

    prompt = fake.prompt()
    assert answer == CURRENT
    assert "live flow diagram" in prompt
    assert "Draw only what the speaker actually said" in prompt
    assert 'auth -> redis "caches tokens"' in prompt
    assert "The gateway checks the token with auth." in prompt
    assert "This one actually caches tokens. [pointing at Auth service (auth)]" in prompt
    assert "Selected: Redis (redis)" in prompt
    assert "Write labels in the language the speaker uses." in prompt
    assert fake.calls[-1][0] == "xai/grok-4-fast-non-reasoning"
    assert fake.calls[-1][2] == Credentials("token", "https://api.x.ai/oauth")


def test_draw_prompt_shows_an_empty_diagram_and_the_kind_hint() -> None:
    drawer, fake = make_drawer('title "Handshake"')

    drawer.draw(Diagram(2, DiagramKind.SEQUENCE), [SpokenLine("The client says hello.")])

    prompt = fake.prompt()
    assert "Current diagram:\n(empty)" in prompt
    assert "every edge is one message, listed in the order the messages happen" in prompt


def test_draw_prompt_includes_the_rolling_summary() -> None:
    drawer, fake = make_drawer(CURRENT)
    diagram = request_path()
    diagram.fold_into_summary("The browser calls the gateway.", 1)

    drawer.draw(diagram, [SpokenLine("Then orders.")])

    assert "Earlier, in short: The browser calls the gateway." in fake.prompt()
    assert "The gateway checks the token with auth." not in fake.prompt()


@pytest.mark.parametrize(("language", "instruction"), [
    (Language.POLISH, "Write all labels in Polish."),
    (Language.ENGLISH, "Write all labels in English."),
])
def test_label_language_is_part_of_the_prompt(language: Language, instruction: str) -> None:
    drawer, fake = make_drawer(CURRENT, language)

    drawer.draw(request_path(), [SpokenLine("x")])

    assert instruction in fake.prompt()


def two_diagrams() -> DiagramSet:
    diagrams = DiagramSet()
    first = diagrams.route(None, DiagramKind.FLOW)
    assert first is not None
    first.update(parse_structure(CURRENT))
    second = diagrams.route(None, DiagramKind.STATE)
    assert second is not None
    second.update(parse_structure('title "Order lifecycle"\ndraft "Draft"'))
    return diagrams


def test_routed_prompt_lists_every_diagram_and_asks_for_a_destination_line() -> None:
    drawer, fake = make_drawer("diagram 1\n" + CURRENT)

    drawer.draw_routed(two_diagrams(), ["Back to the first one."], [SpokenLine("There is a rate limiter.")])

    prompt = fake.prompt()
    assert "Diagram 1 (flow):\n" + CURRENT in prompt
    assert 'Diagram 2 (state, current):\ntitle "Order lifecycle"\ndraft "Draft"' in prompt
    assert "diagram new <type>" in prompt
    assert "Back to the first one." in prompt
    assert "There is a rate limiter." in prompt


@pytest.mark.parametrize(("answer", "expected"), [
    ("diagram 2\ndraft \"Draft\"", RoutedDrawing(Destination.existing(2), 'draft "Draft"')),
    ("diagram new sequence\nclient -> server", RoutedDrawing(Destination.new(DiagramKind.SEQUENCE), "client -> server")),
    ("```\ndiagram 1\n" + CURRENT + "\n```", RoutedDrawing(Destination.existing(1), CURRENT + "\n```")),
    ("none", RoutedDrawing(None, "")),
])
def test_routed_answers_are_split_into_destination_and_diagram(answer: str, expected: RoutedDrawing) -> None:
    drawer, _ = make_drawer(answer)

    assert drawer.draw_routed(two_diagrams(), [], [SpokenLine("x")]) == expected


@pytest.mark.parametrize("answer", ["", "Sure! Here it is:\ndiagram 1", "diagram new pie\na \"A\"", "diagram one"])
def test_routed_answers_without_a_destination_line_are_rejected(answer: str) -> None:
    drawer, _ = make_drawer(answer)

    with pytest.raises(StructureError):
        drawer.draw_routed(two_diagrams(), [], [SpokenLine("x")])


def test_summary_folds_older_lines_into_the_previous_summary() -> None:
    drawer, fake = make_drawer("  The browser calls the gateway, which checks tokens.  ")
    diagram = request_path()
    diagram.fold_into_summary("The browser calls the gateway.", 0)

    summary = drawer.summarize(diagram, ["The gateway checks tokens."])

    assert summary == "The browser calls the gateway, which checks tokens."
    assert "The browser calls the gateway." in fake.prompt()
    assert "The gateway checks tokens." in fake.prompt()


def test_grok_authorization_supplies_the_token_and_its_api_base(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(drawer_module, "grok_access_token", lambda: "grok-token")
    monkeypatch.setattr(drawer_module, "grok_api_base", lambda: "https://api.x.ai/oauth")
    config = AppConfig.model_validate({"providers": {"xai": {"grok_auth": True}}})

    assert credentials_provider(config)() == Credentials("grok-token", "https://api.x.ai/oauth")


def test_an_api_key_is_used_as_is(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("OPENAI_API_KEY", "sk-test")
    config = AppConfig.model_validate({"sketch": {"draw_model": "openai/gpt-4.1-mini"}})

    assert credentials_provider(config)() == Credentials("sk-test")


def test_retrospect_prompt_uses_the_full_transcript_and_asks_to_reconcile() -> None:
    drawer, fake = make_drawer('title "Request path"\nauth "Auth"\ncache "Cache"')
    diagram = request_path()
    diagram.transcript.append("Actually Redis is just a cache.")

    answer = drawer.retrospect(diagram)

    prompt = fake.prompt()
    assert 'cache "Cache"' in answer or "Cache" in answer
    assert "revise a live flow diagram" in prompt
    assert "Later statements win" in prompt
    assert "The gateway checks the token with auth." in prompt
    assert "Actually Redis is just a cache." in prompt
    assert 'auth -> redis "caches tokens"' in prompt
    assert "New lines:" not in prompt
