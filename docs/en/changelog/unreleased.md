# Nexora Unreleased

## Fixed

- `nexora::config::initialize(None)` now uses the current executable and a valid
  `nexora-release.json` to load the selected channel's frozen `runtime_config` from formal macOS
  and Windows packages. Production no longer depends on cwd or a source checkout. Missing,
  unreadable, or invalid bundle configuration fails without development fallback; explicit paths,
  user positional arguments, and environment overrides keep their precedence. Updater health
  argument pairs are never mistaken for a configuration path.

  — Owner: [@openai](https://github.com/openai)
  — Related issue/PR: none

- Add regression coverage for macOS `Contents/Resources/config`, Windows EXE-sibling `config`,
  formal/development precedence and failure boundaries, plus nightly/beta/stable frozen files.

  — Owner: [@openai](https://github.com/openai)
  — Related issue/PR: none

## Changed

- The `publish-nexora-release` Skill now generates in-app Updater `release.notes` separately from
  GitHub Releases and developer changelogs. In-app notes use the selected app's resolved Cargo
  package version and release-preparation date, retain only non-empty New Features, Fixes, and Other
  Adjustments categories, add Important Notices only for required user action, and translate
  implementation details into user-visible outcomes. GitHub Releases and developer changelogs
  continue to retain complete technical changes, owners, Issue/PR links, upgrade steps, and
  validation results.

  — Owner: [@openai](https://github.com/openai)
  — Related issue/PR: none

- Synchronize the CLI scaffold Skill mirror, bilingual CLI/updater documentation, and generated-
  project regression assertions. The Updater still only validates, freezes, signs, publishes, and
  renders safe Markdown; it does not parse the new headings or categories.

  — Owner: [@openai](https://github.com/openai)
  — Related issue/PR: none

## Compatibility and upgrade

- Downstream applications do not copy path logic or change `initialize(None)`. Upgrade Nexora and
  rebuild formal installers with the new CLI. Older packages without `nexora-release.json` are not
  recognized as the new formal configuration boundary.
- This change updates the `develop-nexora-apps`, `develop-nexora-updater`, updater-protocol, and
  `publish-nexora-release` Skills. Downstream projects should synchronize `.agents/skills` while
  upgrading.
