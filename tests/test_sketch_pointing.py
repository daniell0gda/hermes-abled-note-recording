from listening_app.sketch.pointing import Box, PointerTracker

BOXES = {"auth": Box(100, 100, 80, 40), "redis": Box(300, 100, 80, 40)}
SAMPLE_S = 0.1


def tracker() -> PointerTracker:
    pointer = PointerTracker(radius_px=24, window_s=2)
    pointer.set_boxes(BOXES)
    return pointer


def hold(pointer: PointerTracker, x: float, y: float, start: float, seconds: float, jitter: float = 0) -> str | None:
    pointed = None
    for step in range(int(seconds / SAMPLE_S) + 1):
        offset = jitter if step % 2 else -jitter
        pointed = pointer.move(start + step * SAMPLE_S, x + offset, y)
    return pointed


def test_holding_still_over_an_element_for_the_window_points_at_it() -> None:
    assert hold(tracker(), 140, 120, start=10, seconds=2) == "auth"


def test_a_shorter_hold_points_at_nothing() -> None:
    assert hold(tracker(), 140, 120, start=10, seconds=1.5) is None


def test_small_jitter_within_the_radius_still_points() -> None:
    assert hold(tracker(), 140, 120, start=10, seconds=2, jitter=10) == "auth"


def test_moving_further_than_the_radius_points_at_nothing() -> None:
    assert hold(tracker(), 140, 120, start=10, seconds=2, jitter=40) is None


def test_near_an_element_within_the_radius_points_at_the_nearest_one() -> None:
    assert hold(tracker(), 200, 120, start=10, seconds=2) == "auth"
    assert hold(tracker(), 395, 150, start=10, seconds=2) == "redis"


def test_empty_space_beyond_the_radius_points_at_nothing() -> None:
    assert hold(tracker(), 240, 300, start=10, seconds=2) is None


def test_leaving_the_window_ends_pointing() -> None:
    pointer = tracker()
    hold(pointer, 140, 120, start=10, seconds=2)

    pointer.leave(12.5)

    assert pointer.pointed is None
    assert pointer.pointed_during(13, 15) is None


def test_a_segment_uses_the_element_pointed_at_while_it_was_spoken() -> None:
    pointer = tracker()
    hold(pointer, 140, 120, start=10, seconds=4)
    hold(pointer, 340, 120, start=14.1, seconds=4)

    assert pointer.pointed_during(9, 12.5) == "auth"
    assert pointer.pointed_during(16.5, 17.5) == "redis"
    assert pointer.pointed_during(11, 16.6) == "auth"


def test_a_segment_spoken_before_any_pointing_has_no_pointed_element() -> None:
    pointer = tracker()
    hold(pointer, 140, 120, start=10, seconds=4)

    assert pointer.pointed_during(2, 7.9) is None


def test_pointing_started_when_the_mouse_came_to_rest() -> None:
    pointer = tracker()
    hold(pointer, 140, 120, start=10, seconds=3)

    assert pointer.pointed_during(10.2, 10.8) == "auth"


def test_new_boxes_after_a_redraw_are_used_for_the_next_samples() -> None:
    pointer = tracker()
    pointer.set_boxes({"orders": Box(120, 110, 40, 20)})

    assert hold(pointer, 140, 120, start=10, seconds=2) == "orders"
