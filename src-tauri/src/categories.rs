pub fn category(extension: &str) -> String {
    match extension.to_ascii_lowercase().as_str() {
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt" => {
            "Documents"
        }
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "svg" | "raw" => "Images",
        "mp4" | "mkv" | "mov" | "avi" | "webm" => "Videos",
        "mp3" | "wav" | "flac" | "aac" | "m4a" => "Audio",
        "zip" | "rar" | "7z" | "tar" | "gz" | "iso" => "Archives",
        "exe" | "msi" | "dmg" | "app" | "apk" => "Applications",
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "java" | "c" | "cpp" | "h" | "cs" | "go"
        | "html" | "css" | "json" => "Code",
        "tmp" | "temp" | "cache" | "log" => "Temporary",
        _ => "Other",
    }
    .to_string()
}
pub fn protected(path: &std::path::Path) -> bool {
    const PROTECTED_COMPONENTS: &[&str] = &[
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "$recycle.bin",
        "system volume information",
        "recovery",
        "boot",
        "efi",
        "system",
        "library",
    ];
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        value
            .split(['\\', '/'])
            .map(|part| part.trim_end_matches(':'))
            .any(|part| PROTECTED_COMPONENTS.contains(&part))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn categorizes_by_extension_not_name() {
        assert_eq!(category("PDF"), "Documents");
        assert_eq!(category("mp4"), "Videos");
        assert_eq!(category("unknown"), "Other")
    }
    #[test]
    fn recognizes_system_paths() {
        assert!(protected(Path::new("C:\\Windows\\System32\\x.dll")));
        assert!(protected(Path::new("C:\\ProgramData\\vendor\\x.dat")));
        assert!(protected(Path::new("C:\\$Recycle.Bin\\x.dat")));
        assert!(protected(Path::new("C:\\Windows")));
        assert!(!protected(Path::new("D:\\Scans\\cache.tmp")))
    }

    #[test]
    fn does_not_protect_similarly_named_user_folders() {
        assert!(!protected(Path::new("D:\\My Windows Backup\\photo.jpg")));
        assert!(!protected(Path::new(
            "D:\\program files archive\\notes.txt"
        )));
    }
}
