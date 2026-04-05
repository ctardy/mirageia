//! Automatic update of MirageIA via GitHub Releases.
//!
//! Strategy: background download + swap on next startup.
//!
//! 1. On proxy startup, a thread silently checks the latest version
//! 2. If a new version is available, the binary is downloaded to ~/.mirageia/staging/
//! 3. On next startup, the staged binary replaces the current executable
//! 4. `mirageia update` allows manual check/install

use std::fs;
use std::io::Write;
use std::path::PathBuf;

const GITHUB_REPO: &str = "ctardy/mirageia";

/// Current binary version (injected at compile time).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Staging directory for updates (~/.mirageia/staging/).
fn staging_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".mirageia").join("staging"))
}

/// Path to the staged binary ready to be swapped.
fn staged_binary_path() -> Option<PathBuf> {
    let bin_name = if cfg!(windows) {
        "mirageia.exe"
    } else {
        "mirageia"
    };
    staging_dir().map(|d| d.join(bin_name))
}

/// File containing the version of the staged binary.
fn staged_version_path() -> Option<PathBuf> {
    staging_dir().map(|d| d.join("version"))
}

/// Detects the current platform for the GitHub archive name.
fn platform_artifact() -> Option<&'static str> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some("mirageia-windows-x86_64.zip")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Some("mirageia-macos-aarch64.tar.gz")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some("mirageia-linux-x86_64.tar.gz")
    } else {
        None
    }
}

/// Information about the latest GitHub release.
#[derive(Debug)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub download_url: String,
}

/// Update check result.
#[derive(Debug)]
pub enum UpdateStatus {
    /// A new version is available.
    Available(ReleaseInfo),
    /// Already up to date.
    UpToDate,
    /// An update is already downloaded and ready (staging).
    Staged(String),
    /// Error during check.
    Error(String),
}

/// Checks the latest available version on GitHub Releases.
pub async fn check_latest_version() -> UpdateStatus {
    let artifact = match platform_artifact() {
        Some(a) => a,
        None => return UpdateStatus::Error("Platform not supported for automatic updates".into()),
    };

    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = match reqwest::Client::builder().user_agent("mirageia-updater").build() {
        Ok(c) => c,
        Err(e) => return UpdateStatus::Error(format!("HTTP client error: {}", e)),
    };

    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => return UpdateStatus::Error(format!("Cannot reach GitHub: {}", e)),
    };

    if !response.status().is_success() {
        return UpdateStatus::Error(format!("GitHub API returned {}", response.status()));
    }

    let json: serde_json::Value = match response.json().await {
        Ok(j) => j,
        Err(e) => return UpdateStatus::Error(format!("Invalid JSON response: {}", e)),
    };

    let tag = match json["tag_name"].as_str() {
        Some(t) => t.to_string(),
        None => return UpdateStatus::Error("Missing tag_name field in response".into()),
    };

    let remote_version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    let local_version = current_version();

    if remote_version == local_version {
        // Check if a binary is already staged
        if let Some(staged_ver) = read_staged_version() {
            if staged_ver != local_version {
                return UpdateStatus::Staged(staged_ver);
            }
        }
        return UpdateStatus::UpToDate;
    }

    // Compare versions (simplified semver)
    if !is_newer(&remote_version, local_version) {
        return UpdateStatus::UpToDate;
    }

    // Find the download URL for the artifact
    let download_url = json["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find_map(|a| {
                let name = a["name"].as_str()?;
                if name == artifact {
                    a["browser_download_url"].as_str().map(|u| u.to_string())
                } else {
                    None
                }
            })
        });

    match download_url {
        Some(url) => UpdateStatus::Available(ReleaseInfo {
            tag,
            version: remote_version,
            download_url: url,
        }),
        None => UpdateStatus::Error(format!("Artifact '{}' not found in release {}", artifact, tag)),
    }
}

/// Compares two semver versions (e.g., "0.2.0" > "0.1.0").
fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect()
    };
    let r = parse(remote);
    let l = parse(local);

    for i in 0..r.len().max(l.len()) {
        let rv = r.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if rv > lv {
            return true;
        }
        if rv < lv {
            return false;
        }
    }
    false
}

