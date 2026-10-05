use std::ffi::OsStr;

/// 默认当作“噪音”的文件：只有这些文件的目录仍然算空目录。
pub const DEFAULT_IGNORED_FILES: &[&str] = &["desktop.ini", "Thumbs.db", ".DS_Store"];

/// 任何层级都不进入的目录。它们本身算作“有内容”，所以父目录也不会被当成空目录。
pub const DEFAULT_SKIPPED_DIRS: &[&str] = &[
    "$Recycle.Bin",
    "System Volume Information",
    "AppData",
    ".git",
    ".svn",
    ".hg",
    "node_modules",
];

/// 只在磁盘根目录下保护的系统目录。直接扫描整块磁盘时，这些目录里的空目录往往是系统或程序需要的。
pub const VOLUME_ROOT_PROTECTED_DIRS: &[&str] = &[
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "Recovery",
    "Boot",
    "EFI",
    "PerfLogs",
    "Config.Msi",
    "$WinREAgent",
    "$SysReset",
    "$Windows.~BT",
    "$Windows.~WS",
    "$GetCurrent",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    ignored_files: Vec<String>,
    skipped_dirs: Vec<String>,
    /// 隐藏文件是否当作噪音：只含隐藏文件的目录也算空目录，删除时连同隐藏文件一起删掉。
    pub hidden_files_are_noise: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self::new(true, std::iter::empty::<&str>(), std::iter::empty::<&str>())
    }
}

impl ScanOptions {
    pub fn new<I, J, S, T>(default_ignored: bool, extra_ignored: I, extra_skipped: J) -> Self
    where
        I: IntoIterator<Item = S>,
        J: IntoIterator<Item = T>,
        S: AsRef<str>,
        T: AsRef<str>,
    {
        let mut options = Self {
            ignored_files: Vec::new(),
            skipped_dirs: DEFAULT_SKIPPED_DIRS
                .iter()
                .map(|name| name.to_string())
                .collect(),
            hidden_files_are_noise: true,
        };
        if default_ignored {
            options.add_ignored(DEFAULT_IGNORED_FILES);
        }
        options.add_ignored(extra_ignored);
        for name in extra_skipped {
            push_unique(&mut options.skipped_dirs, name.as_ref());
        }
        options
    }

    pub fn add_ignored<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            push_unique(&mut self.ignored_files, name.as_ref());
        }
    }

    pub fn is_ignored_file(&self, name: &OsStr) -> bool {
        matches_any(&self.ignored_files, name)
    }

    pub fn is_skipped_dir(&self, name: &OsStr, parent_is_volume_root: bool) -> bool {
        matches_any(&self.skipped_dirs, name)
            || (parent_is_volume_root
                && name.to_str().is_some_and(|name| {
                    VOLUME_ROOT_PROTECTED_DIRS
                        .iter()
                        .any(|protected| protected.eq_ignore_ascii_case(name))
                }))
    }

    pub fn ignored_files(&self) -> &[String] {
        &self.ignored_files
    }

    pub fn skipped_dirs(&self) -> &[String] {
        &self.skipped_dirs
    }
}

fn push_unique(list: &mut Vec<String>, name: &str) {
    let name = name.trim();
    if !name.is_empty() && !list.iter().any(|item| item.eq_ignore_ascii_case(name)) {
        list.push(name.to_string());
    }
}

fn matches_any(list: &[String], name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| list.iter().any(|item| item.eq_ignore_ascii_case(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_is_case_insensitive() {
        let options = ScanOptions::new(true, ["foo.tmp"], ["cache"]);
        assert!(options.is_ignored_file(OsStr::new("DESKTOP.INI")));
        assert!(options.is_ignored_file(OsStr::new("Foo.TMP")));
        assert!(options.is_skipped_dir(OsStr::new(".GIT"), false));
        assert!(options.is_skipped_dir(OsStr::new("Cache"), false));
        assert!(!options.is_skipped_dir(OsStr::new("Windows"), false));
        assert!(options.is_skipped_dir(OsStr::new("windows"), true));
    }

    #[test]
    fn default_ignored_can_be_disabled() {
        let options = ScanOptions::new(false, ["a.txt"], std::iter::empty::<&str>());
        assert!(!options.is_ignored_file(OsStr::new("desktop.ini")));
        assert!(options.is_ignored_file(OsStr::new("a.txt")));
    }
}
