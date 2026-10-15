# Body Double for Greenhouse

The iOS Body Double (the rest of this repository), distilled into a registered Greenhouse app.
Everything under `greenhouse/` mirrors the layout of `jacepero/greenhouse`, so each file copies
across to the same path.

- **What it is and why:** [`docs/design/body-double-proposal.md`](docs/design/body-double-proposal.md).
  It is a proposal; nothing in it is decided until the owner answers its decisions.
- **What it does:** [`apps/body-double.md`](apps/body-double.md). This covers the tools, results,
  check-ins, storage, manifest and configuration.

## What is here

| Path | Goes to Greenhouse? | What |
|---|---|---|
| `body-double-app/` | yes, as a workspace member | The Rust app: crate `body-double-app`, binary `greenhouse-body-double`, port 4107 |
| `apps/body-double.md` | yes | The app doc (D15) |
| `docs/design/body-double-proposal.md` | yes | The proposal with its decisions for the owner |
| `deploy/greenhouse-body-double.service`, `deploy/body-double/`, `deploy/body-double.md` | yes | The VM140 unit, egress set, example configuration, installer and runbook |
| `deploy/greenhouse.sysusers.d/body-double.conf` | its one line goes into `deploy/greenhouse.sysusers` | The app's principal |
| `appliance/` | new files yes; `appliance/INTEGRATION.md` lists edits to existing ones | The Fedora bootc image additions |
| `llmplayground/public/body-double-widgets.js`, `llmplayground/tests/body-double-widgets.test.js` | yes | SayLLM's rail page and Home widgets |
| `llmplayground/INTEGRATION.md`, `llmplayground/public/body-double.css` | apply by hand | Edits to existing SayLLM files, and the CSS to merge |
| `kernel/` | **no** | A development stand-in for the kernel crate's app SDK (see below) |
| `Cargo.toml`, `Cargo.lock` | **no** | This folder's development workspace |

## Building here

```bash
cd greenhouse
cargo test                                     # the app and the kernel stand-in
cargo clippy -p body-double-app --all-targets --no-deps -- -D warnings
rustfmt --edition 2021 --check body-double-app/src/*.rs body-double-app/tests/*.rs
node --test llmplayground/tests/*.test.js      # the SayLLM widgets
```

Greenhouse apps use the kernel crate as their SDK (`greenhouse-kernel = { path = "../kernel" }`):
the identity verifier, the error type, the manifest validator and the clock. The Greenhouse
repository could not be cloned into this session. So `kernel/` here reconstructs exactly that
slice from the owner's own sessions in that repository:

- `clock.rs` and `error.rs` are verbatim;
- `identity.rs` is verbatim through the verifier, and some of its tests are reconstructed;
- `cron.rs` is verbatim apart from a reconstructed part of one function;
- `validate.rs` is partly rebuilt: the types and half the parsing are verbatim, and the rest
  follows the captured rules and error texts.

`kernel/src/lib.rs` says which is which, with dates.

In Greenhouse the same path dependency resolves to the real kernel crate, which is the build that
counts. Run the checks above in the real workspace before anything is deployed.

## Into `jacepero/greenhouse`

1. Copy `body-double-app/`, `apps/body-double.md`, `docs/design/body-double-proposal.md` and the
   `deploy/` files. Add `"body-double-app"` to the root `Cargo.toml` members. Let Cargo add the
   crate's lock entry; it needs no crate the lock does not already have.
2. Run `cargo test -p body-double-app` and the clippy line above against the real kernel crate.
3. Follow [`deploy/body-double.md`](deploy/body-double.md) for VM140, and
   [`appliance/INTEGRATION.md`](appliance/INTEGRATION.md) for the bootc image.
4. Apply [`llmplayground/INTEGRATION.md`](llmplayground/INTEGRATION.md) to SayLLM.

Approving the contract, setting grants and deploying SayLLM's front end are the owner's acts.

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
   deploy/greenhouse.sysusers.d/body-double.conf to deploy/greenhouse.sysusers. Build with
   --locked; if Cargo wants to change any existing lock entry, stop and report it instead.
4. Run cargo test -p body-double-app, then
   cargo clippy -p body-double-app --all-targets --no-deps -- -D warnings, then
   rustfmt --edition 2021 --check on the crate's files. The crate was built against a
   reconstruction of kernel/src/{error,identity,validate,cron,clock}.rs. Fix only real API
   differences in body-double-app, and report each one.
5. Apply llmplayground/INTEGRATION.md to SayLLM (the new module, its test, and the listed
   edits). Run SayLLM's own tests.
6. Do not edit appliance/ yet. Report appliance/INTEGRATION.md's edits as pending.
7. Commit, without deploying. Do not register, approve or grant anything, and do not touch
   production. The proposal's decisions are the owner's.

Report the exact SHA, the test results and every difference from the reconstruction.
If anything in these instructions does not reconcile with the repository state you observe,
the available tooling, the filesystem, or what you are actually able to do, stop immediately
and request guidance before making further changes. Do not infer, improvise, or silently
substitute a different workflow.
```
