use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::DvdError;

/// Create or clean `out_dir/VIDEO_TS` and return its path.
pub(crate) fn prepare_video_ts(out_dir: &Path) -> Result<PathBuf, DvdError> {
    let video_ts = out_dir.join("VIDEO_TS");

    match fs::metadata(&video_ts) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(DvdError::Invalid(format!(
                    "{} is not a directory",
                    video_ts.display()
                )));
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(&video_ts).map_err(|err| crate::io_error(&video_ts, err))?;
        }
        Err(err) => return Err(crate::io_error(&video_ts, err)),
    }

    clean_video_ts(&video_ts)?;
    Ok(video_ts)
}

/// Write one file and return its byte length.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<u64, DvdError> {
    fs::write(path, bytes).map_err(|err| crate::io_error(path, err))?;
    u64::try_from(bytes.len()).map_err(|_| DvdError::Invalid("file too large".to_string()))
}

/// Return a file size in bytes.
pub(crate) fn file_size(path: &Path) -> Result<u64, DvdError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|err| crate::io_error(path, err))
}

fn clean_video_ts(dir: &Path) -> Result<(), DvdError> {
    let entries = fs::read_dir(dir).map_err(|err| crate::io_error(dir, err))?;

    for entry in entries {
        let entry = entry.map_err(|err| crate::io_error(dir, err))?;
        let file_type = entry.file_type().map_err(|err| crate::io_error(dir, err))?;
        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_cleanup_target(&name) {
            continue;
        }

        let path = entry.path();
        fs::remove_file(&path).map_err(|err| crate::io_error(&path, err))?;
    }

    Ok(())
}

fn is_cleanup_target(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();

    if upper == "VIDEO_TS.IFO" || upper == "VIDEO_TS.BUP" || upper == "VIDEO_TS.VOB" {
        return true;
    }

    let Some(rest) = upper.strip_prefix("VTS_") else {
        return false;
    };
    let Some((nums, ext)) = rest.split_once('.') else {
        return false;
    };
    if ext != "IFO" && ext != "BUP" && ext != "VOB" {
        return false;
    }

    let Some((vts, file)) = nums.split_once('_') else {
        return false;
    };

    !vts.is_empty()
        && vts.bytes().all(|b| b.is_ascii_digit())
        && !file.is_empty()
        && file.bytes().all(|b| b.is_ascii_digit())
}
