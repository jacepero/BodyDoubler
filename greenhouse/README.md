# Body Double for Greenhouse

The iOS Body Double (the rest of this repository), distilled into a registered Greenhouse app.
Everything under `greenhouse/` mirrors the layout of `jacepero/greenhouse`, so each file copies
across to the same path.

- **In Greenhouse:** brought into `jacepero/greenhouse` on branch `feature/body-double`, from main
  at `a69f96d` (commits `d34e5eb`, the app; `f6012bd`, SayLLM; `a62caaa`, docs), 2026-10-16. That branch is now the
  copy that counts; this folder is the staging record it came from.
- **What it is and why:** [`docs/design/body-double-proposal.md`](docs/design/body-double-proposal.md).
  The owner answered every decision in chat on 2026-10-16; the scope entry is the owner's to record
  in the register.
- **What it does:** [`apps/body-double.md`](apps/body-double.md). This covers the tools, results,
  check-ins, storage, manifest and configuration.

## What is here

| Path | Goes to Greenhouse? | What |
|---|---|---|
| `body-double-app/` | yes, as a workspace member | The Rust app: crate `body-double-app`, binary `greenhouse-body-double`, port 4107 |
| `apps/body-double.md` | yes | The app doc (D15) |
| `docs/design/body-double-proposal.md` | yes | The proposal with its decisions for the owner |
| `deploy/greenhouse-body-double.service`, `deploy/body-double/`, `deploy/body-double.md` | yes | The VM140 unit, egress set, example configuration, installer and runbook |
| `deploy/greenhouse.sysusers.d/body-double.conf` | its one line is merged into `deploy/greenhouse.sysusers` (done on `feature/body-double`) | The app's principal |
| `appliance/` | new files yes; `appliance/INTEGRATION.md` lists edits to existing ones | The Fedora bootc image additions |
| `llmplayground/public/body-double-widgets.js`, `llmplayground/tests/body-double-widgets.test.js` | yes | SayLLM's rail page and Home widgets |
| `llmplayground/tests/body-double-chat.test.js` | yes | The Body double persona task in chat. It runs only inside SayLLM, with the patch applied |
| `llmplayground/INTEGRATION.md`, `llmplayground/public/body-double.css` | apply by hand | Edits to existing SayLLM files, and the CSS to merge |
| `kernel/` | **no** | A development stand-in for the kernel crate's app SDK (see below) |
| `Cargo.toml`, `Cargo.lock` | **no** | This folder's development workspace |

## Building here

```bash
cd greenhouse
cargo test                                     # the app and the kernel stand-in
cargo clippy -p body-double-app --all-targets --no-deps -- -D warnings
rustfmt --edition 2021 --check body-double-app/src/*.rs body-double-app/tests/*.rs
node --test llmplayground/tests/body-double-widgets.test.js   # the SayLLM cards
```

`llmplayground/tests/body-double-chat.test.js` imports SayLLM's own modules, so it runs only
inside SayLLM, with the patch applied (`llmplayground/INTEGRATION.md`).

Greenhouse apps use the kernel crate as their SDK (`greenhouse-kernel = { path = "../kernel" }`):
the identity verifier, the error type, the manifest validator, the cron parser and the clock.
`kernel/` here holds exactly those five files, copied verbatim from `jacepero/greenhouse` main at
`d176a23`, so the app builds in this folder. In Greenhouse the same path dependency resolves to the
real kernel crate.

## Checked against the real Greenhouse

On 2026-10-15, with `jacepero/greenhouse` main at `d176a23` cloned read-only, in a scratch copy of
that workspace (nothing in the Greenhouse repository was changed):

- **Build and tests:** `body-double-app` added as a workspace member, with
  `deploy/body-double/` beside it (a config test reads the shipped example). With the real kernel
  crate, `cargo test -p body-double-app` gives 36 unit and 21 integration tests passing.
  `cargo clippy -p body-double-app --all-targets --no-deps -- -D warnings` is clean, with
  rustc 1.97.0. The manifest test runs the real `validate_manifest`.
- **Lock:** `Cargo.lock` gains only the `body-double-app` entry. No existing entry changes and no
  new crate appears.
- **Contract, read in the real source:**
  - manifest rules, registration and identity verification;
  - the scheduler's call (body `{}`, `system:scheduler`, `command`);
  - tool dispatch (wire names, the description prefix, results passed whole, the audit's
    arguments);
  - `/api/notify` and `/api/notify/retire`, and the proxy (path stripping, data-plane
    identity, relayed `content-disposition`).

  All match what the app does.
- **Deploy and appliance:** every `greenhousectl` and `greenhouse-admin` command and flag in the
  runbook exists as written. Every Before text in `appliance/INTEGRATION.md` occurs once in the
  real files. With the edits applied: `nft -c` accepts the egress file, and systemd-sysusers and
  systemd-tmpfiles create the user and the directory.
