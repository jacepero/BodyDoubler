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
node --test llmplayground/tests/               # the SayLLM widgets
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
