import json
from datetime import datetime
from pathlib import Path

import numpy as np

from listening_app.models import HermesStatus, SegmentStatus, Source, TranscriptSegment
from listening_app.transcript_store import (
    Devices,
    SessionFiles,
    SessionMeta,
    TranscriptStore,
    finalize_session,
    find_outboxes,
    find_unfinished_sessions,
)

SESSION = "2026-09-28T10-13-40_a1b2"
STARTED = datetime.fromisoformat("2026-09-28T10:13:40+02:00")


def meta() -> SessionMeta:
    return SessionMeta(session_id=SESSION, started_at=STARTED.isoformat(), stt_model="groq/whisper-large-v3-turbo",
                       devices=Devices(mic="Headset Microphone", output="Headset Earphone"), language="pl")


def segment(seq: int, source: Source, start: float, status: SegmentStatus = SegmentStatus.OK) -> TranscriptSegment:
    return TranscriptSegment(seq=seq, source=source, start=start, end=start + 2, wall_start=STARTED.isoformat(),
                             text=f"zdanie {seq}", language="pl", stt_model="groq/whisper-large-v3-turbo", status=status,
                             hermes_status=HermesStatus.QUEUED)


def test_every_file_of_a_session_is_in_its_own_folder(tmp_path: Path) -> None:
    files = SessionFiles(tmp_path, SESSION)

    paths = [files.jsonl, files.final, files.meta, files.outbox, files.audio_dir]

    assert {path.parent for path in paths} == {tmp_path / SESSION}
    assert SessionFiles.from_outbox(files.outbox) == files


def test_jsonl_has_one_segment_per_line(tmp_path: Path) -> None:
    store = TranscriptStore(SessionFiles(tmp_path, SESSION), meta())
    store.append(segment(0, Source.ME, 1.0))
    store.append(segment(1, Source.OTHERS, 0.5))

    lines = (tmp_path / SESSION / f"{SESSION}.jsonl").read_text(encoding="utf-8").splitlines()

    assert [json.loads(line)["seq"] for line in lines] == [0, 1]
    assert "audio_file" not in json.loads(lines[0])


def test_final_json_sorts_by_start_and_applies_delivery_results(tmp_path: Path) -> None:
    files = SessionFiles(tmp_path, SESSION)
    store = TranscriptStore(files, meta())
    store.append(segment(0, Source.ME, 5.0))
    store.append(segment(1, Source.OTHERS, 1.0, SegmentStatus.STT_FAILED))
    store.append(segment(2, Source.ME, 9.0))
    store.close(datetime.fromisoformat("2026-09-28T11:02:11+02:00"))

    finalize_session(files, {0: HermesStatus.DELIVERED, 2: HermesStatus.DELIVERED})
    transcript = json.loads(files.final.read_text(encoding="utf-8"))

    assert transcript["schema_version"] == 1
    assert [s["seq"] for s in transcript["segments"]] == [1, 0, 2]
    assert transcript["ended_at"] == "2026-09-28T11:02:11+02:00"
    assert transcript["stats"] == {"segments": 3, "failed": 1, "hermes_undelivered": 1, "duration_s": 2911}
    assert transcript["devices"] == {"mic": "Headset Microphone", "output": "Headset Earphone"}


def test_crashed_session_is_found_and_finalized_with_every_complete_line(tmp_path: Path) -> None:
    files = SessionFiles(tmp_path, SESSION)
    store = TranscriptStore(files, meta())
    for seq in range(4):
        store.append(segment(seq, Source.ME, float(seq)))
    with files.jsonl.open("a", encoding="utf-8") as jsonl:
        jsonl.write('{"seq": 4, "source": "me", "sta')  # the process died mid-write

    unfinished = find_unfinished_sessions(tmp_path)
    assert unfinished == [files]

    finalize_session(unfinished[0], {})
    transcript = json.loads(files.final.read_text(encoding="utf-8"))

    assert [s["seq"] for s in transcript["segments"]] == [0, 1, 2, 3]
    assert find_unfinished_sessions(tmp_path) == []


def test_active_session_and_outbox_files_are_not_reported_unfinished(tmp_path: Path) -> None:
    files = SessionFiles(tmp_path, SESSION)
    TranscriptStore(files, meta())
    files.outbox.write_text("{}\n", encoding="utf-8")

    assert find_unfinished_sessions(tmp_path, exclude={SESSION}) == []


def test_folders_without_a_transcript_are_not_reported_unfinished(tmp_path: Path) -> None:
    (tmp_path / "holiday photos").mkdir()
    (tmp_path / "notes.jsonl").write_text("{}\n", encoding="utf-8")

    assert find_unfinished_sessions(tmp_path) == []


def test_outboxes_are_found_in_the_session_folders(tmp_path: Path) -> None:
    files = SessionFiles(tmp_path, SESSION)
    files.folder.mkdir()
    files.outbox.write_text("{}\n", encoding="utf-8")

    assert find_outboxes(tmp_path) == [files.outbox]
    assert find_outboxes(tmp_path / "missing") == []


def test_segment_audio_is_saved_under_the_session_folder(tmp_path: Path) -> None:
    store = TranscriptStore(SessionFiles(tmp_path, SESSION), meta())

    relative = store.save_audio(7, Source.OTHERS, np.zeros(1600, dtype=np.float32))

    assert relative == "audio/00007_others.wav"
    assert (tmp_path / SESSION / relative).stat().st_size > 1600
