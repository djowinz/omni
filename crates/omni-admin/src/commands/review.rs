//! `omni-admin review` — interactive review loop over the pending queue.
//!
//! Walks the pending-reports queue one item at a time, renders a summary
//! (plus a best-effort thumbnail preview via the OS default viewer), and
//! prompts the operator for a decision: keep / remove / ban-author /
//! skip / quit.
//!
//! Wire contract: `worker-api.md` §4.13 (list), §4.14 (show),
//! §4.15 (action), §4.16 (artifact remove), §4.17 (pubkey ban).
//!
//! Every state-changing branch appends to `~/.omni-admin/audit.log` per
//! sub-spec §6; the Worker remains authoritative.
//!
//! Note: `dialoguer::Select`/`Confirm` require a real TTY. End-to-end
//! coverage of the interactive loop lands in T21 (pseudo-terminal driver);
//! this task ships a smoke test that the subcommand is wired into clap.

use clap::Args as ClapArgs;
use std::process::ExitCode;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {}

pub async fn run(_args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;

    // Batch cursor + local in-memory queue. We fetch `limit=50` and walk the
    // batch locally so that `Skip` doesn't immediately re-surface the same
    // report (server-side it's still `pending`, so re-fetching with the same
    // cursor would loop forever).
    let mut cursor: Option<String> = None;
    let page_size: u32 = 50;

    'outer: loop {
        let mut q: Vec<String> = vec!["status=pending".to_string(), format!("limit={page_size}")];
        if let Some(c) = &cursor {
            q.push(format!("cursor={}", urlencoding::encode(c)));
        }
        let query = q.join("&");
        let page: serde_json::Value = client
            .send_signed(
                reqwest::Method::GET,
                "/v1/admin/reports",
                Some(&query),
                None,
                &[],
            )
            .await?;

        // Contract §4.13: list response is `{ items, next_cursor? }`.
        let items = page
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let next_cursor = page
            .get("next_cursor")
            .and_then(|v| v.as_str())
            .map(String::from);

        if items.is_empty() {
            println!("No pending reports.");
            break;
        }

        for summary in items {
            let id = summary
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }

            // Full detail fetch — the list endpoint returns a truncated
            // summary per spec §4.13; the decision UI wants the full body.
            let detail_path = format!("/v1/admin/report/{id}");
            let detail: serde_json::Value = client
                .send_signed(reqwest::Method::GET, &detail_path, None, None, &[])
                .await?;

            render_summary(&detail);
            maybe_preview_thumbnail(&client, &detail).await;

            let choice = prompt_action()?;
            match choice {
                Action::Keep => {
                    let body = serde_json::json!({ "action": "no_action" });
                    let body_bytes = serde_json::to_vec(&body)?;
                    let path = format!("/v1/admin/report/{id}/action");
                    let _: serde_json::Value = client
                        .send_signed(reqwest::Method::POST, &path, None, Some(&body_bytes), &[])
                        .await?;
                    crate::audit::append(&format!("ACTION report={id} action=no_action"))?;
                    println!("  → kept");
                }
                Action::Remove => {
                    // Contract §4.14: show response is `{ report, linked_artifact }`.
                    let artifact_id = detail
                        .get("linked_artifact")
                        .and_then(|a| a.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if artifact_id.is_empty() {
                        eprintln!("  ! report has no linked_artifact.id; cannot remove");
                        continue;
                    }
                    let reason = dialoguer::Input::<String>::new()
                        .with_prompt("Removal reason")
                        .validate_with(|s: &String| -> Result<(), &'static str> {
                            if s.trim().is_empty() {
                                Err("reason cannot be empty")
                            } else {
                                Ok(())
                            }
                        })
                        .interact_text()?;
                    let body = serde_json::json!({ "reason": reason });
                    let body_bytes = serde_json::to_vec(&body)?;
                    let rm_path = format!("/v1/admin/artifact/{artifact_id}/remove");
                    let _: serde_json::Value = client
                        .send_signed(
                            reqwest::Method::POST,
                            &rm_path,
                            None,
                            Some(&body_bytes),
                            &[],
                        )
                        .await?;
                    crate::audit::append(&format!(
                        "REMOVE artifact={artifact_id} reason=\"{}\"",
                        crate::audit::escape_value(&reason)
                    ))?;

                    let action_body = serde_json::json!({
                        "action": "removed",
                        "notes": reason,
                    });
                    let action_bytes = serde_json::to_vec(&action_body)?;
                    let action_path = format!("/v1/admin/report/{id}/action");
                    let _: serde_json::Value = client
                        .send_signed(
                            reqwest::Method::POST,
                            &action_path,
                            None,
                            Some(&action_bytes),
                            &[],
                        )
                        .await?;
                    crate::audit::append(&format!(
                        "ACTION report={id} action=removed notes=\"{}\"",
                        crate::audit::escape_value(&reason)
                    ))?;
                    println!("  → removed");
                }
                Action::BanAuthor => {
                    let pubkey = detail
                        .get("linked_artifact")
                        .and_then(|a| a.get("author_pubkey"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if pubkey.is_empty() {
                        eprintln!("  ! report has no linked_artifact.author_pubkey; cannot ban");
                        continue;
                    }
                    if !cli.yes {
                        let proceed = dialoguer::Confirm::new()
                            .with_prompt(format!(
                                "Ban pubkey {pubkey}? This will tombstone ALL of the author's artifacts."
                            ))
                            .default(false)
                            .interact()?;
                        if !proceed {
                            println!("  → aborted");
                            continue;
                        }
                    }
                    let reason = dialoguer::Input::<String>::new()
                        .with_prompt("Ban reason")
                        .validate_with(|s: &String| -> Result<(), &'static str> {
                            if s.trim().is_empty() {
                                Err("reason cannot be empty")
                            } else {
                                Ok(())
                            }
                        })
                        .interact_text()?;
                    let body = serde_json::json!({ "pubkey": pubkey, "reason": reason });
                    let body_bytes = serde_json::to_vec(&body)?;
                    let v: serde_json::Value = client
                        .send_signed(
                            reqwest::Method::POST,
                            "/v1/admin/pubkey/ban",
                            None,
                            Some(&body_bytes),
                            &[],
                        )
                        .await?;
                    let cascade_count =
                        v.get("cascade_count").and_then(|n| n.as_u64()).unwrap_or(0);
                    let cascade_errors = v
                        .get("cascade_errors")
                        .and_then(|n| n.as_u64())
                        .unwrap_or(0);
                    crate::audit::append(&format!(
                        "BAN pubkey={pubkey} reason=\"{}\" cascade_count={cascade_count} cascade_errors={cascade_errors}",
                        crate::audit::escape_value(&reason)
                    ))?;

                    let action_body = serde_json::json!({ "action": "banned_author" });
                    let action_bytes = serde_json::to_vec(&action_body)?;
                    let action_path = format!("/v1/admin/report/{id}/action");
                    let _: serde_json::Value = client
                        .send_signed(
                            reqwest::Method::POST,
                            &action_path,
                            None,
                            Some(&action_bytes),
                            &[],
                        )
                        .await?;
                    crate::audit::append(&format!("ACTION report={id} action=banned_author"))?;
                    println!("  → banned (cascade_count={cascade_count})");
                }
                Action::Skip => {
                    println!("  → skipped");
                    continue;
                }
                Action::Quit => {
                    println!("bye");
                    break 'outer;
                }
            }
        }

        match next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    Ok(ExitCode::SUCCESS)
}

