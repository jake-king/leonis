# 0001 — Rust + wgpu + SDL3

Status: accepted · Date: 2026-11-20

## Context
leonis must render identically on Linux, macOS and Windows and ship with its own testing and release
tooling. maps-tauri showed the OS web view is the main source of per-platform differences (three
engines, spoofed GPU/core counts, timer rounding, font metrics). guile-2d showed that a thin platform
layer (SDL) plus your own renderer gives full control of timing and output. maps-tauri's data and
meshing code is already Rust.

## Decision
- Language: Rust.
- GPU: wgpu (Vulkan, Metal, DX12, plus software adapters for tests).
- Platform layer: SDL3 for windows, input, IME, clipboard, timing and audio.

## Alternatives considered
- **C + Guile (guile-2d's stack)**: best live-coding experience, but Guile's Windows support is weak (to
  be confirmed if this is ever reconsidered), and Rust's text/accessibility/layout crates are lost.
- **winit instead of SDL3**: the common Rust choice, and AccessKit ships a ready winit adapter. SDL3
  was chosen for its broader platform coverage (audio, gamepads, IME, a path to mobile) and because
  guile-2d's platform experience is with SDL. Cost: the AccessKit integration is ours to wire (probe P2).
- **Keep a web view, swap the engine (Electron, CEF)**: fixes engine differences, not font
  differences, and keeps the memory floor.

## Consequences
- Probes P1 (SDL3 → wgpu surface) and P2 (AccessKit without winit) gate everything past M0.
- Text shaping, IME widgets and accessibility are leonis's to build, not inherited from a browser.
- If P1 or P2 fails, this decision is revisited (winit is the fallback) before M1.
