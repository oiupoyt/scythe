use std::sync::{Arc, Mutex};
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FORGEJO_HOST: &str = "https://git.oiupoyt.space";
pub const REPO_OWNER_NAME: &str = "oiupoyt/scythe";
pub const GITHUB_REPO: &str = "oiupoyt/scythe";

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub name: String,
    pub html_url: String,
    pub release_notes: String,
    pub has_update: bool,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize, Debug)]
struct ForgejoRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
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

/// Choose the release to offer between the one found so far and a candidate.
///
/// A newer version always wins. On equal versions the release carrying
/// more assets wins, so a mirror that is missing an installer does not hide
/// it from the user.
fn pick_newer_release(current: Option<ReleaseInfo>, candidate: ReleaseInfo) -> ReleaseInfo {
    match current {
        None => candidate,
        Some(current) => {
            let candidate_wins = is_newer(&candidate.version, &current.version)
                || (candidate.version == current.version
                    && candidate.assets.len() > current.assets.len());
            if candidate_wins { candidate } else { current }
        }
    }
}

pub fn check_for_updates() -> Option<ReleaseInfo> {
    let urls = [
        format!("{}/api/v1/repos/{}/releases/latest", FORGEJO_HOST, REPO_OWNER_NAME),
        format!("https://api.github.com/repos/{}/releases/latest", REPO_OWNER_NAME),
    ];

    // Every source is queried and the highest version wins. Returning on the
    // first answer would pin the updater to whichever host happens to be
    // listed first, so a release published on the other one would never be
    // seen.
    let mut best: Option<ReleaseInfo> = None;

    for url in urls {
        let mut cmd = std::process::Command::new("curl");
        cmd.args([
            "-s",
            "-H", "User-Agent: scythe-updater",
            "-H", "Accept: application/json",
            "--max-time", "6",
            &url,
        ]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        if let Ok(output) = cmd.output()
            && output.status.success()
            && let Ok(release) = serde_json::from_slice::<ForgejoRelease>(&output.stdout) {
                let clean_ver = release.tag_name.trim().trim_start_matches('v').to_string();
                let has_update = is_newer(&clean_ver, CURRENT_VERSION);

                let info = ReleaseInfo {
                    tag_name: release.tag_name,
                    version: clean_ver,
                    name: release.name.unwrap_or_else(|| format!("v{}", CURRENT_VERSION)),
                    html_url: release.html_url,
                    release_notes: release.body.unwrap_or_default(),
                    has_update,
                    assets: release.assets,
                };

                best = Some(pick_newer_release(best, info));
        }
    }
    best
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate { version: String },
    Available(ReleaseInfo),
    Updating { version: String, progress: String },
    Updated { version: String, message: String },
    Failed(String),
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

/// Name of the real executable inside the Linux release tarball. The
/// scythe-ui and scythe-daemon entries are symlinks to it.
const TARBALL_BINARY: &str = "scythe";

/// Release asset listing the SHA-256 of every other asset.
const SUMS_ASSET: &str = "SHA256SUMS";

/// Finds the SHA-256 published for `asset` in a `sha256sum`-format file.
fn expected_sha256(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == asset && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// SHA-256 of a file, computed by the platform's own tool (sha256sum on
/// Linux, certutil on Windows), the same way downloads use curl.
fn file_sha256(path: &std::path::Path) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    let output = {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("certutil");
        cmd.arg("-hashfile").arg(path).arg("SHA256");
        cmd.creation_flags(0x08000000);
        cmd.output()
    };
    #[cfg(not(target_os = "windows"))]
    let output = std::process::Command::new("sha256sum").arg(path).output();
    let output = output.map_err(|e| format!("Failed to hash {}: {}", path.display(), e))?;
    if !output.status.success() {
        return Err(format!("Failed to hash {}", path.display()));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|w| w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|w| w.to_ascii_lowercase())
        .ok_or_else(|| format!("No SHA-256 in hash output for {}", path.display()))
}

/// Checks a downloaded file against its published SHA-256.
fn verify_sha256(path: &std::path::Path, expected: &str) -> Result<(), String> {
    let actual = file_sha256(path)?;
    if actual != expected.to_ascii_lowercase() {
        return Err(format!(
            "Checksum mismatch for {}: expected {}, got {}. The download was not installed.",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("download"),
            expected,
            actual
        ));
    }
    Ok(())
}

/// Downloads `asset` to `dest` and refuses it unless it matches the SHA-256
/// in the release's SHA256SUMS. A release without SHA256SUMS is refused.
fn download_verified(info: &ReleaseInfo, asset: &ReleaseAsset, dest: &std::path::Path) -> Result<(), String> {
    let sums_asset = info
        .assets
        .iter()
        .find(|a| a.name == SUMS_ASSET)
        .ok_or_else(|| format!("Release v{} has no {}; refusing an unverifiable update", info.version, SUMS_ASSET))?;
    let sums_path = dest.with_extension("sha256sums");
    download_file(&sums_asset.browser_download_url, &sums_path)?;
    let sums = std::fs::read_to_string(&sums_path).map_err(|e| format!("Failed to read {}: {}", SUMS_ASSET, e))?;
    let _ = std::fs::remove_file(&sums_path);
    let expected = expected_sha256(&sums, &asset.name)
        .ok_or_else(|| format!("{} does not list {}; refusing the update", SUMS_ASSET, asset.name))?;
    download_file(&asset.browser_download_url, dest)?;
    if let Err(e) = verify_sha256(dest, &expected) {
        let _ = std::fs::remove_file(dest);
        return Err(e);
    }
    Ok(())
}

fn download_file(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let dest_str = dest.to_str().ok_or_else(|| "Invalid destination path".to_string())?;
    let mut cmd = std::process::Command::new("curl");
    cmd.args([
        "-L",
        "--fail",
        "--silent",
        "--show-error",
        "-H", "User-Agent: scythe-updater",
        "-o", dest_str,
        url,
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd.output().map_err(|e| format!("Failed to run curl: {}", e))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Download failed: {}", err.trim()));
    }
    if !dest.exists() || std::fs::metadata(dest).map(|m| m.len() == 0).unwrap_or(true) {
        return Err("Downloaded file is empty or missing".to_string());
    }
    Ok(())
}

pub fn spawn_auto_update(info: ReleaseInfo, status: Arc<Mutex<UpdateStatus>>) {
    if let Ok(mut lock) = status.lock() {
        *lock = UpdateStatus::Updating {
            version: info.version.clone(),
            progress: "Preparing update...".to_string(),
        };
    }

    std::thread::spawn(move || {
        let res = run_auto_update(&info, &status);
        if let Ok(mut lock) = status.lock() {
            match res {
                Ok(msg) => {
                    *lock = UpdateStatus::Updated {
                        version: info.version.clone(),
                        message: msg,
                    };
                }
                Err(err) => {
                    *lock = UpdateStatus::Failed(err);
                }
            }
        }
    });
}

fn run_auto_update(info: &ReleaseInfo, status: &Arc<Mutex<UpdateStatus>>) -> Result<String, String> {
    let set_progress = |msg: &str| {
        if let Ok(mut lock) = status.lock() {
            *lock = UpdateStatus::Updating {
                version: info.version.clone(),
                progress: msg.to_string(),
            };
        }
    };

    #[cfg(target_os = "linux")]
    {
        // 1. Check if running from AppImage
        if let Ok(appimage_path) = std::env::var("APPIMAGE") {
            let appimage_file = std::path::PathBuf::from(&appimage_path);
            if appimage_file.exists() {
                set_progress("Locating AppImage package...");
                let asset = info
                    .assets
                    .iter()
                    .find(|a| a.name.ends_with(".AppImage"))
                    .ok_or_else(|| "Could not find AppImage in release assets".to_string())?;

                set_progress(&format!("Downloading AppImage ({})...", asset.name));
                let temp_download = appimage_file.with_extension("download");
                download_verified(info, asset, &temp_download)?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&temp_download, std::fs::Permissions::from_mode(0o755));
                }

                set_progress("Applying update...");
                std::fs::rename(&temp_download, &appimage_file)
                    .map_err(|e| format!("Failed to replace AppImage: {}", e))?;

                return Ok(format!("Updated AppImage to v{}", info.version));
            }
        }

        // 2. Standard binary update (.tar.gz containing scythe-ui, scythe-daemon)
        set_progress("Locating Linux package...");
        let asset = info
            .assets
            .iter()
            .find(|a| a.name == "scythe-linux-x86_64.tar.gz")
            .or_else(|| info.assets.iter().find(|a| a.name.contains("linux") && a.name.ends_with(".tar.gz")))
            .ok_or_else(|| "Could not find scythe-linux-x86_64.tar.gz in release assets".to_string())?;

        let temp_dir = std::env::temp_dir().join(format!("scythe_update_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let archive_path = temp_dir.join("scythe-linux-x86_64.tar.gz");

        set_progress(&format!("Downloading {}...", asset.name));
        download_verified(info, asset, &archive_path)?;

        set_progress("Extracting binaries...");
        let extract_status = std::process::Command::new("tar")
            .args(["-xzf", archive_path.to_str().unwrap(), "-C", temp_dir.to_str().unwrap()])
            .output()
            .map_err(|e| format!("Failed to run tar: {}", e))?;

        if !extract_status.status.success() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(format!("tar extraction failed: {}", String::from_utf8_lossy(&extract_status.stderr)));
        }

        let extracted_dir = temp_dir.join("scythe-linux-x86_64");
        if !extracted_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err("Extracted directory not found".to_string());
        }

        // Resolve current executable's directory
        let current_exe = std::env::current_exe().map_err(|e| format!("Cannot locate current executable: {}", e))?;
        let canonical_exe = std::fs::canonicalize(&current_exe).unwrap_or_else(|_| current_exe.clone());
        let exe_dir = canonical_exe.parent().ok_or_else(|| "Invalid executable path".to_string())?;

        set_progress(&format!("Installing binaries into {}...", exe_dir.display()));

        // The tarball's scythe-ui and scythe-daemon are symlinks to the one
        // real binary. Replace the file the running executable resolves to,
        // so every name pointing at it picks up the new version.
        let src = extracted_dir.join(TARBALL_BINARY);
        if !src.is_file() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(format!("{} not found in the release archive", TARBALL_BINARY));
        }
        let dest = canonical_exe.clone();
        let dest_new = dest.with_extension("new");
        std::fs::copy(&src, &dest_new)
            .map_err(|e| format!("Failed to write {}: {}. Check write permissions in {}", dest_new.display(), e, exe_dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest_new, std::fs::Permissions::from_mode(0o755));
        }
        std::fs::rename(&dest_new, &dest)
            .map_err(|e| format!("Failed to atomically replace {}: {}", dest.display(), e))?;

        // Maintain aliases in ~/.local/bin if installed there
        if exe_dir.ends_with(".local/bin") {
            for link in ["scythe", "vrec", "vrec-ui"] {
                let link_path = exe_dir.join(link);
                let target_path = exe_dir.join("scythe-ui");
                if !link_path.exists() {
                    #[cfg(unix)]
                    let _ = std::os::unix::fs::symlink(&target_path, &link_path);
                }
            }
            let daemon_link = exe_dir.join("vrec-daemon");
            if !daemon_link.exists() {
                #[cfg(unix)]
                let _ = std::os::unix::fs::symlink(exe_dir.join("scythe-daemon"), &daemon_link);
            }
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
        Ok(format!("Installed v{} to {}", info.version, exe_dir.display()))
    }

    #[cfg(target_os = "windows")]
    {
        // 1. Check for official NSIS installer (scythe-setup.exe)
        if let Some(asset) = info.assets.iter().find(|a| a.name == "scythe-setup.exe") {
            let temp_dir = std::env::temp_dir();
            let installer_path = temp_dir.join(format!("scythe-setup-v{}.exe", info.version));

            set_progress(&format!("Downloading Windows installer ({})...", asset.name));
            download_verified(info, asset, &installer_path)?;

            set_progress("Launching installer...");
            let mut cmd = std::process::Command::new(&installer_path);
            cmd.arg("/S");
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
            let _ = cmd.spawn().map_err(|e| format!("Failed to launch installer: {}", e))?;

            return Ok(format!("Update installer launched for v{}.", info.version));
        }

        // 2. Portable zip update
        if let Some(asset) = info.assets.iter().find(|a| a.name == "scythe-windows-x86_64.zip") {
            let temp_dir = std::env::temp_dir().join(format!("scythe_update_{}", std::process::id()));
            let _ = std::fs::create_dir_all(&temp_dir);
            let zip_path = temp_dir.join("scythe-windows-x86_64.zip");

            set_progress(&format!("Downloading {}...", asset.name));
            download_verified(info, asset, &zip_path)?;

            set_progress("Extracting archive...");
            let tar_out = std::process::Command::new("tar.exe")
                .args(["-xf", zip_path.to_str().unwrap(), "-C", temp_dir.to_str().unwrap()])
                .output();

            let extracted_ok = tar_out.map(|o| o.status.success()).unwrap_or(false);
            if !extracted_ok {
                let ps_script = format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    zip_path.display(),
                    temp_dir.display()
                );
                let _ = std::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_script])
                    .output()
                    .map_err(|e| format!("Extraction failed: {}", e))?;
            }

            let current_exe = std::env::current_exe().map_err(|e| format!("Cannot locate current executable: {}", e))?;
            let exe_dir = current_exe.parent().ok_or_else(|| "Invalid executable path".to_string())?;

            set_progress("Replacing binaries and libraries...");
            let mut files_to_update = Vec::new();
            let mut search_dirs = vec![temp_dir.clone()];
            while let Some(dir) = search_dirs.pop() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            search_dirs.push(path);
                        } else if path.is_file() {
                            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let lower = name.to_lowercase();
                            if lower.ends_with(".exe") || lower.ends_with(".dll") {
                                files_to_update.push((path, name));
                            }
                        }
                    }
                }
            }

            for (src, name) in files_to_update {
                let dest = exe_dir.join(&name);
                let dest_old = exe_dir.join(format!("{}.old", name));
                let _ = std::fs::remove_file(&dest_old);
                let _ = std::fs::rename(&dest, &dest_old);
                let _ = std::fs::copy(&src, &dest);
            }

            let _ = std::fs::remove_dir_all(&temp_dir);
            return Ok(format!("Updated binaries to v{} in {}", info.version, exe_dir.display()));
        }

        Err("No compatible Windows package found in release assets".to_string())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Automatic update is not supported on this operating system".to_string())
    }
}

