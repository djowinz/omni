//! `omni-admin keygen` — generate a fresh admin Ed25519 keypair on disk.

use clap::Args as ClapArgs;
use omni_identity::Keypair;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Output path for the new admin keypair (e.g. `%APPDATA%\Omni\admin-identity.key`).
    #[arg(long)]
    pub output: PathBuf,
}

pub async fn run(args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    if args.output.exists() {
        anyhow::bail!(
            "refusing to overwrite existing key file: {}",
            args.output.display()
        );
    }
    // load_or_create generates + persists (and hardens ACL on Windows).
    let kp = Keypair::load_or_create(&args.output)?;
    // Belt-and-suspenders on Unix (load_or_create does this on Windows via acl::set_user_only).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&args.output, std::fs::Permissions::from_mode(0o600))?;
    }
    let pub_hex = hex::encode(kp.public_key().0);
    if cli.json {
        println!(
            "{}",
            serde_json::json!({
                "pubkey_hex": pub_hex,
                "path": args.output.display().to_string(),
            })
        );
    } else {
        println!("Wrote admin keypair to: {}", args.output.display());
        println!("Admin pubkey (hex): {pub_hex}");
        println!();
        println!("Add this hex to the Worker `OMNI_ADMIN_PUBKEYS` env var (comma-separated)");
        println!("and redeploy. Then move the key file to its permanent location.");
    }
    Ok(ExitCode::SUCCESS)
}
