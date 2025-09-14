use crate::model::{Config, Usage};
use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn data_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("BandwidthLimiter")
}

pub fn lock(directory: &Path) -> Result<File, String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("instance.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock_exclusive()
        .map_err(|_| "Another BandwidthLimiter instance owns this data directory".to_string())?;
    Ok(file)
}

pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 8 * 1024 * 1024 {
                return Err("Settings file exceeds 8 MiB".into());
            }
            serde_json::from_slice(&bytes)
                .map_err(|e| format!("{}: {e}; file was preserved", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn load(directory: &Path) -> Result<(Config, Usage), String> {
    let config: Config = read_json(&directory.join("settings.json"))?;
    config.validate()?;
    let usage = read_json(&directory.join("usage.json"))?;
    Ok((config, usage))
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    replace(&temporary, path)
}

#[cfg(windows)]
pub fn replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both strings are NUL-terminated and alive for this call.
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
pub fn replace(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_directory() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".test-output");
        fs::create_dir_all(&root).unwrap();
        tempfile::tempdir_in(root).unwrap()
    }
    #[test]
    fn atomic_roundtrip_and_corruption_preservation() {
        let directory = fixture_directory();
        let path = directory.path().join("settings.json");
        write_json(&path, &Config::default()).unwrap();
        let mut config: Config = read_json(&path).unwrap();
        config.startup_update_check = false;
        write_json(&path, &config).unwrap();
        assert!(!read_json::<Config>(&path).unwrap().startup_update_check);
        fs::write(&path, b"broken").unwrap();
        assert!(read_json::<Config>(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
    }
    #[test]
    fn exclusive_instance_lock() {
        let directory = fixture_directory();
        let guard = lock(directory.path()).unwrap();
        assert!(lock(directory.path()).is_err());
        drop(guard);
        assert!(lock(directory.path()).is_ok());
    }
}
