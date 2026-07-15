use std::{error::Error, fmt};

#[test]
fn error_chain_keeps_actionable_context_and_root_cause() {
    let error = OuterError { source: InnerError };

    assert_eq!(
        logging::format_error_chain(&error),
        "应用启动失败: 数据库连接超时"
    );
}

#[test]
fn subscriber_initialization_returns_a_result_instead_of_panicking() {
    logging::initialize("off").expect("测试进程应当能够安装全局日志订阅器");
}

#[derive(Debug)]
struct OuterError {
    source: InnerError,
}

impl fmt::Display for OuterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("应用启动失败")
    }
}

impl Error for OuterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
struct InnerError;

impl fmt::Display for InnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("数据库连接超时")
    }
}

impl Error for InnerError {}
