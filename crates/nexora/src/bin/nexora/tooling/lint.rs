//! Workspace 团队规范 lint 入口。

#[path = "lint/cargo.rs"]
mod cargo;
#[path = "lint/diagnostic.rs"]
mod diagnostic;
#[path = "lint/source.rs"]
mod source;

use std::path::Path;

use self::{cargo::Workspace, diagnostic::Report};
use super::{CliError, CliResult};

/// 执行 workspace 结构、Rust 源码与 GPUI 约束检查。
pub(super) fn run(workspace: &Path, deny_warnings: bool, json: bool) -> CliResult<()> {
    let workspace = Workspace::load(workspace)?;
    let mut report = Report::default();

    cargo::check(&workspace, &mut report)?;
    source::check(&workspace, &mut report)?;
    report.sort();

    if json {
        println!(
            "{}",
            report
                .render_json()
                .map_err(|error| CliError::new(format!("无法生成 lint JSON：{error}")))?
        );
    } else {
        print!("{}", report.render_human());
    }

    let errors = report.error_count();
    let warnings = report.warning_count();
    if errors > 0 || deny_warnings && warnings > 0 {
        return Err(CliError::new(format!(
            "lint 检查未通过（{errors} 个错误，{warnings} 个警告）"
        )));
    }

    Ok(())
}

/// 把绝对路径转换成相对于 workspace 的稳定诊断路径。
pub(super) fn relative_path(root: &Path, path: &Path) -> std::path::PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// 根据 UTF-8 字节偏移计算一基行号和列号。
pub(super) fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = source.get(..offset).unwrap_or(source);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.chars().count() + 1, |(_, tail)| {
            tail.chars().count() + 1
        });
    (line, column)
}
