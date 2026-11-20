# 0003 — The accessibility tree is the test selector

Status: accepted · Date: 2026-11-20

## Context
UI tests need a way to find widgets. Pixel coordinates break on any layout change; internal IDs test
the implementation rather than what the user sees. The web's Testing Library showed that selecting by
role + accessible name gives stable tests and checks accessibility as a side effect.

## Decision
Headless app tests (layer 2) find widgets by accessibility role + accessible name and assert on the
accessibility tree (role, name, value, state). Internal IDs are available only as an escape hatch,
and the debug build counts their use.

## Alternatives considered
- **Test IDs on widgets**: stable, but says nothing about accessibility.
- **Pixel/coordinate scripts**: brittle; kept only for layer 3/4 snapshots.

## Consequences
- An unnamed interactive widget is both an accessibility bug and an untestable widget (SPEC R7).
- The accessibility tree must exist on the virtual platform, without an OS adapter (built in M2).
- Renaming a visible label changes tests — intended: the visible contract changed.