/// Reads the version of the currently staged binary.
fn read_staged_version() -> Option<String> {
    staged_version_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Downloads the new version to the staging directory.
/// Does not replace the current binary — the swap happens on next startup.
pub async fn download_to_staging(release: &ReleaseInfo) -> Result<(), String> {
    let staging = staging_dir().ok_or("Cannot determine staging directory")?;
    fs::create_dir_all(&staging).map_err(|e| format!("Error creating staging dir: {}", e))?;

    let client = reqwest::Client::builder()
        .user_agent("mirageia-updater")
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(&release.download_url)
        .send()
        .await
        .map_err(|e| format!("Download error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Error reading response: {}", e))?;

    // Extract binary from archive
    let binary_data = extract_binary_from_archive(&bytes)?;

    // Write binary to staging
    let bin_path = staged_binary_path()
        .ok_or("Cannot determine staged binary path")?;

    let mut file = fs::File::create(&bin_path)
        .map_err(|e| format!("Error writing binary: {}", e))?;
    file.write_all(&binary_data)
        .map_err(|e| format!("Error writing binary: {}", e))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Error chmod: {}", e))?;
    }

    // Write staged version
    let ver_path = staged_version_path()
        .ok_or("Cannot determine version file path")?;
    fs::write(&ver_path, &release.version)
        .map_err(|e| format!("Error writing version: {}", e))?;

    Ok(())
}

/// Extracts the mirageia binary from a ZIP or TAR.GZ archive.
fn extract_binary_from_archive(data: &[u8]) -> Result<Vec<u8>, String> {
    let bin_name = if cfg!(windows) {
        "mirageia.exe"
    } else {
        "mirageia"
    };

    // Try ZIP first (Windows)
    if data.len() >= 4 && data[0..4] == [0x50, 0x4B, 0x03, 0x04] {
        return extract_from_zip(data, bin_name);
    }

    // Otherwise TAR.GZ (Unix)
    extract_from_targz(data, bin_name)
}

/// Extracts a file from a ZIP archive in memory.
fn extract_from_zip(data: &[u8], target_name: &str) -> Result<Vec<u8>, String> {
    use std::io::{Cursor, Read};

    let reader = Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("Error opening ZIP: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Error reading ZIP: {}", e))?;
        let name = file.name().to_string();
        if name == target_name || name.ends_with(&format!("/{}", target_name)) {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| format!("Error extracting ZIP: {}", e))?;
            return Ok(buf);
        }
    }

    Err(format!("'{}' not found in ZIP archive", target_name))
}

/// Extracts a file from a TAR.GZ archive in memory.
fn extract_from_targz(data: &[u8], target_name: &str) -> Result<Vec<u8>, String> {
    use std::io::{Cursor, Read};

    let gz = flate2::read::GzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| format!("Error reading TAR: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Error in TAR entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Error in TAR path: {}", e))?
            .to_string_lossy()
            .to_string();

        if path == target_name || path.ends_with(&format!("/{}", target_name)) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("Error extracting TAR: {}", e))?;
            return Ok(buf);
        }
    }

    Err(format!(
        "'{}' not found in TAR.GZ archive",
        target_name
    ))
}

/// Applies a staged update on startup.
///
/// If a binary is present in staging/ with a different version from the current one:
/// 1. Renames the current executable to `.old`
/// 2. Copies the staged binary to the current location
/// 3. Removes `.old` and cleans up staging/
///
/// Returns `Some(version)` if the update was applied, `None` otherwise.
pub fn apply_staged_update() -> Option<String> {
    let staged_version = read_staged_version()?;

    if staged_version == current_version() {
        // Same version, clean up staging
        cleanup_staging();
        return None;
    }

    let staged_bin = staged_binary_path()?;
    if !staged_bin.exists() {
        return None;
    }

    let current_exe = std::env::current_exe().ok()?;

    // On Windows, we can't delete a running exe but we can rename it
    let old_exe = current_exe.with_extension("old");

    // Step 1: rename current binary -> .old
    // Ignore error if .old already exists (leftover from previous update)
    let _ = fs::remove_file(&old_exe);
    if fs::rename(&current_exe, &old_exe).is_err() {
        tracing::warn!("Cannot rename current binary for update");
        return None;
    }

    // Step 2: copy staged binary -> current location
    if fs::copy(&staged_bin, &current_exe).is_err() {
        // Rollback: restore old binary
        let _ = fs::rename(&old_exe, &current_exe);
        tracing::warn!("Cannot copy new binary, rollback performed");
        return None;
    }

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755));
    }

    // Step 3: cleanup
    let _ = fs::remove_file(&old_exe);
    cleanup_staging();

    Some(staged_version)
}

