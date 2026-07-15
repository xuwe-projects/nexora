#!/bin/sh

pid="$1"
current_app="$2"
staged_app="$3"
staging_root="$4"
backup_app="${current_app}.xuwe-updater-backup"

while kill -0 "$pid" 2>/dev/null; do
  sleep 0.1
done

if [ -e "$backup_app" ] && ! rm -rf "$backup_app"; then
  rm -rf "$staging_root"
  exit 1
fi

if ! mv "$current_app" "$backup_app"; then
  rm -rf "$staging_root"
  exit 1
fi

if mv "$staged_app" "$current_app"; then
  open "$current_app"
  rm -rf "$backup_app"
  rm -rf "$staging_root"
  exit 0
fi

rm -rf "$current_app"
if mv "$backup_app" "$current_app"; then
  open "$current_app"
  rm -rf "$staging_root"
fi
exit 1
