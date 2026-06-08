# dev_wiki 召回质量评估 (P4②)

`wiki-toolkit/scripts/eval-recall.py` 把「检索好不好」变成可比较的数字：
对一个**固定查询集**算 **Hit@k / MRR / nDCG@k**。它是 P4③ 用 BM25 替换
`score_file()` 前后的**回归门禁**——同一查询集跑两次、diff 两份报告，涨跌一目了然。

零依赖（Python stdlib），与 `build-hot.py` 一致。

## 查询集格式

见 `queries.example.json`。每条查询：

| 字段 | 必填 | 说明 |
|------|------|------|
| `id` | ✓ | 查询唯一 id |
| `query` | ✓ | 查询文本 |
| `relevant` | ✓ | 期望命中的 page_id 列表（＝文件 stem，如 `wiki/x/foo.md` → `foo`）|
| `bc` | | 限定 bounded context（同时约束 `--log` 匹配）|
| `phase` | | SDLC 阶段（P4③ 才真正影响排序；现仅记录/约束日志匹配）|
| `graded` | | `{page_id: gain}` 分级相关度，喂 nDCG；缺省时每个 `relevant` 页 gain=1 |

顶层 `k_values`（默认 `[1,3,5,10]`）决定算哪些 k 的 Hit@k / nDCG@k。

**真实查询集放各自 vault 的 `wiki/_meta/eval/queries.json`**（与 P4① 落的
`retrieval-log.jsonl` 同处）。toolkit 只发 schema + 引擎，不发语料专属答案。

## 三种结果来源（每条查询的排序 page_id 列表）

```bash
# 1) --api：打真实引擎（也＝live 端到端检查），需 app 运行
eval-recall.py --queries wiki/_meta/eval/queries.json \
  --api http://127.0.0.1:19828 --project current \
  --token "$LLM_WIKI_API_TOKEN" --out report.json
# 内部 POST /search {trace:true} 读 trace.candidates（按 finalRank 排序）

# 2) --log：回放 P4① 的 retrieval-log.jsonl（在线漂移模式，不需 app）
eval-recall.py --queries wiki/_meta/eval/queries.json \
  --log wiki/_meta/retrieval-log.jsonl

# 3) --results：离线确定性（CI 门禁 / selftest 用）
#    captured.json = {"query_id": ["pageA","pageB",...], ...}
eval-recall.py --queries wiki/_meta/eval/queries.json \
  --results captured.json --out report.json
```

## 回归门禁（BM25 前后对比）

```bash
# BM25 前：建基线
eval-recall.py --queries q.json --api ... --out baseline.json

# 改完 BM25 后：与基线比，任一指标跌破 --tolerance 即 exit 2
eval-recall.py --queries q.json --api ... --baseline baseline.json --out after.json
```

`--baseline` 比较聚合指标（macro-average）；只罚下跌、不罚上涨。

## 自检

```bash
eval-recall.py --selftest   # 度量数学 + 日志回放 + 门禁逻辑的内置断言
```

度量是纯函数，selftest 离线可验，不需 app/语料。
