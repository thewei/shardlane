# tests/
> L2 | Parent: ../CLAUDE.md

Members

- `test_acceptance_capabilities.py` — verifies driver normalization,
  tool/permission probes, MCP export detection, and capability report
  composition.
- `test_acceptance_assertions.py` — verifies marker / pane / geometry /
  zoom backend-truth assertions and the scenario suites.
- `test_acceptance_evidence.py` — verifies evidence JSONL recording,
  metrics, and invalid-line rejection through the public CLI.

Rules: test only the public seam · no GUI side effects · failures stay
explainable
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