/// Cleans up the staging directory.
fn cleanup_staging() {
    if let Some(dir) = staging_dir() {
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Launches background version check and download.
/// Does not block the main thread — used on proxy startup.
pub fn spawn_background_check() {
    std::thread::spawn(|| {
        // Short delay to avoid impacting startup
        std::thread::sleep(std::time::Duration::from_secs(5));

        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };

        rt.block_on(async {
            match check_latest_version().await {
                UpdateStatus::Available(release) => {
                    tracing::info!(
                        "New version available: v{} (current: v{})",
                        release.version,
                        current_version()
                    );
                    match download_to_staging(&release).await {
                        Ok(()) => {
                            tracing::info!(
                                "v{} downloaded — will be applied on next startup",
                                release.version
                            );
                        }
                        Err(e) => {
                            tracing::debug!("Update download failed: {}", e);
                        }
                    }
                }
                UpdateStatus::Staged(ver) => {
                    tracing::info!(
                        "v{} ready — will be applied on next startup",
                        ver
                    );
                }
                UpdateStatus::UpToDate => {
                    tracing::debug!("MirageIA is up to date (v{})", current_version());
                }
                UpdateStatus::Error(e) => {
                    tracing::debug!("Update check: {}", e);
                }
            }
        });
    });
}

/// Executes the `mirageia update` command (interactive mode).
/// With `--check`: only displays if an update is available.
/// Without flag: checks, downloads, and applies immediately.
pub async fn run_update(check_only: bool) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("  MirageIA v{}", current_version());
    eprintln!("  Checking for updates...");
    eprintln!();

    match check_latest_version().await {
        UpdateStatus::Available(release) => {
            eprintln!(
                "  ✦ New version available: v{} (current: v{})",
                release.version,
                current_version()
            );

            if check_only {
                eprintln!();
                eprintln!("  To update: mirageia update");
                return Ok(());
            }

            eprintln!("  Downloading...");
            download_to_staging(&release).await.map_err(|e| {
                eprintln!("  ✗ {}", e);
                e
            })?;

            eprintln!("  Applying update...");
            match apply_staged_update() {
                Some(ver) => {
                    eprintln!("  ✓ MirageIA updated to v{}", ver);
                    eprintln!();
                    eprintln!("  Restart MirageIA to use the new version.");
                }
                None => {
                    eprintln!("  ✗ Cannot apply update automatically.");
                    eprintln!("    Binary is ready in ~/.mirageia/staging/");
                    eprintln!("    It will be applied on next startup.");
                }
            }
        }
        UpdateStatus::Staged(ver) => {
            eprintln!(
                "  ✦ v{} is already downloaded and ready.",
                ver
            );

            if !check_only {
                eprintln!("  Applying...");
                match apply_staged_update() {
                    Some(v) => {
                        eprintln!("  ✓ MirageIA updated to v{}", v);
                        eprintln!();
                        eprintln!("  Restart MirageIA to use the new version.");
                    }
                    None => {
                        eprintln!("  ✗ Cannot apply update.");
                    }
                }
            } else {
                eprintln!();
                eprintln!("  To apply: mirageia update");
            }
        }
        UpdateStatus::UpToDate => {
            eprintln!("  ✓ MirageIA is up to date (v{})", current_version());
        }
        UpdateStatus::Error(e) => {
            eprintln!("  ✗ Error: {}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn test_is_newer_different_lengths() {
        assert!(is_newer("0.2.0", "0.1"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1", "0.1.0"));
    }

    #[test]
    fn test_current_version_not_empty() {
        assert!(!current_version().is_empty());
    }

    #[test]
    fn test_platform_artifact_not_none() {
        // On any CI platform, we should have an artifact
        let artifact = platform_artifact();
        // Can't guarantee Some on all test platforms
        // but we verify the function doesn't panic
        if let Some(name) = artifact {
            assert!(name.starts_with("mirageia-"));
            assert!(name.contains("x86_64") || name.contains("aarch64"));
        }
    }

    #[test]
    fn test_staging_dir_is_under_mirageia() {
        if let Some(dir) = staging_dir() {
            assert!(dir.to_string_lossy().contains(".mirageia"));
            assert!(dir.to_string_lossy().contains("staging"));
        }
    }
}
