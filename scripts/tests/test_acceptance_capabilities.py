"""
[INPUT]: deterministic command lookup, environment maps, and synthetic MCP exports.
[OUTPUT]: unit-test evidence for the capability probe public seam.
[POS]: no-GUI regression suite; it proves safety/status semantics without launching apps.
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
"""

from __future__ import annotations

import unittest
from pathlib import Path

from scripts.acceptance.capabilities import (
    Availability,
    CapabilityCheck,
    CapabilityResult,
    REQUIRED_COMPUTER_USE_METHODS,
    check_command,
    check_computer_use_exports,
    check_global_capture,
    check_native_input,
    mcp_probe_script,
    normalize_driver,
    local_capability_report,
    run_checks,
)


class CapabilityProbeTests(unittest.TestCase):
    def test_driver_aliases_normalize_to_one_public_value(self) -> None:
        self.assertEqual(normalize_driver(None), "computer-use")
        self.assertEqual(normalize_driver("computer_use"), "computer-use")
        self.assertEqual(normalize_driver("cg-event"), "native")

    def test_unknown_driver_is_rejected_before_side_effects(self) -> None:
        with self.assertRaisesRegex(ValueError, "unknown UI driver"):
            normalize_driver("guessing")

    def test_command_probe_distinguishes_present_and_missing_tools(self) -> None:
        present = check_command("tmux", lookup=lambda _: "/usr/bin/tmux")
        missing = check_command("tmux", lookup=lambda _: None)

        self.assertEqual(present.status, Availability.AVAILABLE)
        self.assertEqual(present.value, "/usr/bin/tmux")
        self.assertEqual(missing.status, Availability.UNAVAILABLE)

    def test_native_input_is_blocked_without_explicit_global_input_opt_in(self) -> None:
        result = check_native_input(
            driver="computer-use",
            env={},
            lookup=lambda _: "/usr/bin/swiftc",
        )

        self.assertEqual(result.status, Availability.BLOCKED)
        self.assertTrue(result.safe)
        self.assertFalse(result.required)

    def test_native_input_reports_unsafe_only_after_explicit_opt_in(self) -> None:
        result = check_native_input(
            driver="native",
            env={"SHARDLANE_ALLOW_GLOBAL_INPUT": "1"},
            lookup=lambda _: "/usr/bin/swiftc",
        )

        self.assertEqual(result.status, Availability.AVAILABLE)
        self.assertFalse(result.safe)

    def test_capture_requires_a_separate_explicit_opt_in(self) -> None:
        blocked = check_global_capture(
            driver="native",
            env={"SHARDLANE_ALLOW_GLOBAL_INPUT": "1"},
            lookup=lambda _: "/usr/sbin/screencapture",
        )
        available = check_global_capture(
            driver="native",
            env={
                "SHARDLANE_ALLOW_GLOBAL_INPUT": "1",
                "SHARDLANE_ALLOW_GLOBAL_CAPTURE": "1",
            },
            lookup=lambda _: "/usr/sbin/screencapture",
        )

        self.assertEqual(blocked.status, Availability.BLOCKED)
        self.assertEqual(available.status, Availability.AVAILABLE)
        self.assertFalse(available.safe)

    def test_computer_use_export_probe_has_unknown_and_resolved_states(self) -> None:
        unknown = check_computer_use_exports(None)
        complete = check_computer_use_exports(REQUIRED_COMPUTER_USE_METHODS)
        incomplete = check_computer_use_exports(("get_app_state",))

        self.assertEqual(unknown.status, Availability.UNKNOWN)
        self.assertEqual(complete.status, Availability.AVAILABLE)
        self.assertEqual(incomplete.status, Availability.UNAVAILABLE)

    def test_checks_compose_into_a_report_without_running_ui(self) -> None:
        report = run_checks(
            "computer-use",
            (
                CapabilityCheck(
                    "backend",
                    lambda: CapabilityResult(
                        name="backend",
                        status=Availability.AVAILABLE,
                        safe=True,
                        detail="fixture",
                    ),
                ),
                CapabilityCheck(
                    "mcp",
                    lambda: CapabilityResult(
                        name="mcp",
                        status=Availability.UNKNOWN,
                        safe=True,
                        detail="fixture",
                    ),
                ),
            ),
        )

        self.assertEqual(report.overall, Availability.DEGRADED)
        self.assertEqual([item.name for item in report.results], ["backend", "mcp"])
        self.assertEqual(report.as_dict()["driver"], "computer-use")

    def test_malformed_probe_result_fails_closed(self) -> None:
        result = CapabilityCheck("broken", lambda: None).run()  # type: ignore[return-value]

        self.assertEqual(result.status, Availability.UNAVAILABLE)
        self.assertTrue(result.safe)

    def test_mcp_probe_script_is_read_only_and_self_contained(self) -> None:
        script = mcp_probe_script()

        self.assertIn('import("@oai/sky")', script)
        self.assertIn("get_app_state", script)
        self.assertIn("nodeRepl.write", script)
        self.assertNotIn("sky.click", script)
        self.assertNotIn("sky.type_text", script)

    def test_local_report_can_merge_the_agent_host_mcp_result(self) -> None:
        report = local_capability_report(
            Path(__file__).resolve().parents[2],
            driver="computer-use",
            env={},
            lookup=lambda name: "/usr/bin/" + name,
            mcp_exports=REQUIRED_COMPUTER_USE_METHODS,
        )

        mcp = next(item for item in report.results if item.name == "computer-use:mcp")
        self.assertEqual(mcp.status, Availability.AVAILABLE)
        self.assertEqual(report.overall, Availability.AVAILABLE)
        self.assertFalse(next(item for item in report.results if item.name == "native:input").required)


if __name__ == "__main__":
    unittest.main()
