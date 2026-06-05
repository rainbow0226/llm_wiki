# Page Templates

每个 wiki 页面类型的骨架模板。`<...>` 是占位符，使用时替换。

| 模板 | type | 目录 | 用途 |
|------|------|------|------|
| `entity.md` | entity | `wiki/entities/` | 具名事物（人/工具/组织/协议/数据集） |
| `concept.md` | concept | `wiki/concepts/` | 概念、技术、框架 |
| `source.md` | source | `wiki/sources/` | 论文、文章、书、博客 |
| `comparison.md` | comparison | `wiki/comparisons/` | 并列对比 |
| `query.md` | query | `wiki/queries/` | 开放问题 |
| `synthesis.md` | synthesis | `wiki/synthesis/` | 跨页综述收口 |
| `solution.md` | solution | `wiki/solutions/` | 跨域方案 hub |
| `playbook.md` | playbook | `wiki/playbooks/` | 可操作步骤手册 |
| `decision.md` | decision | `wiki/decisions/` | 决策记录 / ADR |
| `learning.md` | learning | `wiki/learnings/` | 经验沉淀（仅溯源） |
| `bc-readme.md` | bc-readme | `wiki/_meta/` | 知识域总览页 |

## 强制 frontmatter 字段

所有模板均含企业级必填字段：`type / title / summary / bc / sdlc_phases / tags / related / created / updated`。

- `summary`：100-150 字中文、自包含，是召回唯一直拼字段，禁止留空。
- `bc`：取值必须在消费者 vault 的 `wiki/_meta/bc-registry.yaml` 在册。
- `sdlc_phases`：取值限 `design/dev/test/ops`，纯概念页可为 `[]`。

`/kb-lint` 按上述规则校验。字段权威定义见消费者 vault 的 `schema.md`。
