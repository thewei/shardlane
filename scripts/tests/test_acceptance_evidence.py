"""
[INPUT]: temporary evidence paths and public acceptance-evidence CLI arguments.
[OUTPUT]: unit-test evidence for bounded JSONL recording and summary validation.
[POS]: no-GUI ledger regression suite; it never starts Herdr or touches user data.
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "acceptance-evidence.py"


class EvidenceLedgerTests(unittest.TestCase):
    def _run(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *arguments],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_record_and_summary_form_a_bounded_public_contract(self) -> None:
        with tempfile.TemporaryDirectory(prefix="shardlane-evidence-test-") as directory:
            path = Path(directory) / "evidence.jsonl"
            recorded = self._run(
                "record",
                str(path),
                "--driver",
                "computer-use",
                "--phase",
                "input",
                "--status",
                "pass",
                "--message",
                "marker observed",
                "--metric",
                "pane_count=1",
            )
            summary = self._run("summary", str(path), "--require-phase", "input")

            self.assertEqual(recorded.returncode, 0, recorded.stderr)
            self.assertEqual(summary.returncode, 0, summary.stderr)
            event = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(event["schema"], 1)
            self.assertEqual(event["metrics"], {"pane_count": 1.0})

    def test_summary_rejects_invalid_control_plane_rows(self) -> None:
        with tempfile.TemporaryDirectory(prefix="shardlane-evidence-test-") as directory:
            path = Path(directory) / "evidence.jsonl"
            path.write_text('{"schema":1,"driver":"native"}\n', encoding="utf-8")

            result = self._run("summary", str(path))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("invalid", result.stderr)


if __name__ == "__main__":
    unittest.main()
