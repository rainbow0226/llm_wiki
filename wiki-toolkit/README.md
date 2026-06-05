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
    └── kb-lint/SKILL.md      知识库巡检 lint
```

## 安装 skills

`.claude/` 是本地配置、被 git 忽略，故 skill 的权威副本放在 `wiki-toolkit/skills/`。
消费端按需安装到工作目录的 `.claude/skills/`：

```
cp -r wiki-toolkit/skills/kb-lint <work-dir>/.claude/skills/
```

## 下发到消费者 vault

```
cp wiki-toolkit/_meta-templates/bc-registry.yaml      <vault>/wiki/_meta/
cp wiki-toolkit/_meta-templates/scope-vocabulary.yaml <vault>/wiki/_meta/
# 然后按本库知识域编辑 contexts / keyword_to_bc
```

页面模板按需取用（`templates/<type>.md`）。字段的权威定义在消费者 vault 的 `schema.md`；
校验由 `/kb-lint` 完成。
