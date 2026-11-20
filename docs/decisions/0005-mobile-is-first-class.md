# 0005 — Mobile (iOS, Android) is first-class

Status: accepted · Date: 2026-11-20

## Context
The first draft of the spec made mobile a non-goal and promised only that the design "must not make
mobile impossible". Nothing could check that promise, so it was dropped. The operator then decided
mobile should have first-class support: a general app framework that does not run on phones forces
apps to keep a second codebase, which is the scattered-tooling problem leonis exists to remove.

## Decision
iOS 17+ and Android 10+ are targets on the same footing as the three desktop platforms. Every
milestone delivers on all five; every test layer runs on them (iOS simulator and Android emulator in
CI); `leonis` builds, runs, packages and signs mobile apps with no hand-written native code in the
app. The App trait carries lifecycle events and pointer-based input from M2, and layout respects safe
areas and the on-screen keyboard.

## Alternatives considered
- **Mobile later, keep the path open**: a compile check for mobile targets plus lifecycle events in the
  API. Cheaper, but every desktop-first decision until then would be untested against mobile, and
  retrofitting lifecycle and touch is the usual cause of rewrites.
- **Mobile through a web view**: contradicts the reason for leonis (0001).

## Consequences
- M0 grows three probes (P5–P7): SDL3 + wgpu on mobile, accessibility on mobile, real lifecycle.
  AccessKit's mobile coverage is unverified; P6 may turn into writing an adapter.
- CI needs macOS runners (iOS simulator) and KVM-capable Linux runners (Android emulator); cost is
  measured in M0.
- Every built-in widget needs a touch path (SPEC R30); hover-only UI is a bug.
- Store submission, push notifications and device APIs stay out of v1 (SPEC non-goals).
- Physical-device feel and performance stay a manual per-release check.
