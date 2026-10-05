use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};

pub fn ensure_directory(path: &Path) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => bail!("不是文件夹: {}", path.display()),
        Err(_) => bail!("路径不存在: {}", path.display()),
    }
}

/// `C:\`、`D:\`、`/` 这类卷根目录。
pub fn is_volume_root(path: &Path) -> bool {
    path.parent().is_none()
}

/// 同一块磁盘上的路径返回相同的 key，用来决定哪些任务可以并行。
pub fn volume_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        match path.components().next() {
            Some(Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().to_lowercase(),
            _ => String::new(),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path)
            .map(|metadata| metadata.dev().to_string())
            .unwrap_or_default()
    }
}

/// 转成绝对路径、去重，并去掉被其它根目录包含的根目录（避免同一个目录被处理两次）。
pub fn normalize_roots(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut roots: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for path in paths {
        let path = std::path::absolute(&path).unwrap_or(path);
        let key = path_key(&path);
        if roots.iter().any(|(_, existing)| key.starts_with(existing)) {
            continue;
        }
        roots.retain(|(_, existing)| !existing.starts_with(&key));
        roots.push((path, key));
    }
    roots.into_iter().map(|(path, _)| path).collect()
}

fn path_key(path: &Path) -> Vec<String> {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| {
            let part = component.as_os_str().to_string_lossy();
            if cfg!(windows) {
                part.to_lowercase()
            } else {
                part.into_owned()
            }
        })
        .collect()
}

pub fn is_dir_empty(path: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(path)?.next().is_none())
}

pub fn remove_file_allow_readonly(path: &Path) -> io::Result<()> {
    retry_readonly(path, |p| fs::remove_file(p))
}

pub fn remove_dir_allow_readonly(path: &Path) -> io::Result<()> {
    retry_readonly(path, |p| fs::remove_dir(p))
}

fn retry_readonly(path: &Path, op: fn(&Path) -> io::Result<()>) -> io::Result<()> {
    match op(path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied && clear_readonly(path)? => {
            op(path)
        }
        other => other,
    }
}

/// 清掉只读属性。返回 true 表示确实改过属性，值得重试。
fn clear_readonly(path: &Path) -> io::Result<bool> {
    if !cfg!(windows) {
        return Ok(false);
    }
    let mut permissions = fs::symlink_metadata(path)?.permissions();
    if !permissions.readonly() {
        return Ok(false);
    }
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)?;
    Ok(true)
}

/// 目标目录里已有同名文件时，生成 `name (1).ext` 这样的新名字。
pub fn unique_destination(target_dir: &Path, file_name: &OsStr) -> PathBuf {
    let base = target_dir.join(file_name);
    if fs::symlink_metadata(&base).is_err() {
        return base;
    }

    let file_path = Path::new(file_name);
    let stem = file_path
        .file_stem()
        .map(OsString::from)
        .unwrap_or_else(|| file_name.to_os_string());
    let extension = file_path.extension();

    (1..)
        .map(|index| {
            let mut name = stem.clone();
            name.push(format!(" ({index})"));
            if let Some(extension) = extension {
                name.push(".");
                name.push(extension);
            }
            target_dir.join(name)
        })
        .find(|candidate| fs::symlink_metadata(candidate).is_err())
        .expect("infinite iterator always yields a destination")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn unique_destination_renames_on_conflict() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("deep.txt"), "existing").unwrap();
        fs::write(temp.path().join("deep (1).txt"), "existing").unwrap();

        let destination = unique_destination(temp.path(), OsStr::new("deep.txt"));
        assert_eq!(destination, temp.path().join("deep (2).txt"));
    }

    #[test]
    fn normalize_roots_drops_nested_and_duplicate_roots() {
        let temp = tempdir().unwrap();
        let a = temp.path().join("a");
        let nested = a.join("b");
        let other = temp.path().join("ab");

        let roots = normalize_roots([nested.clone(), a.clone(), other.clone(), a.clone()]);
        assert_eq!(roots, vec![a, other]);
    }

    #[test]
    fn volume_root_detection() {
        if cfg!(windows) {
            assert!(is_volume_root(Path::new(r"C:\")));
            assert!(!is_volume_root(Path::new(r"C:\Users")));
            assert_eq!(
                volume_key(Path::new(r"D:\a")),
                volume_key(Path::new(r"d:\b"))
            );
            assert_ne!(
                volume_key(Path::new(r"C:\a")),
                volume_key(Path::new(r"D:\a"))
            );
        } else {
            assert!(is_volume_root(Path::new("/")));
        }
    }
}
