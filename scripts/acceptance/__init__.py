"""
[INPUT]: the capability probe and backend assertion modules under
scripts/acceptance.
[OUTPUT]: a stable, UI-side-effect-free composition entry point for the
scenario scripts.
[POS]: the Python public boundary of the UI acceptance toolkit; consumed by
both the shell orchestrators and the unit tests.
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
"""

from .assertions import (
    AssertionCheck,
    AssertionReport,
    AssertionResult,
    contains_marker,
    geometry_changed,
    pane_count,
    pane_count_is,
    run_assertions,
    zoomed_flag_is,
)
from .capabilities import (
    CAPABILITY_SCHEMA_VERSION,
    Availability,
    CapabilityCheck,
    CapabilityReport,
    CapabilityResult,
    REQUIRED_COMPUTER_USE_METHODS,
    check_command,
    check_computer_use_exports,
    check_global_capture,
    check_native_input,
    local_capability_report,
    mcp_probe_script,
    normalize_driver,
    run_checks,
)

__all__ = [
    "AssertionCheck",
    "AssertionReport",
    "AssertionResult",
    "CAPABILITY_SCHEMA_VERSION",
    "Availability",
    "CapabilityCheck",
    "CapabilityReport",
    "CapabilityResult",
    "REQUIRED_COMPUTER_USE_METHODS",
    "check_command",
    "check_computer_use_exports",
    "check_global_capture",
    "check_native_input",
    "contains_marker",
    "geometry_changed",
    "local_capability_report",
    "mcp_probe_script",
    "normalize_driver",
    "pane_count",
    "pane_count_is",
    "run_assertions",
    "run_checks",
    "zoomed_flag_is",
]