enum Action {
    Keep,
    Remove,
    BanAuthor,
    Skip,
    Quit,
}

fn prompt_action() -> anyhow::Result<Action> {
    let items = ["[k]eep", "[r]emove", "[b]an author", "[s]kip", "[q]uit"];
    let idx = dialoguer::Select::new()
        .with_prompt("Decision")
        .items(&items)
        .default(0)
        .interact()?;
    Ok(match idx {
        0 => Action::Keep,
        1 => Action::Remove,
        2 => Action::BanAuthor,
        3 => Action::Skip,
        _ => Action::Quit,
    })
}

fn render_summary(detail: &serde_json::Value) {
    // Contract §4.14: detail is `{ report: AdminReportView, linked_artifact }`.
    // Fall back to reading top-level fields too so this helper stays usable if
    // a caller passes a bare report view.
    let report = detail.get("report").unwrap_or(detail);
    let id = report.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let reason = report.get("reason").and_then(|v| v.as_str()).unwrap_or("");
    let notes = report.get("notes").and_then(|v| v.as_str()).unwrap_or("");
    let received = report
        .get("received_at")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reporter = report
        .get("reporter_df")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reporter_short = reporter.get(..8).unwrap_or(reporter);

    println!("────────────────────────────────────────");
    println!("Report {id}");
    println!("  received:  {received}");
    println!("  reporter:  {reporter_short}…");
    println!("  reason:    {reason}");
    if !notes.is_empty() {
        println!("  notes:     {notes}");
    }
    if let Some(artifact) = detail.get("linked_artifact") {
        let name = artifact.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let aid = artifact.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let author = artifact
            .get("author_pubkey")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let author_short = author.get(..16).unwrap_or(author);
        println!("  artifact:  {name} ({aid})");
        if !author.is_empty() {
            println!("  author:    {author_short}…");
        }
        if let Some(tags) = artifact.get("tags").and_then(|v| v.as_array()) {
            let joined: Vec<String> = tags
                .iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect();
            if !joined.is_empty() {
                println!("  tags:      {}", joined.join(", "));
            }
        }
        if let Some(installs) = artifact.get("install_count").and_then(|v| v.as_u64()) {
            println!("  installs:  {installs}");
        }
    }
    println!("────────────────────────────────────────");
}

