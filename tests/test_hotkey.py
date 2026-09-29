import pytest

from listening_app.hotkey import MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_F1, Hotkey, parse_hotkey


def test_letter_hotkey_with_modifiers() -> None:
    assert parse_hotkey("ctrl+alt+r") == Hotkey(MOD_CONTROL | MOD_ALT, ord("R"))


def test_function_key_and_spacing() -> None:
    assert parse_hotkey(" Ctrl + Shift + F9 ") == Hotkey(MOD_CONTROL | MOD_SHIFT, VK_F1 + 8)


@pytest.mark.parametrize("text", ["r", "ctrl+", "hyper+r", "ctrl+enter", "ctrl+f25"])
def test_invalid_hotkeys_are_rejected(text: str) -> None:
    with pytest.raises(ValueError):
        parse_hotkey(text)
