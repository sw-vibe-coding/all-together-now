use std::path::Path;

use atn_core::error::{AtnError, Result};

/// Run `agentrail --saga <path> next` and return (stdout, exit_code).
pub async fn agentrail_next(saga_path: &Path) -> Result<(String, i32)> {
    run_agentrail(saga_path, &["next"]).await
}

/// Run `agentrail --saga <path> status` and return (stdout, exit_code).
pub async fn agentrail_status(saga_path: &Path) -> Result<(String, i32)> {
    run_agentrail(saga_path, &["status"]).await
}

/// Run `agentrail --saga <path> begin`.
pub async fn agentrail_begin(saga_path: &Path) -> Result<()> {
    let (_, code) = run_agentrail(saga_path, &["begin"]).await?;
    if code != 0 {
        return Err(AtnError::Trail(format!(
            "agentrail begin exited with code {code}"
        )));
    }
    Ok(())
}

/// Run `agentrail --saga <path> complete` with the given options.
pub async fn agentrail_complete(
    saga_path: &Path,
    summary: &str,
    next_slug: Option<&str>,
    next_prompt: Option<&str>,
    done: bool,
) -> Result<()> {
    let mut args = vec!["complete", "--summary", summary];

    if done {
        args.push("--done");
    } else if let Some(slug) = next_slug {
        args.push("--next-slug");
        args.push(slug);
        if let Some(prompt) = next_prompt {
            args.push("--next-prompt");
            args.push(prompt);
        }
    }

    let (_, code) = run_agentrail(saga_path, &args).await?;
    if code != 0 {
        return Err(AtnError::Trail(format!(
            "agentrail complete exited with code {code}"
        )));
    }
    Ok(())
}

/// Run `agentrail --saga <path> distill --task_type <type>` and return (stdout, exit_code).
pub async fn agentrail_distill(saga_path: &Path, task_type: &str) -> Result<(String, i32)> {
    run_agentrail(saga_path, &["distill", "--task_type", task_type]).await
}

async fn run_agentrail(saga_path: &Path, args: &[&str]) -> Result<(String, i32)> {
    let output = tokio::process::Command::new("agentrail")
        .arg("--saga")
        .arg(saga_path)
        .args(args)
        .output()
        .await
        .map_err(|e| AtnError::Trail(format!("failed to run agentrail: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let code = output.status.code().unwrap_or(-1);
    Ok((stdout, code))
}
