use crate::model::{REPOSITORY, VERSION};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const MAX_DOWNLOAD: u64 = 100 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub url: String,
    pub size: u64,
    pub digest: Option<String>,
    pub checksum_url: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    Current,
    Available(Release),
    Downloading {
        received: u64,
        total: u64,
    },
    Ready {
        staged: PathBuf,
        hash: String,
    },
    Error(String),
}
#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

pub fn asset_name(version: &Version) -> String {
    format!("BandwidthLimiter-v{version}.exe")
}

pub fn parse_release(bytes: &[u8]) -> Result<Option<Release>, String> {
    let release: ApiRelease =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid release response: {e}"))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|e| e.to_string())?;
    if !version.pre.is_empty() || version <= Version::parse(VERSION).unwrap() {
        return Ok(None);
    }
    let prefix = format!(
        "https://github.com/{REPOSITORY}/releases/download/{}/",
        release.tag_name
    );
    let name = asset_name(&version);
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .ok_or("A newer release has no compatible standalone Windows asset")?;
    if !asset.browser_download_url.starts_with(&prefix)
        || !asset.browser_download_url.ends_with(&format!("/{name}"))
        || asset.size == 0
        || asset.size > MAX_DOWNLOAD
    {
        return Err("Release asset URL or size is invalid".into());
    }
    let checksum_url = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS" && a.browser_download_url == format!("{prefix}SHA256SUMS"))
        .map(|a| a.browser_download_url.clone());
    let digest = asset
        .digest
        .as_ref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|s| valid_hash(s))
        .map(str::to_owned);
    if digest.is_none() && checksum_url.is_none() {
        return Err("Release has no SHA-256 digest or SHA256SUMS; installation is refused".into());
    }
    Ok(Some(Release {
        version,
        url: asset.browser_download_url.clone(),
        size: asset.size,
        digest,
        checksum_url,
    }))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .into()
}
fn get_bytes(url: &str, maximum: u64) -> Result<Vec<u8>, String> {
    let mut response = agent().get(url).header("User-Agent", "BandwidthLimiter/0.1.0").call().map_err(|e| format!("Update request failed: {e}. Repository/release may be unpublished, or GitHub may be unavailable or rate-limited."))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > maximum {
        return Err("Update response exceeds size bound".into());
    }
    Ok(bytes)
}

pub fn check() -> Result<Option<Release>, String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("Updates are available only for the Windows x64 distribution".into());
    }
    parse_release(&get_bytes(
        &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
        1024 * 1024,
    )?)
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn checksum(text: &str, name: &str) -> Result<String, String> {
    let mut hashes = text.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let digest = fields.next()?;
        let file = fields.next()?.trim_start_matches('*');
        (valid_hash(digest) && file == name && fields.next().is_none())
            .then(|| digest.to_lowercase())
    });
    let hash = hashes.next().ok_or("Asset checksum was not found")?;
    if hashes.next().is_some() {
        return Err("Ambiguous duplicate asset checksum".into());
    }
    Ok(hash)
}

pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn validate_pe(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 64 || &bytes[..2] != b"MZ" {
        return Err("Downloaded file is not a Windows executable".into());
    }
    let offset = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
    let header = bytes
        .get(offset..offset.saturating_add(6))
        .ok_or("Truncated PE header")?;
    if &header[..4] != b"PE\0\0" || header[4..6] != [0x64, 0x86] {
        return Err("Update must be a Windows x64 PE executable".into());
    }
    Ok(())
}

