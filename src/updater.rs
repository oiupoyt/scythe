use std::sync::{Arc, Mutex};
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GITHUB_REPO: &str = "oiupoyt/scythe";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub name: String,
    pub html_url: String,
    pub release_notes: String,
    pub has_update: bool,
}

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    body: Option<String>,
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse_nums = |s: &str| -> Vec<u64> {
        let clean = s.trim().trim_start_matches('v');
        clean
            .split('.')
            .filter_map(|p| p.split('-').next().and_then(|n| n.parse::<u64>().ok()))
            .collect()
    };
    let l_nums = parse_nums(latest);
    let c_nums = parse_nums(current);
    for (l, c) in l_nums.iter().zip(c_nums.iter()) {
        if l > c {
            return true;
        } else if l < c {
            return false;
        }
    }
    l_nums.len() > c_nums.len()
}

pub fn check_for_updates() -> Option<ReleaseInfo> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);

    // Run curl with a 4-second timeout to avoid blocking
    let output = std::process::Command::new("curl")
        .args([
            "-s",
            "-H", "User-Agent: scythe-updater",
            "-H", "Accept: application/vnd.github.v3+json",
            "--max-time", "4",
            &url,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let release: GitHubRelease = serde_json::from_slice(&output.stdout).ok()?;
    let clean_ver = release.tag_name.trim().trim_start_matches('v').to_string();
    let has_update = is_newer(&clean_ver, CURRENT_VERSION);

    Some(ReleaseInfo {
        tag_name: release.tag_name,
        version: clean_ver,
        name: release.name.unwrap_or_else(|| format!("Scythe v{}", CURRENT_VERSION)),
        html_url: release.html_url,
        release_notes: release.body.unwrap_or_default(),
        has_update,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate { version: String },
    Available(ReleaseInfo),
    Failed(String),
}

impl Default for UpdateStatus {
    fn default() -> Self {
        UpdateStatus::Idle
    }
}

pub fn spawn_update_check(status: Arc<Mutex<UpdateStatus>>) {
    if let Ok(mut lock) = status.lock() {
        *lock = UpdateStatus::Checking;
    }
    std::thread::spawn(move || {
        match check_for_updates() {
            Some(info) => {
                if let Ok(mut lock) = status.lock() {
                    if info.has_update {
                        *lock = UpdateStatus::Available(info);
                    } else {
                        *lock = UpdateStatus::UpToDate { version: info.version };
                    }
                }
            }
            None => {
                if let Ok(mut lock) = status.lock() {
                    *lock = UpdateStatus::Failed("Could not reach update server".to_string());
                }
            }
        }
    });
}

pub fn open_browser_url(url: &str) {
    let u = url.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", &u])
                .creation_flags(0x08000000)
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&u).spawn();
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&u).spawn();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparisons() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(is_newer("0.2.1", "0.2.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }
}
