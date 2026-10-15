// DEVELOPMENT STAND-IN for the Greenhouse kernel crate (`greenhouse-kernel`).
//
// This is not the kernel. It is never copied into jacepero/greenhouse. It holds
// only the slice of the kernel crate that Rust apps use as their SDK, so that
// body-double-app builds and tests outside the Greenhouse repo. In Greenhouse,
// kernel/ is the real crate, and these five modules are its own.
//
// The real kernel could not be read here. Everything below comes from
// transcripts of the owner's own sessions in that repo. Where it comes from:
//
// - clock.rs: VERBATIM. The Write of kernel/src/clock.rs at
//   2026-09-26T02:07:31Z, formatted with `rustfmt --edition 2021` as the
//   session did at 02:10:53Z before commit b5794a4. No later edit is recorded.
//   It matches every line number and text of the 2026-09-29 greps and seds
//   (571 lines).
// - error.rs: VERBATIM, the whole file. `sed -n 1,60p` on main,
//   2026-09-25T16:52Z; lines 1-40 are unchanged on 2026-10-08T00:52Z.
// - identity.rs: VERBATIM through line 263. Greenhouse-c3 `sed -n 1,133p;
//   164,230p` plus `grep -A30 'pub fn mint'`, 2026-09-16T22:30Z; the same
//   verify_app_request on main 2026-09-24 and 2026-09-29, and the test key
//   helper from main 2026-09-29. The rest of its tests are reconstructed.
// - cron.rs: VERBATIM except lines 41-63 of parse_field, which are
//   reconstructed to fit the captured lines around them (main seds and greps
//   2026-09-26 and 2026-10-14, and the PR #29 diff of 2026-10-09). Most tests
//   are reconstructed.
// - validate.rs: PARTLY RECONSTRUCTED. The types, the name predicates and the
//   parsing of events, secrets, egress, model_compute, notify_categories and
//   schedules are verbatim fragments (main and wt-expansion, 2026-09-12 to
//   2026-10-14). The id, version, health, tools, ui and notify channel
//   parsing, the helpers and most tests are reconstructed from the captured
//   rules and error texts. Each such part is marked "Stand-in".
//
// An app that passes here can still be refused by the real kernel where a
// reconstruction differs. The live check is registration on the appliance.

pub mod clock;
pub mod cron;
pub mod error;
pub mod identity;
pub mod validate;
