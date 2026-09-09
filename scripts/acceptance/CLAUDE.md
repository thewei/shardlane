# acceptance/
> L2 | Parent: ../CLAUDE.md

Members

- `capabilities.py` — UI-side-effect-free capability status, driver guard
  probes, composable reports, and the read-only Computer Use MCP probe code
  executed by Agents.
- `assertions.py` — marker / pane-count / geometry-change / zoom assertions
  over tmux and backend output; starts no processes and touches no UI.
- `__init__.py` — the stable Python seam exporting the capability probes and
  backend assertions.

Rules: composable capabilities · backend output is the ground truth ·
judge the driver first · probes produce no side effects
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
