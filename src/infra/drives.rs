//! 枚举本机磁盘，供界面做多选。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Cdrom,
    Ramdisk,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub path: String,
    pub label: String,
    pub file_system: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[cfg(windows)]
pub fn list() -> Vec<DriveInfo> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn from_wide(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    let mut drives = Vec::new();
    // SAFETY: 以下都是只读的 Win32 查询，传入的缓冲区长度与声明一致。
    unsafe {
        // 读卡器、光驱没有介质时不要弹“请插入磁盘”对话框。
        let previous = SetErrorMode(SEM_FAILCRITICALERRORS);
        let mask = GetLogicalDrives();
        for index in 0..26u32 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let path = format!("{}:\\", (b'A' + index as u8) as char);
            let root = wide(&path);

            let kind = match GetDriveTypeW(root.as_ptr()) {
                2 => DriveKind::Removable,
                3 => DriveKind::Fixed,
                4 => DriveKind::Network,
                5 => DriveKind::Cdrom,
                6 => DriveKind::Ramdisk,
                _ => DriveKind::Unknown,
            };

            let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
            if GetDiskFreeSpaceExW(root.as_ptr(), &mut free, &mut total, &mut total_free) == 0 {
                // 没有介质或无法访问，界面上不显示。
                continue;
            }

            let mut label = [0u16; 261];
            let mut fs_name = [0u16; 261];
            let ok = GetVolumeInformationW(
                root.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs_name.as_mut_ptr(),
                fs_name.len() as u32,
            ) != 0;

            drives.push(DriveInfo {
                path,
                label: if ok { from_wide(&label) } else { String::new() },
                file_system: if ok {
                    from_wide(&fs_name)
                } else {
                    String::new()
                },
                kind,
                total_bytes: total,
                free_bytes: free,
            });
        }
        SetErrorMode(previous);
    }
    drives
}

#[cfg(not(windows))]
pub fn list() -> Vec<DriveInfo> {
    let mut drives = vec![DriveInfo {
        path: "/".to_string(),
        label: String::new(),
        file_system: String::new(),
        kind: DriveKind::Fixed,
        total_bytes: 0,
        free_bytes: 0,
    }];
    if let Some(home) = std::env::var_os("HOME") {
        drives.push(DriveInfo {
            path: home.to_string_lossy().into_owned(),
            label: "Home".to_string(),
            ..drives[0].clone()
        });
    }
    drives
}
