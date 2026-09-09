"""
[INPUT]: synthetic tmux capture, geometry, zoom values, and assertion functions.
[OUTPUT]: unit-test evidence for composable backend-truth assertions.
[POS]: no-GUI regression suite; it verifies observable behavior rather than implementation details.
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
"""

from __future__ import annotations

import unittest

from scripts.acceptance.assertions import (
    AssertionCheck,
    AssertionResult,
    contains_marker,
    geometry_changed,
    pane_count,
    pane_count_is,
    run_assertions,
    zoomed_flag_is,
)


class BackendAssertionTests(unittest.TestCase):
    def test_marker_assertion_uses_captured_backend_text(self) -> None:
        self.assertTrue(contains_marker("shell\nMARKER-123\n", "MARKER-123").passed)
        self.assertFalse(contains_marker("shell\n", "MARKER-123").passed)

    def test_pane_count_is_composable_without_tmux_side_effects(self) -> None:
        capture = "pane-0\n pane-1\n\n"

        self.assertEqual(pane_count(capture), 2)
        self.assertTrue(pane_count_is(capture, 2).passed)
        self.assertFalse(pane_count_is(capture, 1).passed)

    def test_geometry_assertion_requires_a_real_dimension_change(self) -> None:
        self.assertTrue(geometry_changed((80, 24), (100, 24)).passed)
        self.assertFalse(geometry_changed((80, 24), (80, 24)).passed)

    def test_zoom_assertion_accepts_tmux_boolean_values(self) -> None:
        self.assertTrue(zoomed_flag_is("1").passed)
        self.assertTrue(zoomed_flag_is("0", expected=False).passed)
        self.assertFalse(zoomed_flag_is("0").passed)

    def test_assertion_checks_form_a_small_scenario_suite(self) -> None:
        suite = run_assertions(
            (
                AssertionCheck("input_echo", lambda: contains_marker("ok MARKER", "MARKER", name="input_echo")),
                AssertionCheck("pane_zoom", lambda: zoomed_flag_is("1", name="pane_zoom")),
            )
        )

        self.assertTrue(suite.passed)
        self.assertEqual([item.name for item in suite.results], ["input_echo", "pane_zoom"])
        self.assertEqual(suite.as_dict()["failed"], 0)

    def test_failed_assertion_keeps_a_bounded_reason(self) -> None:
        result = AssertionResult(name="marker", passed=False, detail="marker missing")

        self.assertLessEqual(len(result.detail), 240)
        self.assertFalse(run_assertions((AssertionCheck("marker", lambda: result),)).passed)


if __name__ == "__main__":
    unittest.main()
