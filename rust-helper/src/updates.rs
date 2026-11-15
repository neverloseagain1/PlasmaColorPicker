//! Compare the packaged source baseline with GitHub while excluding README files.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

#[derive(Serialize)]
pub(super) struct UpdateFile {
    pub(super) path: String,
    pub(super) status: &'static str,
}

#[derive(Serialize)]
pub(super) struct UpdateCheckResult {
    pub(super) action: &'static str,
    pub(super) latest_commit: String,
    pub(super) files: Vec<UpdateFile>,
}

pub(super) fn check_repository_updates(
    baseline_path: &Path,
) -> Result<UpdateCheckResult, Box<dyn std::error::Error>> {
    let baseline = read_update_baseline(baseline_path)?;
    let commits: serde_json::Value = github_api_json(
        "https://api.github.com/repos/neverloseagain1/PlasmaColorPicker/commits?per_page=1",
    )?;
    let latest = commits
        .as_array()
        .and_then(|commits| commits.first())
        .ok_or("GitHub returned no repository commits")?;
    let latest_commit = latest
        .get("sha")
        .and_then(serde_json::Value::as_str)
        .ok_or("GitHub's latest commit has no SHA")?
        .to_string();
    let tree_sha = latest
        .get("commit")
        .and_then(|commit| commit.get("tree"))
        .and_then(|tree| tree.get("sha"))
        .and_then(serde_json::Value::as_str)
        .ok_or("GitHub's latest commit has no tree SHA")?;
    let tree_url = format!(
        "https://api.github.com/repos/neverloseagain1/PlasmaColorPicker/git/trees/{tree_sha}?recursive=1"
    );
    let tree: serde_json::Value = github_api_json(&tree_url)?;
    if tree
        .get("truncated")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Err("GitHub's repository file list was truncated".into());
    }

    let entries = tree
        .get("tree")
        .and_then(serde_json::Value::as_array)
        .ok_or("GitHub returned an invalid repository file list")?;
    let mut remote_files = BTreeMap::new();
    for entry in entries {
        if entry.get("type").and_then(serde_json::Value::as_str) != Some("blob") {
            continue;
        }
        let path = entry
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or("GitHub returned a file entry without a path")?;
        let sha = entry
            .get("sha")
            .and_then(serde_json::Value::as_str)
            .ok_or("GitHub returned a file entry without a SHA")?;
        if !is_readme_path(path) {
            remote_files.insert(path.to_string(), sha.to_string());
        }
    }

    Ok(UpdateCheckResult {
        action: "check-updates",
        latest_commit,
        files: compare_update_files(&baseline, &remote_files),
    })
}

pub(super) fn github_api_json(url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(ureq::get(url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "PlasmaColorPicker")
        .timeout(Duration::from_secs(12))
        .call()?
        .into_json()?)
}

pub(super) fn read_update_baseline(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let contents = fs::read_to_string(path)?;
    let mut files = BTreeMap::new();
    for (line_number, line) in contents.lines().enumerate() {
        let Some((sha, path)) = line.split_once('\t') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid update baseline entry on line {}", line_number + 1),
            ));
        };
        if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) || path.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid update baseline entry on line {}", line_number + 1),
            ));
        }
        if !is_readme_path(path) {
            files.insert(path.to_string(), sha.to_ascii_lowercase());
        }
    }
    if files.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The update baseline contains no code files",
        ));
    }
    Ok(files)
}

pub(super) fn is_readme_path(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|name| {
        let name = name.to_ascii_lowercase();
        name == "readme" || name.starts_with("readme.")
    })
}

pub(super) fn compare_update_files(
    baseline: &BTreeMap<String, String>,
    remote: &BTreeMap<String, String>,
) -> Vec<UpdateFile> {
    let mut changes = Vec::new();
    for (path, sha) in remote {
        if is_readme_path(path) {
            continue;
        }
        match baseline.get(path) {
            None => changes.push(UpdateFile {
                path: path.clone(),
                status: "added",
            }),
            Some(baseline_sha) if baseline_sha != sha => changes.push(UpdateFile {
                path: path.clone(),
                status: "changed",
            }),
            Some(_) => {}
        }
    }
    for path in baseline.keys() {
        if !is_readme_path(path) && !remote.contains_key(path) {
            changes.push(UpdateFile {
                path: path.clone(),
                status: "removed",
            });
        }
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    changes
}
