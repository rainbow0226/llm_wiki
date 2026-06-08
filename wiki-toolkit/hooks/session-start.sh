#!/usr/bin/env bash
# dev_wiki SessionStart hook — load hot.md + current bounded-context list into
# the session as context (plain stdout on a SessionStart hook is injected as
# context). No-op when the project dir isn't a dev_wiki vault.
#
# Register in <vault>/.claude/settings.json (see wiki-toolkit/hooks/settings-snippet.json).
set -euo pipefail

cat >/dev/null 2>&1 || true   # drain stdin JSON (unused; cwd comes from env)

VAULT="${CLAUDE_PROJECT_DIR:-$PWD}"
[ -d "$VAULT/wiki" ] || exit 0   # not a dev_wiki vault → stay silent

HOT="$VAULT/hot.md"
REG="$VAULT/wiki/_meta/bc-registry.yaml"

# Drop a marker so the Stop hook can tell whether wiki/ changed this session.
mkdir -p "$VAULT/.llm-wiki" 2>/dev/null || true
: > "$VAULT/.llm-wiki/.session-hot-mark" 2>/dev/null || true

echo "# dev_wiki 知识库上下文"
echo
if [ -f "$HOT" ]; then
  cat "$HOT"
else
  echo "（hot.md 未生成 — 运行 \`python3 wiki-toolkit/scripts/build-hot.py\` 生成热点页索引）"
fi

if [ -f "$REG" ]; then
  echo
  echo "## 知识域 (bounded contexts)"
  # Pair each \`- id:\` with the \`title:\` line that follows it (no yaml dep).
  awk '
    /^[[:space:]]*-[[:space:]]*id:/   { id=$0; sub(/.*id:[[:space:]]*/,"",id); next }
    /^[[:space:]]*title:/ && id!=""   { t=$0; sub(/.*title:[[:space:]]*/,"",t);
                                        gsub(/^["\x27]|["\x27]$/,"",id);
                                        print "- " id "：" t; id="" }
  ' "$REG"
fi

exit 0
