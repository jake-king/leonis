# leonis — spec

Status: draft, 2026-11-20, revised after an independent review the same day. Nothing here is built
yet. Every row in the acceptance map names the check that would prove it false and the milestone that
builds that check. What cannot be gated is listed under "Not gated" instead of being dressed up as a
gate.

## Problem / why

Cross-platform desktop apps today pick between two bad options:

- **A web view** (Tauri, Wails, Electron). Tauri and Wails use the OS web view, so one app runs on three
  engines (WebKitGTK, WKWebView, WebView2) with three sets of bugs. Measured in maps-tauri: a 293 MB
  floor for a blank window on WebKitGTK, a spoofed GPU and core count, 1 ms timer rounding, a
  title-bar-dependent window size on macOS, and font metrics that moved labels between platforms.
  Electron fixes the engine but not the fonts, and costs more memory.
- **A native toolkit** (Qt, Flutter, Slint, iced, egui). Rendering is consistent, but testing,
  packaging and release tooling are scattered or missing. Every app rebuilds them; maps-tauri
  hand-built snapshot comparison, network tape, a CI container, perf gates, signing, SBOMs and an
  updater.

leonis is a general Rust app framework whose output is the same on every platform and whose testing,
CI and release tooling ship with it as one CLI. It carries forward what guile-2d (thin platform layer,
input as data, frame clock, debug tools, record/replay) and maps-tauri (redraw-on-change, budgeted
uploads, determinism, cross-platform CI) proved.

Decisions: [0001 stack](decisions/0001-rust-wgpu-sdl3.md) · [0002 hybrid UI](decisions/0002-hybrid-ui-model.md) ·
[0003 accessibility tree as test selector](decisions/0003-accessibility-tree-is-the-test-selector.md) ·
[0004 the CLI owns CI logic](decisions/0004-cli-owns-ci-logic.md) ·
[0005 mobile is first-class](decisions/0005-mobile-is-first-class.md).

Prior art to read before M1: `fenestra` (crates.io, 0.41.0: winit + wgpu, flexbox/grid, headless PNG
rendering, a verification-harness crate). `oriel` (crates.io) is an empty name reservation with a
similar pitch, no code as of 2026-11-20.

## Targets

- Desktop: Linux (X11 + Wayland), macOS 14+, Windows 10/11, x86_64 and aarch64.
- Mobile: iOS 17+ and Android 10+ (API 29), first-class ([0005](decisions/0005-mobile-is-first-class.md)):
  every milestone delivers on all five platforms, and every test layer runs on them (on the iOS
  simulator and the Android emulator in CI).
- Web is a non-goal for v1.

"All platforms" below means these five.

## Architecture

```
app code (Rust)            immediate-mode API: ui.button("Save"), ui.list(items, |row| …)
  ▼
retained tree              stable widget IDs, state, diff → dirty set
  ├─ layout                taffy (flexbox + grid)
  ├─ accessibility         AccessKit tree from retained nodes
  ├─ events                input as data → hit test → focus → widgets
  └─ draw list             rebuilt only for dirty nodes
  ▼
renderer (wgpu)            instanced SDF quads, glyph atlas, images, custom wgpu passes
  ▼
platform (SDL3)            window, input (mouse, keyboard, touch, pen), IME + on-screen keyboard,
                           app lifecycle, safe areas, clipboard, timing, audio
                           — or the VIRTUAL platform (tests): fake clock, scripted input, no window
```

Workspace crates (names provisional): `leonis-platform`, `leonis-render`, `leonis-text`, `leonis-ui`,
`leonis-a11y`, `leonis-test` (virtual platform + app driver), `leonis-cli` (the `leonis` binary), and
the facade crate `leonis`.

## Observable behaviour

### Apps
- An app is a Rust crate that depends on `leonis` and implements one `App` trait (state + a `ui(&mut
  self, &mut Ui)` function). `leonis new <name>` creates one that builds, runs, and passes `leonis test`
  on all five platforms unmodified. The same app code runs on desktop and mobile; leonis supplies the
  mobile entry points (an iOS app delegate, an Android activity), so the app never writes them.
