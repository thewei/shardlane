"""
[INPUT]: UI driver name, environment opt-ins, local command/file lookup and an
         optional Computer Use export list returned by the host MCP.
[OUTPUT]: Bounded capability results/reports and a read-only @oai/sky probe script.
[POS]: UI acceptance capability seam; it does not launch apps, send input, or
       capture the display, so scenario scripts can compose it safely.
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
"""

from __future__ import annotations

import os
import shutil
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Callable, Iterable, Mapping


CAPABILITY_SCHEMA_VERSION = 1
MAX_DETAIL = 240


class Availability(str, Enum):
    """The observable state of one capability in a particular environment."""

    AVAILABLE = "available"
    BLOCKED = "blocked"
    UNKNOWN = "unknown"
    UNAVAILABLE = "unavailable"
    DEGRADED = "degraded"


DRIVER_ALIASES = {
    "computer-use": "computer-use",
    "computer_use": "computer-use",
    "cu": "computer-use",
    "native": "native",
    "cg-event": "native",
    "cg_event": "native",
}

REQUIRED_COMPUTER_USE_METHODS = (
    "target",
    "list_apps",
    "get_app_state",
    "click",
    "drag",
    "paste",
    "perform_secondary_action",
    "press_key",
    "scroll",
    "select_text",
    "set_value",
    "type_text",
)


def _bounded(value: str, field: str) -> str:
    if not isinstance(value, str):
        raise ValueError(f"{field} must be text")
    value = value.strip()
    if not value or len(value) > MAX_DETAIL or any(ord(char) < 0x20 for char in value):
        raise ValueError(f"{field} must be non-empty, single-line, and <= {MAX_DETAIL} chars")
    return value


def normalize_driver(value: str | None) -> str:
    """Normalize the one driver spelling shared by shell, Python, and docs."""

    raw = (value or "computer-use").strip().lower()
    try:
        return DRIVER_ALIASES[raw]
    except KeyError as error:
        raise ValueError(f"unknown UI driver: {value!r}") from error


@dataclass(frozen=True)
class CapabilityResult:
    """One bounded, serializable capability observation."""

    name: str
    status: Availability
    safe: bool
    detail: str
    value: str | None = None
    required: bool = True

    def __post_init__(self) -> None:
        _bounded(self.name, "capability name")
        _bounded(self.detail, "capability detail")
        object.__setattr__(self, "status", Availability(self.status))
        if not isinstance(self.required, bool):
            raise ValueError("capability required flag must be boolean")
        if self.value is not None:
            _bounded(self.value, "capability value")

    def as_dict(self) -> dict[str, object]:
        result: dict[str, object] = {
            "name": self.name,
            "status": self.status.value,
            "safe": self.safe,
            "required": self.required,
            "detail": self.detail,
        }
        if self.value is not None:
            result["value"] = self.value
        return result


@dataclass(frozen=True)
class CapabilityCheck:
    """A named probe that can be assembled into another scenario."""

    name: str
    probe: Callable[[], CapabilityResult]

    def run(self) -> CapabilityResult:
        try:
            result = self.probe()
        except Exception as error:  # noqa: BLE001 - a probe must fail closed.
            return CapabilityResult(
                name=self.name,
                status=Availability.UNAVAILABLE,
                safe=True,
                detail=f"probe error: {type(error).__name__}",
            )
        if not isinstance(result, CapabilityResult):
            return CapabilityResult(
                name=self.name,
                status=Availability.UNAVAILABLE,
                safe=True,
                detail="probe returned an invalid result",
            )
        if result.name != self.name:
            return CapabilityResult(
                name=self.name,
                status=Availability.UNAVAILABLE,
                safe=True,
                detail="probe returned a mismatched capability name",
            )
        return result


@dataclass(frozen=True)
class CapabilityReport:
    """A stable report consumed by humans, CI, and Agent orchestration."""

    driver: str
    results: tuple[CapabilityResult, ...]

    @property
    def overall(self) -> Availability:
        statuses = {result.status for result in self.results if result.required}
        if not statuses:
            return Availability.AVAILABLE
        if Availability.UNAVAILABLE in statuses:
            return Availability.UNAVAILABLE
        if statuses and statuses <= {Availability.AVAILABLE}:
            return Availability.AVAILABLE
        return Availability.DEGRADED

    def as_dict(self) -> dict[str, object]:
        return {
            "schema": CAPABILITY_SCHEMA_VERSION,
            "driver": self.driver,
            "overall": self.overall.value,
            "capabilities": [result.as_dict() for result in self.results],
        }


def run_checks(driver: str | None, checks: Iterable[CapabilityCheck]) -> CapabilityReport:
    """Run a caller-selected list of probes without adding UI side effects."""

    normalized = normalize_driver(driver)
    results = tuple(check.run() for check in checks)
    return CapabilityReport(driver=normalized, results=results)


