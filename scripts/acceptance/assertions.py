"""
[INPUT]: tmux/backend capture strings, pane dimensions, zoom flags, and caller
         supplied assertion functions.
[OUTPUT]: Bounded assertion results/reports for scenario composition; no shell,
          UI, filesystem, or process side effects.
[POS]: Backend truth seam beneath UI acceptance; screenshots remain diagnostics,
       while these helpers decide whether a behavior actually passed.
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable, Iterable, Sequence


MAX_DETAIL = 240


def _detail(value: str) -> str:
    value = str(value).replace("\n", " ").replace("\r", " ").strip()
    return value[:MAX_DETAIL] or "assertion failed"


@dataclass(frozen=True)
class AssertionResult:
    """One bounded result that can be written to an evidence ledger."""

    name: str
    passed: bool
    detail: str

    def __post_init__(self) -> None:
        if not self.name.strip():
            raise ValueError("assertion name must be non-empty")
        object.__setattr__(self, "detail", _detail(self.detail))

    def as_dict(self) -> dict[str, object]:
        return {"name": self.name, "passed": self.passed, "detail": self.detail}


@dataclass(frozen=True)
class AssertionCheck:
    """A named assertion function that a scenario can add to a suite."""

    name: str
    evaluate: Callable[[], AssertionResult]

    def run(self) -> AssertionResult:
        try:
            result = self.evaluate()
        except Exception as error:  # noqa: BLE001 - assertions fail closed.
            return AssertionResult(self.name, False, f"assertion error: {type(error).__name__}")
        if result.name != self.name:
            return AssertionResult(self.name, False, "assertion returned a mismatched name")
        return result


@dataclass(frozen=True)
class AssertionReport:
    results: tuple[AssertionResult, ...]

    @property
    def passed(self) -> bool:
        return all(result.passed for result in self.results)

    def as_dict(self) -> dict[str, object]:
        return {
            "passed": self.passed,
            "total": len(self.results),
            "failed": sum(not result.passed for result in self.results),
            "assertions": [result.as_dict() for result in self.results],
        }


def run_assertions(checks: Iterable[AssertionCheck]) -> AssertionReport:
    """Evaluate a caller-selected suite and preserve order for evidence."""

    return AssertionReport(tuple(check.run() for check in checks))


def contains_marker(capture: str, marker: str, *, name: str = "marker") -> AssertionResult:
    if not isinstance(capture, str) or not isinstance(marker, str) or not marker:
        return AssertionResult(name, False, "marker or capture is empty")
    return AssertionResult(name, marker in capture, "marker present" if marker in capture else "marker missing")


def pane_count(capture: str) -> int:
    """Count non-empty tmux ``list-panes`` lines without invoking tmux."""

    if not isinstance(capture, str):
        raise ValueError("pane capture must be text")
    return sum(bool(line.strip()) for line in capture.splitlines())


def pane_count_is(capture: str, expected: int, *, name: str = "pane_count") -> AssertionResult:
    if isinstance(expected, bool) or not isinstance(expected, int) or expected < 0:
        return AssertionResult(name, False, "expected pane count must be a non-negative integer")
    actual = pane_count(capture)
    return AssertionResult(
        name,
        actual == expected,
        f"pane count expected {expected}, got {actual}",
    )


def _dimensions(value: Sequence[int] | str) -> tuple[int, int] | None:
    if isinstance(value, str):
        fields = value.replace("x", " ").split()
    else:
        fields = list(value)
    if len(fields) != 2:
        return None
    try:
        width, height = (int(field) for field in fields)
    except (TypeError, ValueError):
        return None
    if width < 0 or height < 0:
        return None
    return width, height


def geometry_changed(
    before: Sequence[int] | str,
    after: Sequence[int] | str,
    *,
    name: str = "geometry",
) -> AssertionResult:
    old = _dimensions(before)
    new = _dimensions(after)
    if old is None or new is None:
        return AssertionResult(name, False, "pane dimensions must be width/height")
    return AssertionResult(name, old != new, "pane geometry changed" if old != new else "pane geometry unchanged")


def zoomed_flag_is(value: object, *, expected: bool = True, name: str = "zoom") -> AssertionResult:
    normalized: bool | None
    if isinstance(value, bool):
        normalized = value
    elif isinstance(value, int) and value in (0, 1):
        normalized = bool(value)
    elif isinstance(value, str) and value.strip().lower() in {"1", "true", "yes", "on"}:
        normalized = True
    elif isinstance(value, str) and value.strip().lower() in {"0", "false", "no", "off"}:
        normalized = False
    else:
        normalized = None
    passed = normalized is not None and normalized == expected
    detail = "zoom flag matched" if passed else "zoom flag did not match"
    return AssertionResult(name, passed, detail)
