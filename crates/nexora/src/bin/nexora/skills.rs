//! 生成项目携带的 Agent Skill 模板。

use askama::Template as _;

#[derive(askama::Template)]
#[template(source = "{{ contents }}", ext = "txt", escape = "none")]
struct SkillTemplate<'a> {
    contents: &'a str,
}

struct SkillSource {
    relative_path: &'static str,
    contents: &'static str,
}

macro_rules! skill_source {
    ($path:literal) => {
        SkillSource {
            relative_path: concat!(".agents/skills/", $path),
            contents: include_str!(concat!("../../../templates/skills/", $path)),
        }
    };
}

const SKILL_SOURCES: &[SkillSource] = &[
    skill_source!("api-interface-design/SKILL.md"),
    skill_source!("api-interface-design/agents/openai.yaml"),
    skill_source!("axum-web-framework/SKILL.md"),
    skill_source!("axum-web-framework/agents/openai.yaml"),
    skill_source!("build-modular-axum-backend/SKILL.md"),
    skill_source!("build-modular-axum-backend/agents/openai.yaml"),
    skill_source!("build-modular-axum-backend/references/architecture.md"),
    skill_source!("build-modular-axum-backend/references/configuration.md"),
    skill_source!("build-modular-axum-backend/references/contracts.md"),
    skill_source!("build-modular-axum-backend/references/migrations.md"),
    skill_source!("build-modular-axum-backend/references/module-template.md"),
    skill_source!("define-page/SKILL.md"),
    skill_source!("define-page/agents/openai.yaml"),
    skill_source!("desktop-ui-component-selection/SKILL.md"),
    skill_source!("desktop-ui-component-selection/agents/openai.yaml"),
    skill_source!("develop-nexora-apps/SKILL.md"),
    skill_source!("develop-nexora-apps/agents/openai.yaml"),
    skill_source!("git-commit/SKILL.md"),
    skill_source!("git-commit/agents/openai.yaml"),
    skill_source!("publish-nexora-release/SKILL.md"),
    skill_source!("publish-nexora-release/agents/openai.yaml"),
    skill_source!("gpui-component/SKILL.md"),
    skill_source!("gpui-component/agents/openai.yaml"),
    skill_source!("gpui-component/references/style-guide.md"),
    skill_source!("gpui-component/references/usage.md"),
    skill_source!("gpui-desktop-development/SKILL.md"),
    skill_source!("gpui-desktop-development/agents/openai.yaml"),
    skill_source!("gpui-test/SKILL.md"),
    skill_source!("gpui/SKILL.md"),
    skill_source!("gpui/agents/openai.yaml"),
    skill_source!("gpui/references/action.md"),
    skill_source!("gpui/references/async.md"),
    skill_source!("gpui/references/context.md"),
    skill_source!("gpui/references/element-advanced.md"),
    skill_source!("gpui/references/element-api.md"),
    skill_source!("gpui/references/element-best-practices.md"),
    skill_source!("gpui/references/element-examples.md"),
    skill_source!("gpui/references/element-id.md"),
    skill_source!("gpui/references/element-patterns.md"),
    skill_source!("gpui/references/element.md"),
    skill_source!("gpui/references/entity-advanced.md"),
    skill_source!("gpui/references/entity-api.md"),
    skill_source!("gpui/references/entity-best-practices.md"),
    skill_source!("gpui/references/entity-patterns.md"),
    skill_source!("gpui/references/entity.md"),
    skill_source!("gpui/references/event.md"),
    skill_source!("gpui/references/focus-handle.md"),
    skill_source!("gpui/references/global.md"),
    skill_source!("gpui/references/layout-style.md"),
    skill_source!("gpui/references/test-examples.md"),
    skill_source!("gpui/references/test-reference.md"),
    skill_source!("gpui/references/test.md"),
    skill_source!("karpathy-guidelines/SKILL.md"),
    skill_source!("karpathy-guidelines/agents/openai.yaml"),
    skill_source!("rust-code-style/SKILL.md"),
    skill_source!("rust-code-style/agents/openai.yaml"),
    skill_source!("rust-technology-selection/SKILL.md"),
    skill_source!("rust-technology-selection/agents/openai.yaml"),
    skill_source!("sqlx-database-code-review/SKILL.md"),
    skill_source!("sqlx-database-code-review/agents/openai.yaml"),
    skill_source!("sqlx-database-code-review/references/migrations.md"),
    skill_source!("sqlx-database-code-review/references/queries.md"),
    skill_source!("sqlx-database-code-review/references/review-verification-protocol.md"),
];

/// 返回写入 Skill 文件前需要按顺序创建的目录。
pub(super) const DIRECTORIES: &[&str] = &[
    ".agents",
    ".agents/skills",
    ".agents/skills/api-interface-design",
    ".agents/skills/api-interface-design/agents",
    ".agents/skills/axum-web-framework",
    ".agents/skills/axum-web-framework/agents",
    ".agents/skills/build-modular-axum-backend",
    ".agents/skills/build-modular-axum-backend/agents",
    ".agents/skills/build-modular-axum-backend/references",
    ".agents/skills/define-page",
    ".agents/skills/define-page/agents",
    ".agents/skills/desktop-ui-component-selection",
    ".agents/skills/desktop-ui-component-selection/agents",
    ".agents/skills/develop-nexora-apps",
    ".agents/skills/develop-nexora-apps/agents",
    ".agents/skills/git-commit",
    ".agents/skills/git-commit/agents",
    ".agents/skills/publish-nexora-release",
    ".agents/skills/publish-nexora-release/agents",
    ".agents/skills/gpui",
    ".agents/skills/gpui/agents",
    ".agents/skills/gpui/references",
    ".agents/skills/gpui-component",
    ".agents/skills/gpui-component/agents",
    ".agents/skills/gpui-component/references",
    ".agents/skills/gpui-desktop-development",
    ".agents/skills/gpui-desktop-development/agents",
    ".agents/skills/gpui-test",
    ".agents/skills/karpathy-guidelines",
    ".agents/skills/karpathy-guidelines/agents",
    ".agents/skills/rust-code-style",
    ".agents/skills/rust-code-style/agents",
    ".agents/skills/rust-technology-selection",
    ".agents/skills/rust-technology-selection/agents",
    ".agents/skills/sqlx-database-code-review",
    ".agents/skills/sqlx-database-code-review/agents",
    ".agents/skills/sqlx-database-code-review/references",
];

/// 渲染全部 Agent Skill，并保留它们在目标项目中的相对路径。
///
/// # Errors
///
/// 任一文本模板无法由 Askama 渲染时返回包含相对路径的错误。
pub(super) fn render() -> Result<Vec<(String, String)>, String> {
    SKILL_SOURCES
        .iter()
        .map(|source| {
            let contents = SkillTemplate {
                contents: source.contents,
            }
            .render()
            .map_err(|error| {
                format!(
                    "无法渲染 Agent Skill 模板 `{}`：{error}",
                    source.relative_path
                )
            })?;
            Ok((source.relative_path.to_owned(), contents))
        })
        .collect()
}