pub fn restart_application() {
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let _ = cmd.spawn();
    }
    std::process::exit(0);
}

pub fn open_browser_url(url: &str) {
    let u = url.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use windows::core::HSTRING;
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            unsafe {
                let _ = ShellExecuteW(
                    None,
                    windows::core::w!("open"),
                    &HSTRING::from(&u),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
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

    /// SHA256SUMS as written by `sha256sum` in the release workflow.
    const SUMS: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  scythe-linux-x86_64.tar.gz\n\
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  scythe-x86_64.AppImage\n";

    #[test]
    fn test_checksum_lookup_finds_the_asset_line() {
        assert_eq!(
            expected_sha256(SUMS, "scythe-linux-x86_64.tar.gz").as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(expected_sha256(SUMS, "scythe.exe"), None);
    }

    /// sha256sum marks binary-mode entries with "*name"; both forms count.
    #[test]
    fn test_checksum_lookup_accepts_binary_mode_marker() {
        let sums = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad *scythe.exe\n";
        assert!(expected_sha256(sums, "scythe.exe").is_some());
    }

    /// A downloaded file must be refused unless its SHA-256 matches the
    /// published sum. The updater used to install whatever curl fetched:
    /// a truncated or swapped asset replaced the user's binary.
    #[test]
    fn test_verify_accepts_matching_and_refuses_mismatched_files() {
        let dir = std::env::temp_dir().join(format!("scythe_sum_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good");
        std::fs::write(&good, b"abc").unwrap();
        assert!(verify_sha256(&good, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_ok());
        assert!(verify_sha256(&good, "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD").is_ok());
        let bad = dir.join("bad");
        std::fs::write(&bad, b"abd").unwrap();
        assert!(verify_sha256(&bad, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The release tarball holds the real binary as `scythe`; scythe-ui and
    /// scythe-daemon are symlinks to it. Installing the symlinks copied the
    /// old binary onto itself, so auto-update never updated anything.
    #[test]
    fn test_tarball_install_copies_the_real_binary() {
        assert_eq!(TARBALL_BINARY, "scythe");
    }

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

    fn release(version: &str, assets: usize) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: format!("v{version}"),
            version: version.to_string(),
            assets: (0..assets).map(|i| ReleaseAsset {
                name: format!("asset-{i}"),
                browser_download_url: String::new(),
                size: None,
            }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn test_newer_release_wins_over_older() {
        let picked = pick_newer_release(Some(release("0.2.18", 2)), release("0.2.21", 1));
        assert_eq!(picked.version, "0.2.21");
    }

    #[test]
    fn test_older_release_does_not_win() {
        let picked = pick_newer_release(Some(release("0.2.21", 7)), release("0.2.18", 2));
        assert_eq!(picked.version, "0.2.21");
    }

    #[test]
    fn test_equal_version_prefers_more_assets() {
        // A mirror that is missing assets must not hide them from the user.
        let picked = pick_newer_release(Some(release("0.2.21", 4)), release("0.2.21", 7));
        assert_eq!(picked.assets.len(), 7);
    }

    #[test]
    fn test_equal_version_keeps_first_when_assets_match() {
        let picked = pick_newer_release(Some(release("0.2.21", 7)), release("0.2.21", 7));
        assert_eq!(picked.tag_name, "v0.2.21");
        assert_eq!(picked.assets.len(), 7);
    }

    #[test]
    fn test_first_release_is_kept_when_nothing_to_compare() {
        let picked = pick_newer_release(None, release("0.2.21", 7));
        assert_eq!(picked.version, "0.2.21");
    }
}
