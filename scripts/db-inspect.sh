#!/bin/bash
#
# Decrypted database inspector for Crate (development tooling)
#
# Exports a plaintext SNAPSHOT of the SQLCipher database via sqlcipher_export()
# and opens it in DB Browser for SQLite. The live database is never modified:
# GUI edits only ever hit the disposable copy under /tmp.
#
# Desktop-only by design: the key is read from `db.key` in the app data dir
# (the desktop FileKeyProvider). On mobile the key lives in the Keychain /
# Keystore, so there is nothing to read here.
#
# Usage:
#   ./scripts/db-inspect.sh           # inspect the dev build (crate-dev)
#   ./scripts/db-inspect.sh prod      # inspect the production build
#
# Requirements: sqlcipher + sqlitebrowser (Arch: pacman -S sqlcipher sqlitebrowser)

set -euo pipefail

# --- Resolve which install to inspect --------------------------------------
case "${1:-dev}" in
	dev)  IDENTIFIER="com.bbx-audio.crate-dev" ;;
	prod) IDENTIFIER="com.bbx-audio.crate" ;;
	*)
		echo "usage: $0 [dev|prod]" >&2
		exit 1
		;;
esac

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/$IDENTIFIER"
DB_PATH="$DATA_DIR/crate.db"
KEY_PATH="$DATA_DIR/db.key"
SNAPSHOT_DIR="/tmp/crate-inspect"
SNAPSHOT_PATH="$SNAPSHOT_DIR/$IDENTIFIER.db"

# --- Preflight checks -------------------------------------------------------
for dep in sqlcipher sqlitebrowser; do
	command -v "$dep" >/dev/null 2>&1 || {
		echo "error: '$dep' is not installed (Arch: sudo pacman -S $dep)" >&2
		exit 1
	}
done

[ -f "$DB_PATH" ] || { echo "error: database not found at $DB_PATH — did you run the app at least once?" >&2; exit 1; }
[ -f "$KEY_PATH" ] || { echo "error: key file not found at $KEY_PATH" >&2; exit 1; }

KEY=$(cat "$KEY_PATH")

# --- Export a decrypted snapshot -------------------------------------------
# sqlcipher_export() appends into an existing target file, so always start from
# a clean directory; a stale snapshot would otherwise mix old rows with new.
rm -rf "$SNAPSHOT_DIR"
mkdir -p "$SNAPSHOT_DIR"

# Key is passed through stdin, never argv, so it does not show up in `ps`.
sqlcipher "$DB_PATH" <<SQL
PRAGMA key="$KEY";
ATTACH DATABASE '$SNAPSHOT_PATH' AS plain KEY '';
SELECT sqlcipher_export('plain');
DETACH DATABASE plain;
SQL

# Verify the snapshot really is plain, readable SQLite (SQLCipher opens
# unencrypted files transparently when no key is set).
CHECK=$(sqlcipher "$SNAPSHOT_PATH" "PRAGMA integrity_check; SELECT count(*) || ' tables' FROM sqlite_master WHERE type='table';" 2>&1)
grep -q '^ok$' <<<"$CHECK" || {
	echo "error: snapshot verification failed:" >&2
	echo "$CHECK" >&2
	exit 1
}

# --- Launch DB Browser ------------------------------------------------------
# Arch's sqlitebrowser is Qt5 and ships only the xcb platform plugin (no
# qt5-wayland). Under a Wayland session an inherited QT_QPA_PLATFORM=wayland
# aborts the app at startup, so force xcb whenever an X display exists.
if [ -n "${DISPLAY:-}" ]; then
	export QT_QPA_PLATFORM=xcb
elif [ -n "${WAYLAND_DISPLAY:-}" ]; then
	echo "warning: no DISPLAY available; if sqlitebrowser aborts, start an XWayland session or set DISPLAY" >&2
fi

echo "Snapshot: $SNAPSHOT_PATH (${CHECK//$'\n'/, })"
nohup sqlitebrowser "$SNAPSHOT_PATH" >/dev/null 2>&1 &
disown

echo "DB Browser launched. Re-run this script anytime for a fresh snapshot."