- Lifecycle: the app receives `suspended`, `resumed`, `surface_lost`, `surface_restored` and
  `low_memory` events. On suspend leonis stops drawing and offers the app a chance to save state; on
  surface loss it releases GPU resources tied to the surface and rebuilds them on restore. App state
  survives a suspend → resume cycle without app code.
- Input is pointer-based: mouse, touch and pen events carry a pointer ID and kind, so multi-touch and
  gestures (tap, long press, drag, pinch, fling scrolling) work on every platform. Hover-only
  interactions have a touch equivalent in every built-in widget.
- Layout respects safe areas (notches, rounded corners, system bars) and the on-screen keyboard: a
  focused text field is scrolled clear of the keyboard. Rotation and size changes relayout.
- The app redraws only when something changed: input, a timer it asked for, data it received, or an
  animation in progress. At rest it draws nothing.
- Text renders with fonts bundled into the app by default. System fonts are opt-in, per app.
- An app can embed custom wgpu rendering in a widget (a map, a chart, a 3D view) and receives the
  device, queue, target and a frame budget.

### Frame budget
The frame budget is the display's refresh interval (16.7 ms at 60 Hz; on the virtual platform, a
configured value, default 16.7 ms). A frame is **missed** when it takes more than 1.5 budgets — the
rule both guile-2d and maps-tauri arrived at, because timer rounding makes "over budget" noisy.

### Determinism
- Given the same build and the same inputs (recorded input, injected clock, seeded RNG, taped network),
  the virtual platform produces the same retained tree and the same draw list, byte for byte, on
  Linux, macOS and Windows.
- Pixels: on the software adapters (llvmpipe/lavapipe on Linux, WARP on Windows) the same draw list
  produces the same pixels, exactly or within a measured per-snapshot tolerance. macOS has no software
  adapter; its pixels are compared against Linux within a measured tolerance. The Android emulator
  (SwiftShader) and the iOS simulator are compared against Linux the same way. "Same everywhere" for
  pixels means exactly this, not more.

### Counters (exposed by the app driver and the debug socket in debug builds)
`frames_drawn`, `nodes_rebuilt`, `draw_commands_rebuilt`, `missed_frames`, `inbox_items_applied`,
`inbox_time_per_frame`, and `internal_id_selector_uses`. Several acceptance rows read these; they are
part of the testing interface, versioned like the formats.

### Tooling (`leonis` CLI)
Exit codes for every subcommand: `0` pass, `1` a check failed, `2` could not run (missing tool, wrong
tool version, no baseline, nothing to check). A run that checked nothing is exit 2, never 0.

