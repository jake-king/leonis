# 0004 — The `leonis` CLI owns CI logic; workflows are thin shims

Status: accepted · Date: 2026-11-20

## Context
maps-tauri's CI worked once its logic moved into one script (`tools/forge-job.sh`) that the workflows
called, so a job ran the same way locally (`tools/ci-local.sh`, a container) and in CI. The failures
came from logic in the workflow itself: a verdict job that exited 0 on a red leg, a job that worked
only on a warm cache, a perf baseline that silently compared nothing.

## Decision
Every CI job is a `leonis ci <job>` subcommand. `leonis ci init` writes workflows for GitHub,
Forgejo or GitLab whose steps only call those subcommands. `leonis ci local <job>` runs the same job
in the same Linux container. The aggregate verdict is computed by `leonis ci verdict`, which fails if
any required leg failed or did not run.

## Alternatives considered
- **Document example workflows**: the common practice; drifts per project, logic duplicated per CI host.
- **A dedicated CI service**: out of scope (SPEC non-goals).

## Consequences
- Fixing CI behaviour is a leonis release, not an edit in every app's workflow files.
- macOS and Windows jobs cannot run in `leonis ci local`; they run only on hosted runners.
- SPEC R13–R15 are the gates: no exit 0 on zero checks, the verdict fails on a red leg, generated
  workflows contain only `leonis ci` steps.
