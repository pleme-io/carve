# carve

Monolithic-branch → ticket-aligned stacked-PR primitive (caixa Binario kind).

> **Theory:** `pleme-io/theory/CARVE.md` (TODO — write once execute lands)
> **Operator doc:** `pleme-io/docs/carve.md` (TODO)
> **Skill:** `pleme-io/blackmatter-claude/skills/carve/SKILL.md` (TODO)
> **Pipeline sibling:** [vitrine](../vitrine) — pre-merge evidence delivery

This CLI automates the operator side of the carve pattern: taking a
single branch where development happened freely and producing the
stacked PRs the team needs to review. The pattern itself was codified
in May 2026 from the ASM-18003 DBK staging asia-southeast1 rollout,
which we did entirely by hand — that surgery is the v0.1 regression
fixture.

## Subcommands (all implemented in v0.1)

```bash
carve plan --epic <KEY>        # walk branch, fetch JIRA epic children,
                               # emit plan.yaml shell for operator editing
carve verify                   # dry-run invariants on plan.yaml
carve execute                  # backup tag + branches + tree-hash gate + push + PRs
carve jira-sync                # story points + transitions, policy-capped
carve restack --from <BRANCH>  # replay descendants after parent-PR fix
carve diagram                  # idempotent PR-body stack diagram refresh
carve gate --pr <N>            # CI hook: refuse out-of-order merge
carve status                   # stack health snapshot
```

## Configuration

Per-team/per-repo TOML, layered:

```
1. Built-in defaults
2. ~/.config/carve/config.toml     (user-global)
3. <repo>/.carve.toml              (repo-local — wins)
```

Key knobs:
- `jira.story_points_field` — custom-field id (default `customfield_10016`)
- `jira.points_per_day` — operator-days → story points scale (default 1.0)
- `jira.max_auto_transition` — policy cap on auto-transitions (default `"In Review"`)
- `jira.transition_ids` — optional pinning of transition ids by name

## Build / Run

```bash
nix build .#carve                       # builds the binary
nix run  .#carve -- --help              # invocation
nix run  .#carve -- plan --epic ASM-18003   # generate plan shell
```

For pure cargo dev (no nix):

```bash
cargo build --workspace
cargo test  --workspace
cargo run --bin carve -- --help
```

## Conventions

- Rust edition 2021, MIT license (workspace defaults)
- clap derive for CLI dispatch
- shikumi-style typed config via the `carve-types` crate
- BLAKE3-attested backups via cordel-borrowed pattern
- substrate's `rust-workspace-release-flake.nix` for multi-platform release
- caixa-native (caixa.lisp declares Binario kind)
- Module trio auto-emitted by the substrate flake — consumers
  `imports = [ carve.homeManagerModules.default ]; programs.carve.enable = true;`

## Workspace layout

```
crates/
  carve-types/       # data model: Plan, TicketScope, CrossCuttingCommit, etc.
  carve/             # CLI binary
```

## Plan-centric design

Every command reads or writes a `plan.yaml`. The plan is operator-editable;
this is where commit-to-ticket *judgment* lives. Carve never silently
changes scope — it proposes, the operator decides, and execute applies
exactly what's in the YAML (after invariant checks).

## What this binary deliberately doesn't do

- **Doesn't enable GitHub merge queue** — that needs a repo admin and is
  org-policy. `carve gate` is the CI fallback for when merge queue isn't
  available, not a replacement for it.
- **Doesn't auto-merge PRs** — operator always presses the button.
- **Doesn't rewrite history on the source branch** — the original branch +
  the BLAKE3-attested backup tag are *always* preserved.
- **Doesn't decide commit-to-ticket assignment unilaterally** — for any
  commit that touches paths in multiple ticket scopes, carve emits a
  cross-cutting entry and refuses execute until the operator chooses
  a SplitDecision.
- **Doesn't store credentials** — JIRA + GitHub auth come from
  operator-level env / `gh` CLI.

## Regression fixture

The ASM-18003 stack (`asm18003-pre-split-backup` tag + 6 published
sub-tickets ASM-18005..ASM-18010) is the v0.1 ground-truth regression test:

```bash
cargo test --workspace --test asm18003_fixture
```

This test gives carve the original tip + ticket scopes and asserts the
generated plan + executed stack tree-hash to exactly match what we built
by hand in May 2026.
