# 0006 — Two-tier snapshot comparison

Status: accepted · Date: 2026-11-20 · Evidence: [P4](../probes/p4.md)

## Context
The SPEC wanted software-adapter renders to match byte for byte across platforms. P4 measured one
fixed scene (MSAA edges, gradients, blending, bilinear filtering, sRGB) on five renderers. Each renderer
was deterministic with itself, but no two agreed byte for byte — not even lavapipe at Mesa 25.2 vs
26.2. Every difference was at most 2/255 per channel, yet present in 80–99% of 8x8 blocks, so a
block-count tolerance cannot separate rounding from a defect.

## Decision
Layer-3 snapshots compare in two tiers:
1. **Exact tier.** Baselines are rendered by lavapipe at a pinned Mesa version inside the pinned CI
   container image (the image `leonis ci local` also uses). Comparison is byte-exact. The Mesa
   version is recorded beside the baselines; changing it re-baselines in the same commit, with the
   diff report in the commit or PR.
2. **Cross-platform tier.** Every other renderer — WARP, Metal, the Android emulator, the iOS
   simulator, real GPUs — is compared against the exact-tier baseline with a per-channel maximum
   difference of **2/255** by default. No block-count measure. A larger tolerance for a snapshot
   requires a new measurement on record (the CI run that measured it).

## Alternatives considered
- **Byte-exact everywhere**: refuted by P4.
- **Per-platform baselines**: five sets of images to review on every change, and nothing says the
  platforms agree with each other.
- **Block-fraction tolerance (maps-tauri)**: P4's rounding noise touches up to 99.7% of blocks, so any
  passing threshold is too loose to catch defects.
- **Perceptual metrics (SSIM etc.)**: harder to explain in a failure message and to set from a
  measurement; revisit if text or images need it.

## Consequences
- A one-unit defect is caught in the exact tier only; the cross-platform tier catches anything above
  2/255. SPEC R6 has a planted control for each tier.
- Text is unmeasured: P4 had no glyphs. M1 re-measures with text and records any larger tolerance.
- The exact tier depends on the pinned container, which ties layer 3 to `leonis ci local`
  (SPEC R15/R24); a developer without the container gets cross-platform-tier comparisons only.
