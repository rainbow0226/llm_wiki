# wiki-toolkit

中心化知识工具链：模板与元数据**模板**集中在本目录（随 fork 仓库分发），
消费者 vault 持有自己的实例，**单向下发、不反向同步**（计划取舍：「消费者 wiki 不持副本」）。

## 结构

```
wiki-toolkit/
├── templates/            各页面类型骨架（11 类，含 5 个企业级类型）
├── _meta-templates/      wiki/_meta/ 下文件的模板
│   ├── bc-registry.yaml      bounded context 注册表模板
│   └── scope-vocabulary.yaml 受控词表模板
├── skills/               Claude Code skill 的权威副本（仓库内 .claude/ 被 gitignore）
│   ├── wiki-lint/SKILL.md         知识库巡检 lint
│   ├── wiki-add/SKILL.md        写一页成品知识进 wiki（POST /sources 写入通道）
│   ├── wiki-distill/SKILL.md    会话/调研 → learning 页（同写入通道）
│   ├── wiki-find/SKILL.md       快速单跳召回，summary 直拼（~5s）
│   └── wiki-find-deep/SKILL.md  多跳图谱展开召回，按 bc 聚类（~30-60s）
├── hooks/                Claude Code hooks（权威副本）
│   ├── session-start.sh        SessionStart：把 hot.md + 当前知识域注入会话上下文
│   ├── stop.sh                 Stop：本会话 wiki/ 有变更则刷新 hot.md（非阻塞，防循环）
│   └── settings-snippet.json   注册片段（合并进 <vault>/.claude/settings.json）
└── scripts/
    └── build-hot.py            生成 <vault>/hot.md（最近更新 + 链接中心度；零依赖 stdlib）
```

> 命名约定：知识库 skill 全部统一 `wiki-*` 前缀 —— 写入 `wiki-add`/`wiki-distill`，检索 `wiki-find`/`wiki-find-deep`，
> 巡检 `wiki-lint`（原 `kb-lint`，已改名；旧名仍保留为触发别名）。

## 接入 Claude Code（MCP server）

dev_wiki 的 MCP server 暴露 `llm_wiki_*` 工具（`status`/`projects`/`files`/`read_file`/`search`/`graph`/`rescan_sources`/`graph_traverse`）。
先 build，再用官方 CLI 注册到**用户级**配置（`~/.claude.json`），任何项目里都能用：

```
npm --prefix mcp-server run build          # 产物 mcp-server/dist/src/index.js
claude mcp add llm_wiki -s user \
  -e LLM_WIKI_API_BASE_URL=http://127.0.0.1:19828 \
  -- node <repo>/mcp-server/dist/src/index.js
claude mcp list                            # 应显示  llm_wiki  ✔ Connected
```

- env 只需 `LLM_WIKI_API_BASE_URL`（默认 `http://127.0.0.1:19828`）；启用鉴权再加 `-e LLM_WIKI_API_TOKEN=…`。
- **无需 `NO_PROXY`**：MCP server 是 Node（全局 `fetch`/undici **不读** `HTTP_PROXY`），不像 Python MCP 那样会把 localhost 请求送进代理。
- 改了服务端口/路径：`claude mcp remove llm_wiki -s user` 后重新 `add`，或直接编辑 `~/.claude.json`。

## 运行态前置（MCP 工具与 `wiki-*` skills 都依赖）

两者都通过 HTTP API 打到**运行中的 dev_wiki.app**（`:19828`）。实测前必须：

1. 启动 dev_wiki.app → 打开目标项目 → Settings → **开启 API Server**（allowUnauthenticated，或配 token 并把同一 token 设进 `LLM_WIKI_API_TOKEN`）。
2. 配 **embedding model/key**，向量召回才工作；否则只有关键词（BM25）半环，hybrid 的向量半环静默缺席。

