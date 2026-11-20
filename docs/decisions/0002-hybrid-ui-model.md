# 0002 — Hybrid UI model: immediate-mode API over a retained tree

Status: accepted · Date: 2026-11-20

## Context
leonis is a general app framework: forms, lists and text editing as well as GPU-heavy visualizations.
Immediate-mode APIs (egui, guile-2d's ui module) are simple to write and suit debug tools and replay.
Accessibility, damage tracking and layout caching need persistent nodes, which immediate mode lacks.

## Decision
Apps describe their UI each frame through an immediate-mode API. The framework keeps a retained tree
keyed by stable widget IDs (call site + optional key), diffs each frame's description against it,
and derives layout, the accessibility tree and the draw list from the retained nodes, rebuilding only
what changed.

## Alternatives considered
- **Pure immediate mode (egui-style)**: simplest, but accessibility and minimal redraw are bolted on.
- **Pure retained (Qt/GTK-style widget objects)**: strong accessibility, heavier API, more state
  synchronisation bugs in app code.
- **Reactive signals (Xilem, Leptos-style)**: fine-grained updates, but a steeper model and harder to
  record/replay deterministically.

## Consequences
- Widget ID stability is a contract (SPEC R23): reordering without keys loses state, so lists must
  take keys.
- The diff step is a per-frame cost; it only runs on frames that redraw (SPEC R2).
- The accessibility tree is designed in M2 even though screen-reader support lands in M4.
