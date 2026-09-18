use std::sync::{Arc, Mutex};
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
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
struct GitHubRelease {
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

pub fn check_for_updates() -> Option<ReleaseInfo> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);

    // Run curl with a 6-second timeout to avoid blocking
    let mut cmd = std::process::Command::new("curl");
    cmd.args([
        "-s",
        "-H", "User-Agent: scythe-updater",
        "-H", "Accept: application/vnd.github.v3+json",
        "--max-time", "6",
        &url,
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd.output().ok()?;

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
        assets: release.assets,
    })
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
                download_file(&asset.browser_download_url, &temp_download)?;

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
        download_file(&asset.browser_download_url, &archive_path)?;

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

        for bin_name in ["scythe-ui", "scythe-daemon"] {
            let src = extracted_dir.join(bin_name);
            if src.exists() {
                let dest = exe_dir.join(bin_name);
                let dest_new = exe_dir.join(format!("{}.new", bin_name));

                std::fs::copy(&src, &dest_new)
                    .map_err(|e| format!("Failed to write {}: {}. Check write permissions in {}", bin_name, e, exe_dir.display()))?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest_new, std::fs::Permissions::from_mode(0o755));
                }

                std::fs::rename(&dest_new, &dest)
                    .map_err(|e| format!("Failed to atomically replace {}: {}", bin_name, e))?;
            }
        }

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
            download_file(&asset.browser_download_url, &installer_path)?;

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
            download_file(&asset.browser_download_url, &zip_path)?;

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

            set_progress("Replacing binaries...");
            for bin_name in ["scythe-ui.exe", "scythe-daemon.exe"] {
                let src = temp_dir.join(bin_name);
                if src.exists() {
                    let dest = exe_dir.join(bin_name);
                    let dest_old = exe_dir.join(format!("{}.old", bin_name));
                    let _ = std::fs::remove_file(&dest_old);
                    let _ = std::fs::rename(&dest, &dest_old);
                    let _ = std::fs::copy(&src, &dest);
                }
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
