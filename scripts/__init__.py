"""
[INPUT]: Python modules under scripts/.
[OUTPUT]: Makes the repository scripts importable for isolated unit tests and
          reusable acceptance scenarios; it performs no work at import time.
[POS]: Package marker at the scripts root; shell entrypoints remain executable.
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
"""
