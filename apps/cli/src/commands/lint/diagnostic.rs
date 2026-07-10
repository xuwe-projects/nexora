//! Lint 诊断、分级与输出格式。

use std::{fmt::Write as _, path::PathBuf};

use serde_json::json;

/// 单条诊断的严重级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Severity {
    /// 启发式规则产生的警告，默认不会让命令失败。
    Warning,
    /// 确定违反规范的错误，会让命令返回非零退出码。
    Error,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// 团队规则产生的一条结构化诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Diagnostic {
    severity: Severity,
    rule: &'static str,
    message: String,
    path: PathBuf,
    line: usize,
    column: usize,
    help: Option<String>,
}

impl Diagnostic {
    /// 创建一条确定性错误诊断。
    pub(super) fn error(
        rule: &'static str,
        path: impl Into<PathBuf>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Error, rule, path, line, column, message)
    }

    /// 创建一条启发式警告诊断。
    pub(super) fn warning(
        rule: &'static str,
        path: impl Into<PathBuf>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Warning, rule, path, line, column, message)
    }

    fn new(
        severity: Severity,
        rule: &'static str,
        path: impl Into<PathBuf>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            rule,
            message: message.into(),
            path: path.into(),
            line,
            column,
            help: None,
        }
    }

    /// 为诊断补充面向开发者的修复建议。
    pub(super) fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// 返回稳定的规则标识。
    pub(super) fn rule(&self) -> &'static str {
        self.rule
    }

    /// 返回当前诊断是否属于允许带原因豁免的启发式警告。
    pub(super) fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }
}

/// 一次 workspace 检查产生的全部诊断。
#[derive(Debug, Default)]
pub(super) struct Report {
    diagnostics: Vec<Diagnostic>,
}

impl Report {
    /// 添加一条诊断。
    pub(super) fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// 按文件位置和规则名称稳定排序，保证本地与 CI 输出一致。
    pub(super) fn sort(&mut self) {
        self.diagnostics.sort_by(|left, right| {
            (&left.path, left.line, left.column, left.severity, left.rule).cmp(&(
                &right.path,
                right.line,
                right.column,
                right.severity,
                right.rule,
            ))
        });
    }

    /// 返回错误数量。
    pub(super) fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .count()
    }

    /// 返回警告数量。
    pub(super) fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Warning)
            .count()
    }

    /// 将诊断渲染成人类可读的终端文本。
    pub(super) fn render_human(&self) -> String {
        let mut output = String::new();

        for diagnostic in &self.diagnostics {
            let _ = writeln!(
                output,
                "{}[{}] {}:{}:{}: {}",
                diagnostic.severity.label(),
                diagnostic.rule,
                diagnostic.path.display(),
                diagnostic.line,
                diagnostic.column,
                diagnostic.message
            );
            if let Some(help) = &diagnostic.help {
                let _ = writeln!(output, "  help: {help}");
            }
        }

        let _ = writeln!(
            output,
            "lint result: {} error(s), {} warning(s)",
            self.error_count(),
            self.warning_count()
        );
        output
    }

    /// 将诊断渲染成供 CI 和编辑器消费的 JSON。
    pub(super) fn render_json(&self) -> Result<String, serde_json::Error> {
        let diagnostics = self
            .diagnostics
            .iter()
            .map(|diagnostic| {
                json!({
                    "severity": diagnostic.severity.label(),
                    "rule": diagnostic.rule,
                    "message": diagnostic.message,
                    "path": diagnostic.path.to_string_lossy(),
                    "line": diagnostic.line,
                    "column": diagnostic.column,
                    "help": diagnostic.help,
                })
            })
            .collect::<Vec<_>>();

        serde_json::to_string_pretty(&json!({
            "diagnostics": diagnostics,
            "summary": {
                "errors": self.error_count(),
                "warnings": self.warning_count(),
            }
        }))
    }
}