> app 未运行时：MCP 仍会 `✔ Connected`（那只是 stdio 层握手），但工具调用与 skill 的 curl 会连不上 API（连接拒绝 / 502）。

## 安装 skills

`.claude/` 是本地配置、被 git 忽略，故 skill 的权威副本放在 `wiki-toolkit/skills/`。
按需安装，两种作用域二选一：

```
# 用户级（推荐用于实测）：装一次，任何项目里都能 /wiki-find 等
cp -r wiki-toolkit/skills/{wiki-lint,wiki-add,wiki-distill,wiki-find,wiki-find-deep} ~/.claude/skills/

# 或项目级：仅在该工作目录可见
cp -r wiki-toolkit/skills/{wiki-lint,wiki-add,wiki-distill,wiki-find,wiki-find-deep} <work-dir>/.claude/skills/
```

其中四个走 HTTP API（写入 `POST /sources`，检索 `POST /search` + `GET /graph`；`wiki-lint` 是只读 schema 巡检，读 vault 文件不打 API），
鉴权用环境变量 `LLM_WIKI_API_TOKEN`（或 Settings → API Server 里允许无 token / 配 token）；
若配了 llm_wiki MCP server，检索可改用 `llm_wiki_search`/`llm_wiki_graph`/`llm_wiki_read_file`，数据等价。
curl 鉴权统一用 `AUTH=(); [ -n "$TOKEN" ] && AUTH=(-H "Authorization: Bearer $TOKEN")` 模式（bash/zsh 均稳）。

## 安装 hooks（SessionStart / Stop + hot.md）

Hooks 必须在 `<vault>/.claude/settings.json` 里注册才生效（与 skill 不同，放进目录还不够）。

```
# 1. 装脚本（build-hot.py 与 .sh 同放 .claude/hooks/，stop.sh 会就近找它）
mkdir -p <vault>/.claude/hooks
cp wiki-toolkit/hooks/session-start.sh wiki-toolkit/hooks/stop.sh <vault>/.claude/hooks/
cp wiki-toolkit/scripts/build-hot.py                              <vault>/.claude/hooks/
chmod +x <vault>/.claude/hooks/*.sh

# 2. 把 wiki-toolkit/hooks/settings-snippet.json 里的 "hooks" 块合并进
#    <vault>/.claude/settings.json（已有 settings 则手动并入，勿整体覆盖）

# 3. 首次生成 hot.md（之后 Stop hook 会在 wiki/ 有变更时自动刷新）
python3 wiki-toolkit/scripts/build-hot.py --vault <vault>
```

- **SessionStart**：纯 stdout 注入会话上下文 —— 把 `<vault>/hot.md` + 当前知识域列表喂给模型；
  并写一个 `.llm-wiki/.session-hot-mark` 标记。**不在 dev_wiki vault（无 `wiki/`）时静默 no-op。**
- **Stop**：若本会话内 `wiki/*.md` 有改动（对比标记 mtime）则重跑 `build-hot.py` 刷新 hot.md；
  非阻塞（始终 exit 0），用 `stop_hook_active` 防循环；hot.md 在 **vault 根**（不在 `wiki/` 内），
  刷新它不会再触发变更 → 无环。
- **hot.md 排序**：最近更新 + 链接中心度（入/出 `[[链接]]` 度数）。**访问频次维度待 P4 召回日志**。
  hot.md 放 vault 根 → 对关键词搜索 / `/wiki-lint` / 文件 API 隐形，仅 SessionStart hook 直接读盘。

## 下发到消费者 vault

```
cp wiki-toolkit/_meta-templates/bc-registry.yaml      <vault>/wiki/_meta/
cp wiki-toolkit/_meta-templates/scope-vocabulary.yaml <vault>/wiki/_meta/
# 然后按本库知识域编辑 contexts / keyword_to_bc
```

页面模板按需取用（`templates/<type>.md`）。字段的权威定义在消费者 vault 的 `schema.md`；
校验由 `/wiki-lint` 完成。
