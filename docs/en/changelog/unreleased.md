# Nexora Unreleased

- Desktop applications can now register additional independent Windows that remain available before
  Account sign-in with `ApplicationOptions::unauthenticated_window("window-id")`. `settings` remains
  available by default, while unknown, non-Window, and duplicate IDs fail startup validation.