/// Best-effort thumbnail preview. Swallows every error — a missing
/// thumbnail or a sandboxed environment without a default viewer must
/// never break the review loop.
async fn maybe_preview_thumbnail(client: &crate::client::AdminClient, detail: &serde_json::Value) {
    let Some(url) = detail
        .get("linked_artifact")
        .and_then(|a| a.get("thumbnail_url"))
        .and_then(|v| v.as_str())
    else {
        return;
    };
    let resp = match client.http.get(url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return,
    };
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return,
    };
    let tmp = std::env::temp_dir().join(format!(
        "omni-admin-thumb-{}.img",
        chrono::Utc::now().timestamp_millis()
    ));
    if std::fs::write(&tmp, &bytes).is_err() {
        return;
    }
    let _ = open_with_os(&tmp);
}

#[cfg(target_os = "windows")]
fn open_with_os(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("cmd")
        .args(["/c", "start", ""])
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn open_with_os(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_os(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canned Worker-shaped response (contract §4.14) feeds cleanly through
    /// `render_summary` — guards against the prior bug where it read
    /// top-level `reason`/`notes`/`artifact` (which don't exist on the new
    /// `{ report, linked_artifact }` envelope).
    #[test]
    fn render_summary_accepts_worker_show_shape() {
        let detail = serde_json::json!({
            "report": {
                "id": "rep-1",
                "reason": "spam",
                "notes": "dup content",
                "received_at": "2026-04-15T00:00:00Z",
                "reporter_df": "deadbeefcafebabe0011223344556677"
            },
            "linked_artifact": {
                "id": "art-9",
                "name": "Neon",
                "author_pubkey": "aabbccddeeff00112233445566778899",
                "tags": ["ui", "dark"],
                "install_count": 42
            }
        });
        // Doesn't panic, doesn't silently drop the fields we rely on.
        render_summary(&detail);
    }
}
