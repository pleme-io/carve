# carve

> Monolithic branch → ticket-aligned stacked-PR primitive.

Pre-PR sibling of [vitrine][vitrine] in the pleme-io delivery pipeline.
Vitrine ships *evidence* into a PR right before merge; carve produces the
*PRs themselves* — taking a single branch where everything was developed
and carving it into a stack of ticket-aligned, dependency-ordered pull
requests the team can actually review.

[vitrine]: https://github.com/pleme-io/vitrine

## Why this exists

The author's natural development flow is to work in one branch until
everything works. The team's review flow needs ticket-sized, scope-clean
chunks. Reconciling the two by hand — analysing 45 commits, deciding
which JIRA sub-ticket each one belongs to, splitting the cross-cutting
commits, ordering the stack, verifying nothing was lost, pushing the
branches, creating the PRs, syncing JIRA — is slow and error-prone.

Carve is the reliable, reproducible, attestable form of that workflow.

## Pipeline position

```
single branch  ──→  carve  ──→  stacked PRs  ──→  vitrine  ──→  merge
(operator dev)     (this tool)  (team review)   (pre-merge evidence)
```

## Subcommands

| Command           | Status   | Purpose                                                                       |
| ----------------- | -------- | ----------------------------------------------------------------------------- |
| `carve plan`      | v0.1 ✅  | Walk current branch, fetch JIRA epic children, emit `plan.yaml` for editing.  |
| `carve verify`    | v0.1 ✅  | Dry-run: invariant + cross-cutting + unassigned checks.                       |
| `carve execute`   | v0.2     | Build branches, BLAKE3-attested backup tag, tree-hash gate, push, open PRs.   |
| `carve jira-sync` | v0.3     | Story points + status transitions + ADF comments with PR links.               |
| `carve restack`   | v0.4     | Replay descendants after a fix lands on a parent PR.                          |
| `carve diagram`   | v0.5     | Idempotent ASCII stack-diagram regeneration inside PR bodies.                 |
| `carve gate`      | v0.5     | CI hook: refuse out-of-order merges.                                          |
| `carve status`    | v0.5     | Stack health snapshot: merge state, base drift, JIRA divergence.              |

## Quickstart

```bash
# 1. Generate a plan shell
carve plan --epic ASM-18003

# 2. Edit plan.yaml — populate `paths` globs per ticket scope
$EDITOR plan.yaml

# 3. Dry-run
carve verify

# 4. (v0.2) Apply
carve execute
```

## Plan file shape

`plan.yaml` is the single source of truth for every carve operation. It is
operator-editable between `plan` and `execute`; this is where commit-to-
ticket *judgment* is captured. Carve proposes; the operator decides;
carve executes attestably.

Top-level structure:

```yaml
meta:        { carve_version, generated_at, jira_epic, operator }
source:      { name, master_branch, tip, merge_base }
tickets:     [ TicketScope, ... ]      # one per JIRA sub-ticket
commits:     [ CommitFingerprint, ... ]  # frozen at plan time
assignments: [ CommitAssignment, ... ]   # commit → ticket
cross_cutting: [ CrossCuttingCommit, ... ]   # splits & operator decisions
stack:       { root, nodes: [ StackNode, ... ] }   # the dependency chain
```

The full schema is documented by the `carve-types` crate.

## Build

```bash
nix build .#carve
nix run  .#carve -- --help
```

## Auth

Carve never asks for credentials. It reads from operator-level auth that
already exists on the workstation:

- **JIRA**: `ATLASSIAN_BASE_URL`, `ATLASSIAN_EMAIL`, `ATLASSIAN_API_TOKEN`
- **GitHub**: `gh` CLI authenticated (`gh auth status` should succeed)
- **Git**: standard `~/.gitconfig` for `user.email`

## Family

- [vitrine][vitrine] — pre-merge evidence delivery
- [cordel][cordel] — typed attestable ops (BLAKE3-sealed pattern carve borrows)
- [shikumi][shikumi] — config discovery pattern carve borrows for types
- [dq][dq] — universal infrastructure data query

[cordel]: https://github.com/pleme-io/cordel
[shikumi]: https://github.com/pleme-io/shikumi
[dq]: https://github.com/pleme-io/dq
