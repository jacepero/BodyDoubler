// DEVELOPMENT STAND-IN for the Greenhouse kernel crate (`greenhouse-kernel`).
//
// This is not the kernel. It is never copied into jacepero/greenhouse. It holds
// only the slice of the kernel crate that Rust apps use as their SDK, so that
// body-double-app builds and tests outside the Greenhouse repo. In Greenhouse,
// kernel/ is the real crate, and these five modules are its own.
//
// All five files are VERBATIM copies of jacepero/greenhouse main at d176a23
// (2026-10-14), kernel/src/{clock,cron,error,identity,validate}.rs, tests
// included. They were first reconstructed from transcripts of the owner's
// sessions, then replaced with the real files once the repository could be
// read (2026-10-15). To refresh them, copy the same five files again.
//
// The check that counts is the real workspace: body-double-app was built and
// tested there against the whole kernel crate (README.md, "Checked against the
// real Greenhouse").

pub mod clock;
pub mod cron;
pub mod error;
pub mod identity;
pub mod validate;
