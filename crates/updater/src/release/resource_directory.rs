use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutableLayout {
    MacOs,
    ExecutableDirectory,
}

pub(crate) fn from_executable(
    executable: &Path,
    layout: ExecutableLayout,
) -> Result<PathBuf, &'static str> {
    let executable_directory = executable.parent().ok_or("当前可执行文件没有父目录")?;
    if layout == ExecutableLayout::MacOs
        && executable_directory
            .file_name()
            .and_then(|name| name.to_str())
            == Some("MacOS")
        && executable_directory
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("Contents")
    {
        return executable_directory
            .parent()
            .map(|contents| contents.join("Resources"))
            .ok_or("macOS bundle 目录结构无效");
    }
    Ok(executable_directory.to_path_buf())
}
