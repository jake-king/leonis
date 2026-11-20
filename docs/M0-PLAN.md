# M0 plan — the seven probes first

Status: started 2026-11-20. Roles: Claude builds, the operator reviews.

Each probe is a throwaway program under `probes/pN-<slug>/` (its own crate, not the workspace) and a
transcript under `docs/probes/pN.md`: the command, the output, and the verdict. A probe's verdict is one
of **pass**, **fail** (and what the design does about it), or **partial** (which platforms are still
unproven). The SPEC's "Passes when" column is the bar.

## Where each probe can run

The dev box (Arch, Wayland session with Xwayland, NVIDIA GPU, SDL3 3.4.18 from pacman) can run the
Linux halves. Everything else needs hardware or runners the operator provides.

| Probe | Here (Linux) | Needs from the operator |
|---|---|---|
| P1 SDL3 → wgpu surface | X11 + Wayland | macOS + Windows: CI runners (a GitHub remote, as maps-tauri) or machines |
| P2 accessibility without winit | AT-SPI read + Orca | Windows (NVDA) and macOS (VoiceOver) machines |
| P3 IME | fcitx5/ibus + mozc, if installed | Windows + macOS machines with Japanese input |
| P4 software adapter parity | llvmpipe via `vulkan-swrast` (not installed: `sudo pacman -S vulkan-swrast`) | Windows runner for WARP |
| P5 SDL3 + wgpu on mobile | Android emulator (needs the Android SDK installed user-locally) | A Mac for the iOS simulator; one Android and one iOS device |
| P6 accessibility on mobile | `uiautomator dump` on the emulator | Mac + iOS device |
| P7 lifecycle | Android emulator | Devices + Mac |

## Order

1. **P1 on Linux** — the foundation every other probe builds on. Starts now.
2. **P4 Linux half** once `vulkan-swrast` is installed, then P2 Linux half (AT-SPI).
3. **CI remote decision** (operator): where macOS/Windows probes run.
4. **P5/P7 Android** after the Android SDK is installed.
5. iOS (P5–P7) when a Mac is available.

Nothing past M0 starts until every probe has a verdict (SPEC).