pub fn download(
    release: &Release,
    directory: &Path,
    state: &Arc<Mutex<State>>,
) -> Result<(PathBuf, String), String> {
    let expected = match &release.digest {
        Some(digest) => digest.to_lowercase(),
        None => checksum(
            &String::from_utf8(get_bytes(
                release
                    .checksum_url
                    .as_deref()
                    .ok_or("Missing checksum URL")?,
                65536,
            )?)
            .map_err(|e| e.to_string())?,
            &asset_name(&release.version),
        )?,
    };
    let updates = directory.join("updates");
    fs::create_dir_all(&updates).map_err(|e| e.to_string())?;
    let partial = updates.join("update.partial");
    let staged = updates.join(asset_name(&release.version));
    let result = (|| {
        let mut response = agent()
            .get(&release.url)
            .header("User-Agent", "BandwidthLimiter/0.1.0")
            .call()
            .map_err(|e| e.to_string())?;
        let mut reader = response.body_mut().as_reader();
        let mut file = File::create(&partial).map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 65536];
        let mut total = 0;
        loop {
            let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > release.size || total > MAX_DOWNLOAD {
                return Err("Download exceeds declared size".into());
            }
            file.write_all(&buffer[..count])
                .map_err(|e| e.to_string())?;
            *state.lock().unwrap() = State::Downloading {
                received: total,
                total: release.size,
            };
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        if total != release.size {
            return Err("Incomplete update download; installed executable was preserved".into());
        }
        if hash_file(&partial)? != expected {
            return Err("SHA-256 verification failed; installed executable was preserved".into());
        }
        let mut header = vec![0; (total as usize).min(1024 * 1024)];
        File::open(&partial)
            .map_err(|e| e.to_string())?
            .read_exact(&mut header)
            .map_err(|e| e.to_string())?;
        validate_pe(&header)?;
        crate::storage::replace(&partial, &staged)?;
        Ok((staged, expected))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Prepare a separate copy of this executable as the updater. Never uses shell scripts.
pub fn launch_helper(staged: &Path, hash: &str, directory: &Path) -> Result<(), String> {
    if hash_file(staged)? != hash {
        return Err("Staged update changed after download".into());
    }
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let ready = staged.with_extension("helper-ready");
    if ready.exists() {
        fs::remove_file(&ready).map_err(|e| e.to_string())?;
    }
    let helper = directory
        .join("updates")
        .join(format!("updater-{}.exe", std::process::id()));
    fs::copy(&target, &helper).map_err(|e| format!("Cannot prepare update helper: {e}"))?;
    let mut child = std::process::Command::new(helper)
        .arg("--apply-update")
        .arg(std::process::id().to_string())
        .arg(staged)
        .arg(target)
        .arg(hash)
        .spawn()
        .map_err(|e| format!("Cannot start update helper: {e}"))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if ready.is_file() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "Update helper exited ({status}); see updates/last-update-error.txt"
            ));
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Update helper did not become ready; application remains running".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Only invoked by the user-initiated updater after download, never during startup checks.
#[cfg(windows)]
pub fn apply_update(pid: u32, staged: &Path, target: &Path, expected: &str) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    if !valid_hash(expected) || hash_file(staged)? != expected {
        return Err("Staged update verification failed".into());
    }
    if !target.is_absolute() || !target.is_file() || target == staged {
        return Err("Invalid executable replacement target".into());
    }
    // SAFETY: wait-only handle; no process mutation. A failed open/wait never proceeds to replacement.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return Err("Cannot wait for the original application; retry the update".into());
        }
        if let Err(error) = fs::write(staged.with_extension("helper-ready"), b"ready") {
            CloseHandle(process);
            return Err(format!("Cannot acknowledge update preparation: {error}"));
        }
        let result = WaitForSingleObject(process, 60000);
        CloseHandle(process);
        if result != WAIT_OBJECT_0 {
            return Err(
                "Application did not exit within 60 seconds; update was not installed".into(),
            );
        }
    }
    let candidate = target.with_extension("update-new");
    let backup = target.with_extension("previous.exe");
    fs::copy(target, &backup)
        .map_err(|e| format!("Cannot preserve the previous executable: {e}"))?;
    fs::copy(staged, &candidate).map_err(|e| e.to_string())?;
    if hash_file(&candidate)? != expected {
        return Err("Copied update checksum mismatch".into());
    }
    crate::storage::replace(&candidate, target)?;
    std::process::Command::new(target)
        .spawn()
        .map_err(|e| format!("Update installed; restart manually: {e}"))?;
    Ok(())
}
#[cfg(not(windows))]
pub fn apply_update(_: u32, _: &Path, _: &Path, _: &str) -> Result<(), String> {
    Err("Windows-only update helper".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str) -> serde_json::Value {
        serde_json::json!({"tag_name": format!("v{version}"), "draft": false, "prerelease": false,
            "assets": [{"name": format!("BandwidthLimiter-v{version}.exe"), "size": 100,
            "browser_download_url": format!("https://github.com/{REPOSITORY}/releases/download/v{version}/BandwidthLimiter-v{version}.exe"),
            "digest": format!("sha256:{}", "a".repeat(64))}]})
    }
    #[test]
    fn stable_semantic_versions_and_canonical_assets() {
        let mut fixture = release("0.10.0");
        assert_eq!(
            parse_release(&serde_json::to_vec(&fixture).unwrap())
                .unwrap()
                .unwrap()
                .version,
            Version::parse("0.10.0").unwrap()
        );
        fixture["prerelease"] = true.into();
        assert!(
            parse_release(&serde_json::to_vec(&fixture).unwrap())
                .unwrap()
                .is_none()
        );
        fixture["prerelease"] = false.into();
        fixture["assets"][0]["browser_download_url"] = "https://example.com/malicious.exe".into();
        assert!(parse_release(&serde_json::to_vec(&fixture).unwrap()).is_err());
        assert!(
            parse_release(&serde_json::to_vec(&release("0.0.9")).unwrap())
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn checksum_requires_exact_name_and_unambiguous_hash() {
        let line = format!("{}  BandwidthLimiter-v0.2.0.exe", "b".repeat(64));
        assert_eq!(
            checksum(&line, "BandwidthLimiter-v0.2.0.exe").unwrap(),
            "b".repeat(64)
        );
        assert!(checksum(&line, "Other.exe").is_err());
        assert!(checksum(&format!("{line}\n{line}"), "BandwidthLimiter-v0.2.0.exe").is_err());
        assert!(validate_pe(b"<html>error</html>").is_err());
        let mut pe = vec![0u8; 128];
        pe[..2].copy_from_slice(b"MZ");
        pe[60] = 64;
        pe[64..70].copy_from_slice(b"PE\0\0\x64\x86");
        assert!(validate_pe(&pe).is_ok());
        pe[68] = 0x4c;
        assert!(validate_pe(&pe).is_err());
    }
}
