from collections.abc import Callable

import pytest

from listening_app.sketch.feed import SketchFeed
from listening_app.transcriber import Transcription

Handler = Callable[[Transcription], None]


class FakeMicrophone:
    def __init__(self, handler: Handler) -> None:
        self.handler = handler
        self.running = False

    def start(self) -> None:
        self.running = True

    def stop(self) -> None:
        self.running = False


class FakeRecording:
    def __init__(self) -> None:
        self.handler: Handler | None = None

    def listen(self, handler: Handler | None) -> None:
        self.handler = handler


class Feeds:
    def __init__(self) -> None:
        self.microphones: list[FakeMicrophone] = []

    def open_microphone(self, handler: Handler) -> FakeMicrophone:
        self.microphones.append(FakeMicrophone(handler))
        return self.microphones[-1]

    def running(self) -> list[FakeMicrophone]:
        return [microphone for microphone in self.microphones if microphone.running]


def handler(_: Transcription) -> None:
    pass


def test_sketch_alone_listens_through_its_own_microphone_transcription() -> None:
    feeds = Feeds()
    feed = SketchFeed(handler, feeds.open_microphone)

    feed.start(None)

    assert [microphone.handler for microphone in feeds.running()] == [handler]


def test_sketch_during_a_recording_shares_the_recordings_transcription() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)

    feed.start(recording)

    assert recording.handler is handler
    assert feeds.microphones == []


def test_a_recording_started_while_sketching_takes_over_from_the_microphone() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(None)

    feed.use_recording(recording)

    assert recording.handler is handler
    assert feeds.running() == []


def test_a_recording_stopped_while_sketching_hands_back_to_the_microphone() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(recording)

    feed.use_microphone()

    assert recording.handler is None
    assert len(feeds.running()) == 1


def test_handing_back_to_the_microphone_twice_keeps_one_microphone() -> None:
    feeds = Feeds()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(None)

    feed.use_microphone()

    assert len(feeds.microphones) == 1


def test_stopping_the_sketch_leaves_the_recording_running_without_it() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(recording)

    feed.stop()

    assert recording.handler is None
    assert feeds.running() == []


def test_stopping_a_sketch_alone_stops_its_microphone() -> None:
    feeds = Feeds()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(None)

    feed.stop()

    assert feeds.running() == []


def test_muting_a_sketch_alone_stops_its_microphone() -> None:
    feeds = Feeds()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(None)

    feed.mute()

    assert feeds.running() == []
    assert not feed.listening


def test_unmuting_a_sketch_alone_opens_a_new_microphone() -> None:
    feeds = Feeds()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(None)
    feed.mute()

    feed.unmute()

    assert len(feeds.running()) == 1
    assert feed.listening


def test_muting_during_a_recording_only_detaches_from_it() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(recording)

    feed.mute()

    assert recording.handler is None
    assert feeds.microphones == []


def test_unmuting_during_a_recording_listens_to_it_again() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(recording)
    feed.mute()

    feed.unmute()

    assert recording.handler is handler


def test_a_muted_sketch_stays_deaf_when_a_recording_starts() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(None)
    feed.mute()

    feed.use_recording(recording)

    assert recording.handler is None
    assert feeds.running() == []


def test_a_muted_sketch_opens_no_microphone_when_the_recording_stops() -> None:
    feeds, recording = Feeds(), FakeRecording()
    feed = SketchFeed(handler, feeds.open_microphone)
    feed.start(recording)
    feed.mute()

    feed.use_microphone()

    assert feeds.microphones == []


def test_a_microphone_that_cannot_open_keeps_the_sketch_muted() -> None:
    def broken_microphone(_: Handler) -> FakeMicrophone:
        raise OSError("no microphone")

    feed = SketchFeed(handler, broken_microphone)
    feed.mute()

    with pytest.raises(OSError):
        feed.unmute()

    assert not feed.listening