- **SayLLM:** every Before text in `llmplayground/INTEGRATION.md` occurs once in the real files.
  With the patch applied, SayLLM's own suite goes from 411 of 412 passing to 451 of 452. The new
  tests pass: the cards, and the Body double persona task's reserve and tool scope. The one
  failure is the same test before and after; it needs `deploy/`, which the copy lacked. The suite
  ran with stand-ins for `express` and `better-sqlite3`, since no packages could be installed. The Body double page rendered in Chromium from the real `home.js`, `widgets.js`
  and `styles.css`, under SayLLM's own content security policy.

What the comparison found wrong was fixed in this folder. The deploy and SayLLM notes say where.

## Into `jacepero/greenhouse`

1. Copy `body-double-app/`, `apps/body-double.md`, `docs/design/body-double-proposal.md` and the
   `deploy/` files. Add `"body-double-app"` to the root `Cargo.toml` members.
   - In the same commit, add `COPY body-double-app body-double-app` to `appliance/Containerfile`'s
     `greenhouse-rust` stage (the first edit in `appliance/INTEGRATION.md`). The image build
     copies only the crates it lists, and Cargo cannot load the workspace with a member
     missing. The copy alone builds nothing.
   - Build once without `--locked` so Cargo adds the crate's own lock entry. `git diff
     Cargo.lock` should then show only that entry, because the crate needs no crate the lock
     lacks. Use `--locked` from then on.
2. Run `cargo test -p body-double-app` and the clippy line above against the real kernel crate.
3. Follow [`deploy/body-double.md`](deploy/body-double.md) for VM140, and
   [`appliance/INTEGRATION.md`](appliance/INTEGRATION.md) for the bootc image.
4. Apply [`llmplayground/INTEGRATION.md`](llmplayground/INTEGRATION.md) to SayLLM.

Approving the contract, setting grants and deploying SayLLM are the owner's acts. The SayLLM patch
changes its server (`src/`) as well as `public/`, so it needs a restart. A `public/`-only deploy
would offer the Body double task while every chat still got the app's tools at full cost.

### Instructions for a Claude session in the Greenhouse repository

```text
Bring the Body Double app from jacepero/BodyDoubler (branch
claude/body-doubler-greenhouse-app-82rrdq, folder greenhouse/) into this repository as a
proposal-stage change. Read greenhouse/README.md, greenhouse/apps/body-double.md and
greenhouse/docs/design/body-double-proposal.md there first.

1. Verify the current branch, HEAD and a clean worktree here. Make a new branch for this work.
2. Copy, keeping paths: body-double-app/, apps/body-double.md,
   docs/design/body-double-proposal.md, deploy/greenhouse-body-double.service,
   deploy/body-double/, deploy/body-double.md. Do NOT copy greenhouse/kernel/ or
   greenhouse/Cargo.toml or Cargo.lock from BodyDoubler; they are a stand-in for this repo's
   kernel crate.
3. Add "body-double-app" to the root Cargo.toml members, and add the line in
   deploy/greenhouse.sysusers.d/body-double.conf to deploy/greenhouse.sysusers. In the same
   commit, add `COPY body-double-app body-double-app` after `COPY weather-app weather-app` in
   appliance/Containerfile's greenhouse-rust stage (appliance/INTEGRATION.md, step 1, first
   edit): the image build cannot load the workspace without it. Then run
   cargo test -p body-double-app --offline once without --locked so Cargo adds the crate's own
   lock entry, and check git diff Cargo.lock. Only the body-double-app package entry may
   appear. If any existing entry changes or a new third-party crate appears, stop and report it.
   Commit the updated Cargo.lock; builds after that use --locked.
4. Run cargo test -p body-double-app, then
   cargo clippy -p body-double-app --all-targets --no-deps -- -D warnings, then
   rustfmt --edition 2021 --check on the crate's files. They passed read-only against main at
   d176a23 (greenhouse/README.md, "Checked against the real Greenhouse"). If anything fails on
   the current main, fix only real API differences in body-double-app, and report each one.
5. Apply llmplayground/INTEGRATION.md to SayLLM (the new module, its two tests, and the listed
   edits). Run SayLLM's own tests.
6. Make no other appliance/ edit yet. Report appliance/INTEGRATION.md's remaining edits as
   pending.
7. Commit, without deploying. Do not register, approve or grant anything, and do not touch
   production. The proposal's decisions are the owner's.

Report the exact SHA, the test results and every difference from what README.md records.
If anything in these instructions does not reconcile with the repository state you observe,
the available tooling, the filesystem, or what you are actually able to do, stop immediately
and request guidance before making further changes. Do not infer, improvise, or silently
substitute a different workflow.
```
