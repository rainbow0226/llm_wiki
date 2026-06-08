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
└── skills/               Claude Code skill 的权威副本（仓库内 .claude/ 被 gitignore）
    ├── kb-lint/SKILL.md         知识库巡检 lint
    ├── wiki-add/SKILL.md        写一页成品知识进 wiki（POST /sources 写入通道）
    ├── wiki-distill/SKILL.md    会话/调研 → learning 页（同写入通道）
    ├── wiki-find/SKILL.md       快速单跳召回，summary 直拼（~5s）
    └── wiki-find-deep/SKILL.md  多跳图谱展开召回，按 bc 聚类（~30-60s）
```

> 命名约定：知识库 skill 统一 `wiki-*` 前缀（写入 `wiki-add`/`wiki-distill`，检索 `wiki-find`/`wiki-find-deep`）。
> `kb-lint` 是 P1 既有巡检 skill，保留原名。

## 安装 skills

`.claude/` 是本地配置、被 git 忽略，故 skill 的权威副本放在 `wiki-toolkit/skills/`。
消费端按需安装到工作目录的 `.claude/skills/`：

```
cp -r wiki-toolkit/skills/{kb-lint,wiki-add,wiki-distill,wiki-find,wiki-find-deep} <work-dir>/.claude/skills/
```

四个 `wiki-*` skill 走 HTTP API（写入 `POST /sources`，检索 `POST /search` + `GET /graph`），
鉴权用环境变量 `LLM_WIKI_API_TOKEN`（或 Settings → API Server 里允许无 token / 配 token）；
若配了 llm_wiki MCP server，检索可改用 `llm_wiki_search`/`llm_wiki_graph`/`llm_wiki_read_file`，数据等价。
curl 鉴权统一用 `AUTH=(); [ -n "$TOKEN" ] && AUTH=(-H "Authorization: Bearer $TOKEN")` 模式（bash/zsh 均稳）。

## 下发到消费者 vault

```
cp wiki-toolkit/_meta-templates/bc-registry.yaml      <vault>/wiki/_meta/
cp wiki-toolkit/_meta-templates/scope-vocabulary.yaml <vault>/wiki/_meta/
# 然后按本库知识域编辑 contexts / keyword_to_bc
```

页面模板按需取用（`templates/<type>.md`）。字段的权威定义在消费者 vault 的 `schema.md`；
校验由 `/kb-lint` 完成。
