//! Subcommand modules. Each `<name>.rs` owns a `pub struct Args` and a
//! `pub async fn run(args, cli) -> anyhow::Result<ExitCode>`. T13–T20 fill
//! these in; T10 leaves them as `bail!("not yet implemented")` stubs.

use std::process::ExitCode;

use crate::{Cli, Cmd};

pub mod artifact;
pub mod device;
pub mod keygen;
pub mod limits;
pub mod pubkey;
pub mod reports;
pub mod review;
pub mod stats;
pub mod vocab;

/// Dispatch a parsed `Cli` to the appropriate subcommand handler.
///
/// Handlers receive their parsed `Args` by value and a borrow of the full
/// `Cli` for access to the global flags (`--key-file`, `--worker-url`,
/// `--yes`, `--json`). We clone the `Cmd` so we can hand each handler an
/// owned `Args` while still passing `&cli` — clap-parsed `Args` types
/// derive `Clone`, so this is cheap and avoids placeholder values that
/// break every time a subcommand's `Args` gains a required field.
pub async fn dispatch(cli: Cli) -> anyhow::Result<ExitCode> {
    let result: anyhow::Result<ExitCode> = match cli.cmd.clone() {
        Cmd::Keygen(a) => keygen::run(a, &cli).await,
        Cmd::Reports(a) => reports::run(a, &cli).await,
        Cmd::Artifact(a) => artifact::run(a, &cli).await,
        Cmd::Pubkey(a) => pubkey::run(a, &cli).await,
        Cmd::Device(a) => device::run(a, &cli).await,
        Cmd::Vocab(a) => vocab::run(a, &cli).await,
        Cmd::Limits(a) => limits::run(a, &cli).await,
        Cmd::Review(a) => review::run(a, &cli).await,
        Cmd::Stats(a) => stats::run(a, &cli).await,
    };
    // Spec §6 exit-code routing: a Worker-returned contract error envelope
    // must map to a deterministic exit code (Admin=2, Auth=3,
    // Malformed/Integrity=4, Io=5, Quota=6). Any other error (signing,
    // local I/O, decode) falls through to anyhow's default → exit 1.
    match result {
        Ok(code) => Ok(code),
        Err(e) => match e
            .downcast_ref::<crate::client::AdminError>()
            .and_then(|ae| match ae {
                crate::client::AdminError::Response {
                    status,
                    kind,
                    detail,
                    body,
                } => Some((status, kind, detail, body)),
                _ => None,
            }) {
            Some((status, kind, detail, body)) => {
                let code = crate::client::kind_to_exit_code(kind);
                let detail_str = detail
                    .as_ref()
                    .map(|d| format!(" detail={d}"))
                    .unwrap_or_default();
                eprintln!("HTTP {status}: kind={kind}{detail_str} — {body}");
                Ok(ExitCode::from(code as u8))
            }
            None => Err(e),
        },
    }
}