| Command | Does |
|---|---|
| `leonis new <name>` | Create an app with tests and CI shims |
| `leonis run [--target <t>] [--record <file>]` | Run the app on the host, `android` (emulator or device), or `ios-sim`/`ios` ; optionally record input + clock + network |
| `leonis test [--layer 1..5]` | Run the test layers (below) |
| `leonis replay <file>` | Re-run a recording on the virtual platform, check its assertions |
| `leonis snapshot compare\|update` | Render snapshots, compare to the committed ones |
| `leonis bench [--against <ref>]` | Instruction-count benches + frame/idle/memory budgets vs a git ref (default: the repo's default branch) |
| `leonis ci <job>` | Run one CI job (the only thing generated workflows call) |
| `leonis ci local [<job>]` | Run a CI job in the same Linux container image CI uses |
| `leonis ci init github\|forgejo\|gitlab` | Write thin workflow shims that call `leonis ci <job>` |
| `leonis ci verdict` | Aggregate verdict: fails if any required leg failed or did not run |
| `leonis package <format>` | deb, rpm, AppImage, Flatpak, dmg, msi, apk, aab, ipa |
| `leonis release [--publish]` | Sign, notarize, checksums, SBOM, updater manifest; `--publish` uploads them |

### Test layers
1. **Logic**: `cargo test`, no window, no GPU.
2. **Headless app**: the virtual platform drives the app; tests select by accessibility role + name and
   assert on the accessibility tree ([0003](decisions/0003-accessibility-tree-is-the-test-selector.md)).
3. **Render snapshots**: offscreen wgpu on the software adapters.
4. **Platform end-to-end**: a real SDL3 window on real OS runners, the Android emulator and the iOS
   simulator: accessibility adapters (read through UI Automation, AT-SPI, the macOS AX API, Android's
   accessibility node tree via `uiautomator`, and XCUITest on iOS), DPI, window sizing, lifecycle,
   touch, real GPUs.
5. **Performance**: instruction counts, frame budgets, idle behaviour, memory.

### Debug tools (debug builds; compiled out of release builds)
Timed blocks + an overlay (F4), pause/single-step, a widget inspector, input record/replay with a
one-key save of the current session as a test file, screenshot, and a local socket for live queries
of the tree. Carried from guile-2d.

### Release-build reporting
Release builds keep one test hook: with `LEONIS_REPORT=1` the app prints status lines (`ready`, `idle`,
errors) to stdout, as maps-tauri's `report` command did. Nothing else from the debug tools ships.

## Interfaces + invariants

Callers may rely on:
- Widget IDs are stable across frames for the same call site + key; state is kept by ID. Reordering a
  keyed list keeps each row's state; reordering an unkeyed list is reported in debug builds.
- The accessibility tree is derived from the retained tree; every interactive built-in widget has a
  role and an accessible name.
- The render thread never blocks on I/O; background work reaches the UI through an inbox. Per frame
  the inbox applies items until its time budget (default 4 ms) is spent; it always applies at least
  one item so a single oversized item still makes progress.
- Every child process leonis starts (signers, packagers, notarizers) goes through one spawner, which
  passes secrets by environment or file, never argv.
- Recording format, snapshot format, counters and CLI exit codes are versioned; a reader refuses a
  newer version with exit 2 rather than misreading it.

Must never happen (the floor):
- F1. A command reports pass (exit 0) when it checked nothing.
- F2. The aggregate CI verdict passes while any required leg failed or did not run.
- F3. `leonis release --publish` uploads anything when a configured signing step failed or was skipped.
- F4. An updater installs a payload whose signature does not verify.
- F5. A secret reaches argv, logs, or a recording file.

## Non-goals (v1)
- Web/WASM.
- Uploading to the App Store or Google Play. `leonis release` produces signed, store-ready ipa/aab
  files; submission is the developer's step.
- Mobile platform features beyond the core app shell (push notifications, in-app purchase, camera,
  sensors) in v1. Apps reach them through their own platform code; leonis only must not block that.
- A web view widget or HTML/CSS rendering.
- A visual designer / drag-and-drop UI builder.
- Native-looking widgets. leonis looks the same everywhere by design; it follows platform *behaviour*
  (shortcuts, the macOS menu bar, scrolling feel) where users expect it.
- A scripting language. Live coding in v1 is dylib hot reload; a script layer (e.g. Steel) may be
  evaluated after M7.
- Hosting CI or providing runners/VMs.
- Providing signing credentials (Apple developer ID and provisioning profiles, Windows code-signing
  certificate, Android upload keystore). Without them, OS signing and notarization are reported as not
  run (exit 2), never as passed.
- Testing on physical phones in CI. CI uses the emulator and simulator; real devices are a manual
  per-release check (see "Not gated").

## M0 probes — assumptions to prove before building on them

Each is one throwaway program, run on every platform it names, with a transcript kept in
`docs/probes/`. Nothing past M0 starts until all seven have a recorded result and the design reflects
it.

| Probe | Question | Passes when |
|---|---|---|
| P1 | Do the Rust SDL3 bindings give wgpu a usable raw window handle? | A wgpu surface presents a cleared frame in an SDL3 window on Linux (X11 + Wayland), macOS, Windows |
| P2 | Can AccessKit's platform adapters be driven without winit? | A button's role + name is readable through UI Automation, AT-SPI and the macOS AX API, and Orca, VoiceOver and NVDA announce it |
| P3 | Does SDL3's IME path cover composition + candidate positioning? | Japanese input composes and commits in an SDL3 window on each desktop OS, candidate window at the caret, and the on-screen keyboard composes and commits on iOS and Android (by hand, with an IME installed) |
| P4 | Is software-adapter rendering identical across Linux and Windows? | One test scene renders byte-identical on llvmpipe and WARP. If not, the probe records the measured difference, and accepting a tolerance becomes a recorded design decision, not a silent pass |
| P5 | Does SDL3 + wgpu run on iOS and Android, built from Rust without hand-written native code? | A cleared frame presents on the iOS simulator, the Android emulator, and one physical device of each; the build is driven by a script, not an IDE |
| P6 | Can AccessKit (or our own adapter) expose the tree on Android and iOS? | A button's role + name is readable through `uiautomator dump` and XCUITest, and TalkBack/VoiceOver announce it. If AccessKit has no adapter for a platform, the probe sizes writing one, and that becomes a decision |
| P7 | Does the lifecycle survive real suspend/resume and surface loss? | On the emulator, simulator and both devices: background the app, return, rotate, and force a low-memory kill; the probe logs the SDL3 events received and shows the frame redraws after each |

## Milestones

| # | Delivers | Testing that ships with it |
|---|---|---|
| M0 | Probes P1–P7; workspace; window + clear + input on all five platforms; `leonis new`; `leonis run --target` | Layers 1 + 3 skeleton; `leonis ci <job>`, `ci local`, `ci init`, `ci verdict`; 5-platform CI matrix (emulator + simulator) |
| M1 | Renderer primitives (rect, rounded, border, shadow, image) + text | Cross-platform snapshot suite with tolerance lint |
| M2 | Retained tree, layout (incl. safe areas), pointer events + gestures, focus, lifecycle, redraw-on-change, inbox, debug tools, counters | Virtual platform + app driver (layer 2) with touch + lifecycle events, record/replay, layer 5 + `leonis bench` |
| M3 | Text input + IME + on-screen keyboard | IME composition and keyboard insets on the virtual platform |
| M4 | Accessibility on all five platforms | Role/name selectors; layer 4 tree reads |
| M5 | Widgets: virtualized list, scroll (fling on touch), menus, dialogs, forms | A settings-style sample app through all layers on all five platforms |
| M6 | Packaging + release (desktop formats, apk/aab, ipa) + desktop updater | `leonis release` on the sample app, verified install on each platform |
| M7 | Custom wgpu content + dylib hot reload | Port of the maps-tauri native renderer as the showcase |

## Acceptance map

Every floor row and every check that could pass vacuously has a planted defect (the **control**) that
must make it fail before its first pass counts.

| # | Requirement | Verified by | Control | M |
|---|---|---|---|---|
| R1 | `leonis new` app builds, runs, passes `leonis test` on all five platforms | CI job `new-app` on the 5-platform matrix (emulator + simulator for mobile) | — | M0 |
| R2 | An app at rest draws nothing | Layer 2: after input stops, `frames_drawn` over the next 300 virtual frames == 0. Layer 5 repeats it on a real window over 5 s; CPU use is reported, not gated | A widget that requests a redraw every frame must fail | M2 |
| R3 | Only dirty nodes are rebuilt | Layer 2: changing one list row gives `nodes_rebuilt == 1` and `draw_commands_rebuilt` == that row's count | A planted "mark everything dirty" must fail | M2 |
| R4 | Tree + draw list are deterministic | `leonis replay` of a fixed recording (≥ 600 frames, ≥ 50 distinct frame hashes required, else exit 2) hashes tree + draw list per frame; equal across two runs and across all five platforms (on mobile, replayed in the emulator and simulator) | An unseeded RNG and a HashMap-ordered child list, each planted, must change the hashes | M2 |
| R5 | Bundled fonts by default | Layer 3 snapshot in a container with no system fonts equals the default snapshot | Opting into system fonts in that container must fail or differ | M1 |
| R6 | Software-adapter pixels match within committed tolerances | `leonis snapshot compare` on Linux and Windows (and macOS vs Linux). `tests/snapshot-tolerance.toml`: every non-zero entry names the CI run that measured it; a lint fails entries without one | A one-block (8×8 px) colour change in a snapshot must fail at the committed tolerance | M1 |
| R7 | Every interactive built-in widget has a role + name | Layer 2 walks every built-in widget's accessibility node | A planted unnamed widget must fail | M2 |
| R8 | Tests select by role + name | Layer 2 API `app.find(Role::Button, "Save")`; `internal_id_selector_uses` is printed in the test summary | Renaming the button must fail the test | M2 |
| R9 | The accessibility tree reaches each platform | Layer 4: the sample app's tree read through UI Automation (Windows), AT-SPI (Linux), AX API (macOS), `uiautomator dump` (Android), XCUITest (iOS) matches the virtual-platform tree's roles + names | A widget whose name is dropped in the adapter must fail | M4 |
| R10 | IME composition is handled | Layer 2: composition start/update/commit/cancel events on the virtual platform produce the right text and caret | A planted handler that commits on update must fail | M3 |
| R11 | Background work never blocks the render thread | Layer 5: sample with a 500 ms blocking loader on a worker; over ≥ 600 frames, `missed_frames` == 0 | The same loader moved onto the render thread must fail | M2 |
| R12 | Inbox respects its time budget | Layer 2 with a fake clock and items of declared cost (0.5 ms each, plus one of 10 ms): each frame's applied cost ≤ 4 ms, except a frame that applied exactly one item | A planted unbudgeted drain must fail | M2 |
| R13 | Exit 0 never means "checked nothing" (F1) | `leonis-cli` tests, one per subcommand: test with no tests, replay with no assertions, snapshot compare with no committed image, bench with no baseline, ci verdict with zero legs, a missing or wrong-version tool → each exit 2 | Each case is itself the control | M0 (bench: M2) |
| R14 | Verdict fails on a failed or missing leg (F2) | In `leonis ci local`: generated workflow with a planted failing leg, and separately a planted skipped leg → verdict non-zero | Both plants are the controls | M0 |
| R15 | The same job runs locally and in CI | Lint: generated workflow steps are only checkout, install-leonis (pinned version) and `leonis ci <job>`. The image digest in the workflow equals the one `leonis ci local` uses | An extra `run:` step, and a mismatched digest, must each fail | M0 |
| R16 | Newer formats are refused | Unit tests: version N+1 recording, snapshot, counter set → exit 2 naming the version | — | M2 |
| R17 | Release artefacts are signed with checksums | `leonis release` with a throwaway key: signature + checksums verify. OS signing/notarization runs only with credentials; without them the step reports exit 2 | Tampered checksum file must fail verification | M6 |
| R18 | Nothing is published after a failed signing step (F3) | `leonis release --publish` against a fake upload target that records calls | A planted signing failure → zero upload calls | M6 |
| R19 | Updater refuses bad payloads (F4) | Update end-to-end test (maps-tauri `update-e2e.sh` pattern) | Wrong key, tampered payload, unsigned manifest — each refused | M6 |
| R20 | Secrets stay out of argv, logs, recordings (F5) | The spawner records every child's argv in test mode; a release run with a planted key value checks every recorded argv, all logs, and the output files. `leonis run --record` with a planted auth header checks the recording | The planted value is the control; a spawner bypass (direct `Command::new`) fails a lint | M6 (recording: M2) |
| R21 | Perf regressions fail CI | `leonis bench --against <default branch>`: instruction-count increase over the threshold fails. Threshold is set from measured run-to-run variance in M2 (maps-tauri measured ≤ 0.035% with gungraun; 3% is the starting value) | A planted slowdown must fail; no baseline → exit 2 | M2 |
| R22 | Debug tools exist only in debug builds | Layer 2: overlay, pause/step, inspector, save-recording reachable through the driver. Release: the build-info line (`LEONIS_REPORT=1`) lists compiled-in features; `debug-tools` must be absent | Building release with `debug-tools` enabled must fail the check | M2 |
| R23 | Widget state is kept by ID | Layer 2: reorder a keyed list, each row keeps its text-field contents; an unkeyed reorder raises the debug warning | A planted index-based ID must fail | M2 |
| R24 | `leonis ci local` matches CI's container | Covered by R15's digest check | (R15) | M0 |
| R25 | Custom wgpu content in a widget | maps-tauri native renderer inside a leonis widget, snapshot-tested like any widget | — | M7 |
| R26 | Hot reload of app code | Layer 4 scripted test: edit a label in the sample app → new label appears within 10 s, a counter's value unchanged. Local time-to-change is reported, not gated | A reload that resets state must fail | M7 |
| R27 | Packages install and launch on each platform | Per-format install in a clean VM/container/runner, emulator or simulator; the installed release build prints `ready` and `idle` with `LEONIS_REPORT=1` (on Android via logcat, on iOS via the simulator log) | A package missing the bundled fonts must fail | M6 |
| R28 | State survives suspend → resume and surface loss | Layer 2: virtual suspend, surface_lost, surface_restored, resume → tree hash equals the pre-suspend hash and the next frame draws. Layer 4: the same cycle driven on the emulator (`adb shell input keyevent HOME`, relaunch) and simulator (`xcrun simctl` background/launch) | A planted resume handler that drops state, and one that skips surface rebuild, must each fail | M2 (layer 4: M4) |
| R29 | Multi-touch and gestures work | Layer 2: recorded two-pointer pinch, long press, drag and fling produce the expected widget events and scroll positions. Layer 4: the same gestures injected on the emulator and simulator | A planted single-pointer-only input path must fail the pinch case | M2 |
| R30 | Every built-in widget is usable without hover | Layer 2 walks every built-in widget: each hover-triggered behaviour has a touch path reachable through the driver | A planted tooltip-only control must fail | M5 |
| R31 | Safe areas and the on-screen keyboard are respected | Layer 2: with virtual insets (notch, home indicator, keyboard), no interactive widget's bounds intersect an inset, and the focused text field is fully visible. Layer 3 snapshots at both orientations | A planted layout that ignores insets must fail | M3 |
| R32 | Mobile builds need no hand-written native code | `leonis new` app builds apk and ipa (simulator) in CI with no files outside the generated project; a lint fails on Java/Kotlin/Swift/Objective-C files in the app | A planted `.swift` file in the app must fail the lint | M0 |
| R33 | Mobile release artefacts are signed | `leonis release` with throwaway keys: apk/aab signature verifies (`apksigner verify`); ipa codesign verifies on a macOS runner when credentials exist, else exit 2 | A tampered apk must fail verification | M6 |

## Not gated (stays a human check)

- **Screen readers announcing widgets** (Orca, VoiceOver, NVDA): a per-release manual check with the
  transcript kept. R9 gates the tree each screen reader reads, not the speech.
- **Real OS IME behaviour** (candidate window placement, platform quirks): P3 by hand, then a
  per-release manual check. Hosted runners have no IME installed; R10 gates leonis's handling of
  composition events only.
- **Physical phones**: touch feel, performance, thermal throttling, and OEM differences on real
  Android and iOS devices. A per-release manual check on at least one device of each, transcript kept.
  The emulator and simulator gate behaviour, not feel.
- **TalkBack and iOS VoiceOver speech**: as for desktop screen readers above.
- **Spec completeness**: whether this list names every requirement that matters.

## Open questions
- SDL3 Rust bindings: which crate (`sdl3` vs raw `sdl3-sys`) — decided by P1.
- Text stack: parley vs cosmic-text — decided in M1 by a bidi/fallback/shaping test set.
- macOS snapshots: compare against Linux within a tolerance (current plan) or keep a macOS baseline —
  decided after the first M1 cross-platform run.
- Desktop updater on mobile: none — app stores own updates. Whether to support Android sideload
  updates is open.
- CI cost: iOS needs macOS runners and Android needs KVM-capable Linux runners for a usable emulator;
  measure job times in M0 before deciding which mobile legs run per push vs nightly.
- Licensing: decided 2026-11-20, MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).