def check_command(name: str, *, lookup: Callable[[str], str | None] = shutil.which) -> CapabilityResult:
    """Report whether an executable is discoverable; never execute it."""

    _bounded(name, "command name")
    path = lookup(name)
    if path:
        return CapabilityResult(
            name=f"command:{name}",
            status=Availability.AVAILABLE,
            safe=True,
            detail="executable discovered",
            value=path,
        )
    return CapabilityResult(
        name=f"command:{name}",
        status=Availability.UNAVAILABLE,
        safe=True,
        detail="executable not found",
    )


def _check_file(name: str, path: Path, *, executable: bool = False) -> CapabilityResult:
    if not path.is_file():
        return CapabilityResult(
            name=name,
            status=Availability.UNAVAILABLE,
            safe=True,
            detail="file not found",
        )
    if executable and not os.access(path, os.X_OK):
        return CapabilityResult(
            name=name,
            status=Availability.UNAVAILABLE,
            safe=True,
            detail="file is not executable",
        )
    return CapabilityResult(name=name, status=Availability.AVAILABLE, safe=True, detail="file present")


def check_native_input(
    *,
    driver: str | None,
    env: Mapping[str, str] | None = None,
    lookup: Callable[[str], str | None] = shutil.which,
    required: bool | None = None,
) -> CapabilityResult:
    """Check the guarded global-input path without posting an event."""

    selected = normalize_driver(driver)
    variables = os.environ if env is None else env
    is_required = selected == "native" if required is None else required
    if selected != "native":
        return CapabilityResult(
            name="native:input",
            status=Availability.BLOCKED,
            safe=True,
            detail="native global input is disabled for the Computer Use driver",
            required=is_required,
        )
    if variables.get("SHARDLANE_ALLOW_GLOBAL_INPUT") != "1":
        return CapabilityResult(
            name="native:input",
            status=Availability.BLOCKED,
            safe=True,
            detail="set SHARDLANE_ALLOW_GLOBAL_INPUT=1 for explicit opt-in",
            required=is_required,
        )
    swiftc = lookup("swiftc")
    if not swiftc:
        return CapabilityResult(
            name="native:input",
            status=Availability.UNAVAILABLE,
            safe=True,
            detail="swiftc is required to build the guarded event helper",
            required=is_required,
        )
    return CapabilityResult(
        name="native:input",
        status=Availability.AVAILABLE,
        safe=False,
        detail="explicit global input enabled; this can move the user pointer",
        value=swiftc,
        required=is_required,
    )


def check_global_capture(
    *,
    driver: str | None,
    env: Mapping[str, str] | None = None,
    lookup: Callable[[str], str | None] = shutil.which,
    required: bool = True,
) -> CapabilityResult:
    """Check the guarded shared-display capture path without taking a screenshot."""

    selected = normalize_driver(driver)
    variables = os.environ if env is None else env
    if selected != "native":
        return CapabilityResult(
            name="native:capture",
            status=Availability.BLOCKED,
            safe=True,
            detail="shared-display capture is disabled for the Computer Use driver",
            required=required,
        )
    if variables.get("SHARDLANE_ALLOW_GLOBAL_INPUT") != "1":
        return CapabilityResult(
            name="native:capture",
            status=Availability.BLOCKED,
            safe=True,
            detail="native capture requires the explicit global-input opt-in first",
            required=required,
        )
    if variables.get("SHARDLANE_ALLOW_GLOBAL_CAPTURE") != "1":
        return CapabilityResult(
            name="native:capture",
            status=Availability.BLOCKED,
            safe=True,
            detail="set SHARDLANE_ALLOW_GLOBAL_CAPTURE=1 for shared-display capture",
            required=required,
        )
    screencapture = lookup("screencapture")
    if not screencapture:
        return CapabilityResult(
            name="native:capture",
            status=Availability.UNAVAILABLE,
            safe=True,
            detail="screencapture is required for the native capture path",
            required=required,
        )
    return CapabilityResult(
        name="native:capture",
        status=Availability.AVAILABLE,
        safe=False,
        detail="explicit shared-display capture enabled; it can expose the user screen",
        value=screencapture,
        required=required,
    )


