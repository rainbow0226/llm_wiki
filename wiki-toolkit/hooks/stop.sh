#!/usr/bin/env bash
# dev_wiki Stop hook — when wiki/ changed during this session, refresh hot.md
# so the next session starts with a current digest. Non-blocking: always
# exits 0, never blocks the stop. No-op outside a dev_wiki vault.
#
# Register in <vault>/.claude/settings.json (see wiki-toolkit/hooks/settings-snippet.json).
set -euo pipefail

INPUT="$(cat 2>/dev/null || true)"

# Loop guard: if we're already inside a stop-hook continuation, do nothing.
if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  [ "$(printf '%s' "$INPUT" | jq -r '.stop_hook_active // false')" = "true" ] && exit 0
fi

VAULT="${CLAUDE_PROJECT_DIR:-$PWD}"
[ -d "$VAULT/wiki" ] || exit 0

MARK="$VAULT/.llm-wiki/.session-hot-mark"
[ -f "$MARK" ] || exit 0   # no session marker → SessionStart never ran here

# Any wiki/*.md modified after the session-start marker? (hot.md lives at the
# vault root, NOT under wiki/, so refreshing it never counts as a change → no loop.)
if [ -z "$(find "$VAULT/wiki" -name '*.md' -newer "$MARK" -print -quit 2>/dev/null)" ]; then
  exit 0
fi

# Locate build-hot.py: co-located with the hooks first (the install copies it
# next to the .sh files), then the in-repo toolkit path as a fallback.
SCRIPT=""
for cand in \
  "$VAULT/.claude/hooks/build-hot.py" \
  "$VAULT/wiki-toolkit/scripts/build-hot.py" \
  "${CLAUDE_PROJECT_DIR:-}/wiki-toolkit/scripts/build-hot.py"; do
  if [ -f "$cand" ]; then SCRIPT="$cand"; break; fi
done
if [ -n "$SCRIPT" ] && command -v python3 >/dev/null 2>&1; then
  # build-hot prints its one-line summary to stderr (shown in transcript/debug).
  python3 "$SCRIPT" --vault "$VAULT" >/dev/null 2>&1 || true
  echo "[dev_wiki] wiki 本次会话有变更 → 已刷新 hot.md" >&2
fi

exit 0
