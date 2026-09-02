"""
[INPUT]: scripts/acceptance 下的能力探针与后端断言模块。
[OUTPUT]: 为场景脚本提供稳定、无 UI 副作用的组合入口。
[POS]: UI 验收工具包的 Python 公共边界；shell 编排器和单元测试共同消费。
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
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
