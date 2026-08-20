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

## Compatibility and upgrade

- Downstream applications do not copy path logic or change `initialize(None)`. Upgrade Nexora and
  rebuild formal installers with the new CLI. Older packages without `nexora-release.json` are not
  recognized as the new formal configuration boundary.
- This change updates the `develop-nexora-apps`, `develop-nexora-updater`, and updater-protocol
  Skills. Downstream projects should synchronize `.agents/skills` while upgrading.