def check_computer_use_exports(
    exports: Iterable[str] | None,
    *,
    required: bool = True,
) -> CapabilityResult:
    """Resolve a host MCP export list; ``None`` means the shell cannot inspect MCP."""

    if exports is None:
        return CapabilityResult(
            name="computer-use:mcp",
            status=Availability.UNKNOWN,
            safe=True,
            detail="run the read-only mcp__node_repl__js probe in the Agent host",
            required=required,
        )
    provided = {str(item) for item in exports}
    missing = [name for name in REQUIRED_COMPUTER_USE_METHODS if name not in provided]
    if missing:
        return CapabilityResult(
            name="computer-use:mcp",
            status=Availability.UNAVAILABLE,
            safe=True,
            detail=f"missing Sky exports: {','.join(missing[:4])}",
            required=required,
        )
    return CapabilityResult(
        name="computer-use:mcp",
        status=Availability.AVAILABLE,
        safe=True,
        detail="host MCP exposes the required read/action API",
        value="@oai/sky",
        required=required,
    )


def mcp_probe_script() -> str:
    """Return a read-only node_repl snippet that another Agent can execute."""

    required = ", ".join(f'"{name}"' for name in REQUIRED_COMPUTER_USE_METHODS)
    return f'''globalThis.sky ??= (await import("@oai/sky")).sky;
nodeRepl.write(JSON.stringify((() => {{
  const required = [{required}];
  const capabilities = Object.fromEntries(required.map((name) => [
    name,
    name === "target" ? sky.target === "mac" : typeof sky[name] === "function",
  ]));
  return {{
    transport: "codex-computer-use",
    package: "@oai/sky",
    target: sky.target,
    capabilities,
    missing: required.filter((name) => !capabilities[name]),
  }};
}})()));'''


def _herdr_result(*, env: Mapping[str, str], lookup: Callable[[str], str | None]) -> CapabilityResult:
    explicit = env.get("HERDR_BIN")
    if explicit:
        path = Path(explicit)
        return _check_file("command:herdr", path, executable=True)
    discovered = lookup("herdr")
    if discovered:
        return CapabilityResult(
            name="command:herdr",
            status=Availability.AVAILABLE,
            safe=True,
            detail="executable discovered",
            value=discovered,
        )
    fallback_home = Path(env.get("HOME", str(Path.home())))
    fallback = fallback_home / ".local" / "bin" / "herdr"
    return _check_file("command:herdr", fallback, executable=True)


def local_capability_report(
    root: Path,
    *,
    driver: str | None = None,
    env: Mapping[str, str] | None = None,
    lookup: Callable[[str], str | None] = shutil.which,
    mcp_exports: Iterable[str] | None = None,
) -> CapabilityReport:
    """Build the standard no-GUI inventory used by ``verify.sh ui`` and Agents."""

    variables = dict(os.environ if env is None else env)
    selected = normalize_driver(driver or variables.get("SHARDLANE_UI_DRIVER"))
    root = Path(root)
    checks = (
        CapabilityCheck("command:tmux", lambda: check_command("tmux", lookup=lookup)),
        CapabilityCheck("command:swiftc", lambda: check_command("swiftc", lookup=lookup)),
        CapabilityCheck("command:screencapture", lambda: check_command("screencapture", lookup=lookup)),
        CapabilityCheck("command:herdr", lambda: _herdr_result(env=variables, lookup=lookup)),
        CapabilityCheck(
            "source:acceptance-evidence",
            lambda: _check_file("source:acceptance-evidence", root / "scripts" / "acceptance-evidence.py", executable=True),
        ),
        CapabilityCheck(
            "source:acceptance-capabilities",
            lambda: _check_file(
                "source:acceptance-capabilities",
                root / "scripts" / "acceptance-capabilities.py",
                executable=True,
            ),
        ),
        CapabilityCheck(
            "source:acceptance-package",
            lambda: _check_file("source:acceptance-package", root / "scripts" / "acceptance" / "__init__.py"),
        ),
        CapabilityCheck(
            "source:acceptance-probes",
            lambda: _check_file("source:acceptance-probes", root / "scripts" / "acceptance" / "capabilities.py"),
        ),
        CapabilityCheck(
            "source:acceptance-assertions",
            lambda: _check_file("source:acceptance-assertions", root / "scripts" / "acceptance" / "assertions.py"),
        ),
        CapabilityCheck(
            "source:ui-driver-guard",
            lambda: _check_file("source:ui-driver-guard", root / "scripts" / "ui-driver-guard.sh", executable=True),
        ),
        CapabilityCheck(
            "computer-use:mcp",
            lambda: check_computer_use_exports(mcp_exports, required=selected == "computer-use"),
        ),
        CapabilityCheck(
            "native:input",
            lambda: check_native_input(
                driver=selected,
                env=variables,
                lookup=lookup,
                required=selected == "native",
            ),
        ),
        CapabilityCheck(
            "native:capture",
            lambda: check_global_capture(
                driver=selected,
                env=variables,
                lookup=lookup,
                required=selected == "native" and variables.get("SHARDLANE_ALLOW_GLOBAL_CAPTURE") == "1",
            ),
        ),
    )
    return run_checks(selected, checks)
