#!/usr/bin/env python3
"""eval-recall.py — dev_wiki retrieval-quality evaluation (P4②).

Scores a fixed query set against the retrieval engine and reports
Hit@k / MRR / nDCG@k. It is the regression gate we run BEFORE and AFTER
swapping score_file() for BM25 (P4③): same query set, two reports, diff
them — the change's effect becomes a number instead of a vibe.

No external deps — stdlib only (mirrors build-hot.py).

Three result sources (a per-query ranked list of page_ids):

  --api URL        POST {URL}/api/v1/projects/{project}/search with
                   {query, bc, phase, topK, trace:true} and read
                   trace.candidates (ordered by finalRank). The canonical
                   engine — also doubles as the live end-to-end check.
                   Needs the dev_wiki app running.

  --log FILE       Replay wiki/_meta/retrieval-log.jsonl (what P4① writes):
                   for each query in the set, use the newest matching log
                   line's candidates. App-independent; "online" drift mode.

  --results FILE   A captured JSON map {query_id: [page_id, ...]}. Fully
                   offline + deterministic — the reproducible CI gate, and
                   what --selftest exercises.

Query set (JSON, see wiki-toolkit/eval/queries.example.json):
  {
    "k_values": [1, 3, 5, 10],
    "queries": [
      { "id": "q1", "query": "支付失败重试", "bc": "payment",
        "relevant": ["payment-retry-policy"],
        "graded": { "payment-retry-policy": 3, "payment-timeout": 1 } }
    ]
  }
  `relevant` = binary-relevant page_ids (file stems). `graded` is optional;
  when absent, every relevant page is treated as gain 1 for nDCG.

Gate:
  --baseline OLD.json   compare aggregate metrics against a prior report;
                        exit 2 if any metric regresses beyond --tolerance.
  --out NEW.json        write the fresh report (the next run's baseline).

Usage:
  eval-recall.py --queries q.json --results captured.json --out report.json
  eval-recall.py --queries q.json --log wiki/_meta/retrieval-log.jsonl
  eval-recall.py --queries q.json --api http://127.0.0.1:19828 \
                 --project current --token "$LLM_WIKI_API_TOKEN" --out r.json
  eval-recall.py --selftest
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from typing import Dict, List, Optional

DEFAULT_K = [1, 3, 5, 10]


# --------------------------------------------------------------------------
# Metric math — pure functions, the verifiable core (see --selftest).
# A "ranked" list is page_ids ordered best-first. `gains` maps page_id ->
# graded relevance gain (>0 for relevant pages, absent/0 for the rest).
# --------------------------------------------------------------------------

def hit_at_k(ranked: List[str], relevant: set, k: int) -> float:
    return 1.0 if any(p in relevant for p in ranked[:k]) else 0.0


def reciprocal_rank(ranked: List[str], relevant: set) -> float:
    for i, p in enumerate(ranked):
        if p in relevant:
            return 1.0 / (i + 1)
    return 0.0


def dcg_at_k(ranked: List[str], gains: Dict[str, float], k: int) -> float:
    total = 0.0
    for i, p in enumerate(ranked[:k]):
        g = gains.get(p, 0.0)
        if g:
            total += g / math.log2(i + 2)  # rank i is 1-based i+1 → log2(rank+1)
    return total


def ndcg_at_k(ranked: List[str], gains: Dict[str, float], k: int) -> float:
    ideal = sorted((g for g in gains.values() if g > 0), reverse=True)[:k]
    idcg = sum(g / math.log2(i + 2) for i, g in enumerate(ideal))
    if idcg == 0.0:
        return 0.0
    return dcg_at_k(ranked, gains, k) / idcg


def score_query(ranked: List[str], relevant: set, gains: Dict[str, float],
                k_values: List[int]) -> Dict[str, float]:
    out: Dict[str, float] = {"mrr": reciprocal_rank(ranked, relevant)}
    for k in k_values:
        out[f"hit@{k}"] = hit_at_k(ranked, relevant, k)
        out[f"ndcg@{k}"] = ndcg_at_k(ranked, gains, k)
    return out


def aggregate(per_query: List[Dict[str, float]]) -> Dict[str, float]:
    if not per_query:
        return {}
    keys = per_query[0].keys()
    return {key: sum(q[key] for q in per_query) / len(per_query) for key in keys}


# --------------------------------------------------------------------------
# Result sources — produce a ranked page_id list per query.
# --------------------------------------------------------------------------

def gains_for(query: dict) -> Dict[str, float]:
    """nDCG gains: every binary-relevant page gets gain 1, then `graded` refines
    those (and may add extra graded pages). Merging — rather than letting
    `graded` REPLACE `relevant` — ensures a relevant page the author forgot to
    grade still contributes to nDCG, so nDCG can't silently disagree with the
    Hit@k/MRR that score off the same `relevant` set."""
    gains: Dict[str, float] = {pid: 1.0 for pid in query.get("relevant", [])}
    graded = query.get("graded")
    if isinstance(graded, dict):
        for pid, g in graded.items():
            gains[pid] = float(g)
    return gains


def ranked_from_results(query: dict, results: dict) -> List[str]:
    val = results.get(query["id"])
    if val is None:
        val = results.get(query["query"])  # allow keying by query text too
    return list(val) if val else []


def ranked_from_log(query: dict, log_lines: List[dict]) -> List[str]:
    """Newest log entry whose query (and bc/phase, when the set pins them)
    matches; its candidates ordered by finalRank → page_ids."""
    want_bc = query.get("bc")
    want_phase = query.get("phase")
    best: Optional[dict] = None
    for entry in log_lines:
        if entry.get("query") != query["query"]:
            continue
        if want_bc is not None and entry.get("bc") != want_bc:
            continue
        if want_phase is not None and entry.get("phase") != want_phase:
            continue
        # Later lines are newer (append-only log); keep the last match.
        if best is None or entry.get("ts", 0) >= best.get("ts", 0):
            best = entry
    if not best:
        return []
    cands = sorted(best.get("candidates", []),
                   key=lambda c: c.get("finalRank", 1 << 30))
    return [c["pageId"] for c in cands if "pageId" in c]


def _ranked_from_trace_body(body: dict, qid: str) -> List[str]:
    """Extract the ranked page_ids from a /search response body.

    A *present* trace with zero candidates is a legitimate empty result.
    An *absent* or null trace means the target build did not honor
    `trace:true` — that's a misconfiguration, not a 0-score query, so we
    fail loud rather than let the gate report a phantom regression."""
    if body.get("trace") is None:
        raise SystemExit(
            f"eval-recall: query {qid!r} got a response with no trace — the "
            "target build does not honor trace:true, so --api cannot evaluate "
            "it (every query would score 0 and the gate would cry regression).")
    cands = sorted(body["trace"].get("candidates", []),
                   key=lambda c: c.get("finalRank", 1 << 30))
    return [c["pageId"] for c in cands if "pageId" in c]


def ranked_from_api(query: dict, base_url: str, project: str,
                    token: Optional[str], top_k: int) -> List[str]:
    import urllib.error
    import urllib.request

    payload = {"query": query["query"], "topK": top_k, "trace": True}
    if query.get("bc"):
        payload["bc"] = query["bc"]
    if query.get("phase"):
        payload["phase"] = query["phase"]
    url = f"{base_url.rstrip('/')}/api/v1/projects/{project}/search"
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=data, method="POST")
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    # --api targets an explicit base_url (usually localhost). Bypass any
    # HTTP(S)_PROXY in the environment — otherwise a proxied shell routes the
    # localhost request through the proxy and gets URLError/502, the same trap
    # the wiki-* skills hit and fixed with `curl --noproxy '*'`.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    qid = query.get("id", query.get("query", "?"))
    # Turn transport/parse failures into a clean exit-1 message rather than a
    # raw traceback (an operational error, not a regression).
    try:
        with opener.open(req, timeout=30) as resp:
            body = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        raise SystemExit(f"eval-recall: query {qid!r} → API HTTP {e.code} at {url}: {e.reason}")
    except urllib.error.URLError as e:
        raise SystemExit(f"eval-recall: query {qid!r} → cannot reach API at {url}: {e.reason} "
                         "(is dev_wiki running? is the proxy bypassed?)")
    except json.JSONDecodeError as e:
        raise SystemExit(f"eval-recall: query {qid!r} → API returned non-JSON: {e}")
    return _ranked_from_trace_body(body, qid)


# --------------------------------------------------------------------------
# Report + regression gate.
# --------------------------------------------------------------------------

def run_eval(query_set: dict, source) -> dict:
    k_values = query_set.get("k_values") or DEFAULT_K
    kmax = max(k_values)
    per_query = []
    shorted = []  # queries whose ranked list is shorter than kmax
    for q in query_set["queries"]:
        ranked = source(q, kmax)
        relevant = set(q.get("relevant", []))
        scores = score_query(ranked, relevant, gains_for(q), k_values)
        per_query.append({"id": q["id"], **scores, "n_ranked": len(ranked)})
        if len(ranked) < kmax:
            shorted.append((q["id"], len(ranked)))
    # No silent caps: a ranked list shorter than kmax (e.g. a --log/--results
    # capture truncated to a smaller top_k than the eval's k) scores Hit@k/nDCG@k
    # over a too-short list and understates recall. Surface it rather than let it
    # read as full coverage.
    if shorted:
        shown = ", ".join(f"{qid}({n})" for qid, n in shorted[:8])
        more = "" if len(shorted) <= 8 else f" +{len(shorted) - 8} more"
        print(f"eval-recall: WARNING {len(shorted)} query(ies) returned fewer than "
              f"k={kmax} results; metrics at large k are computed over a short list "
              f"and may understate recall: {shown}{more}", file=sys.stderr)
    metric_rows = [{kk: vv for kk, vv in q.items() if kk not in ("id", "n_ranked")}
                   for q in per_query]
    return {
        "n_queries": len(per_query),
        "k_values": k_values,
        "metrics": aggregate(metric_rows),
        "per_query": per_query,
    }


def compare(baseline: dict, report: dict, tolerance: float) -> List[str]:
    regressions = []
    base_m = baseline.get("metrics", {})
    for key, new_val in report.get("metrics", {}).items():
        old_val = base_m.get(key)
        if old_val is None:
            continue
        if new_val < old_val - tolerance:
            regressions.append(f"{key}: {old_val:.4f} → {new_val:.4f} "
                               f"(−{old_val - new_val:.4f})")
    return regressions


def fmt_metrics(metrics: Dict[str, float]) -> str:
    order = ["mrr"] + [k for k in metrics if k != "mrr"]
    return "  ".join(f"{k}={metrics[k]:.4f}" for k in order if k in metrics)


# --------------------------------------------------------------------------
# Self-test — exercises the metric math on hand-checked cases.
# --------------------------------------------------------------------------

def selftest() -> int:
    relevant = {"a", "c"}
    ranked = ["a", "b", "c", "d"]  # relevant at ranks 1 and 3
    assert hit_at_k(ranked, relevant, 1) == 1.0
    assert hit_at_k(["b", "x"], relevant, 1) == 0.0
    assert hit_at_k(["b", "x"], relevant, 3) == 0.0
    assert hit_at_k(["b", "a"], relevant, 2) == 1.0
    assert reciprocal_rank(ranked, relevant) == 1.0
    assert reciprocal_rank(["b", "c", "a"], relevant) == 0.5
    assert reciprocal_rank(["x", "y"], relevant) == 0.0

    # nDCG: ideal order is [a(3), c(1)]; the candidate ranks c above a.
    gains = {"a": 3.0, "c": 1.0}
    idcg = 3.0 / math.log2(2) + 1.0 / math.log2(3)          # a@1, c@2
    dcg = 1.0 / math.log2(2) + 3.0 / math.log2(3)           # c@1, a@2
    got = ndcg_at_k(["c", "a"], gains, 5)
    assert abs(got - dcg / idcg) < 1e-9, got
    # Perfect ranking → nDCG 1.0
    assert abs(ndcg_at_k(["a", "c"], gains, 5) - 1.0) < 1e-9
    # No relevant gains → nDCG 0.0 (no division blow-up)
    assert ndcg_at_k(["x"], {}, 5) == 0.0

    # Binary fallback: graded absent → every relevant page gains 1.
    q = {"relevant": ["a", "c"]}
    assert gains_for(q) == {"a": 1.0, "c": 1.0}
    q2 = {"relevant": ["a"], "graded": {"a": 2, "b": 1}}
    assert gains_for(q2) == {"a": 2.0, "b": 1.0}
    # graded REFINES relevant, it doesn't replace it: a relevant page the author
    # forgot to grade keeps gain 1 (so nDCG still credits it, matching Hit@k).
    q3 = {"relevant": ["a", "c"], "graded": {"a": 3}}
    assert gains_for(q3) == {"a": 3.0, "c": 1.0}, gains_for(q3)

    # Aggregate macro-averages across queries.
    agg = aggregate([{"hit@1": 1.0, "mrr": 1.0}, {"hit@1": 0.0, "mrr": 0.5}])
    assert agg == {"hit@1": 0.5, "mrr": 0.75}, agg

    # Log replay picks newest matching line and orders by finalRank.
    log = [
        {"query": "q", "ts": 1, "candidates": [{"pageId": "old", "finalRank": 1}]},
        {"query": "q", "ts": 2, "candidates": [
            {"pageId": "b", "finalRank": 2}, {"pageId": "a", "finalRank": 1}]},
        {"query": "other", "ts": 3, "candidates": [{"pageId": "z", "finalRank": 1}]},
    ]
    assert ranked_from_log({"query": "q"}, log) == ["a", "b"]
    assert ranked_from_log({"query": "missing"}, log) == []
    # bc pin filters log lines.
    log_bc = [
        {"query": "q", "bc": "pay", "ts": 1, "candidates": [{"pageId": "p", "finalRank": 1}]},
        {"query": "q", "bc": "ship", "ts": 2, "candidates": [{"pageId": "s", "finalRank": 1}]},
    ]
    assert ranked_from_log({"query": "q", "bc": "pay"}, log_bc) == ["p"]

    # API trace parsing: a present-but-empty trace is a legit empty result;
    # an absent/null trace is a misconfig → loud failure, not a 0-score query.
    assert _ranked_from_trace_body(
        {"trace": {"candidates": [
            {"pageId": "b", "finalRank": 2}, {"pageId": "a", "finalRank": 1}]}},
        "q") == ["a", "b"]
    assert _ranked_from_trace_body({"trace": {"candidates": []}}, "q") == []
    for bad in ({}, {"trace": None}):
        try:
            _ranked_from_trace_body(bad, "q")
            assert False, "expected SystemExit on missing/null trace"
        except SystemExit:
            pass

    # End-to-end on a tiny set via the results source.
    qset = {"k_values": [1, 3], "queries": [
        {"id": "q1", "query": "q1", "relevant": ["a"]},
        {"id": "q2", "query": "q2", "relevant": ["z"]},
    ]}
    results = {"q1": ["a", "b", "x"], "q2": ["b", "c", "y"]}  # ≥ max(k) so no short-list warning
    rep = run_eval(qset, lambda q, k: ranked_from_results(q, results))
    assert rep["metrics"]["hit@1"] == 0.5   # q1 hits, q2 misses
    assert rep["metrics"]["mrr"] == 0.5     # q1 rr=1, q2 rr=0

    # Regression gate detects a drop, ignores an improvement.
    base = {"metrics": {"hit@1": 0.5, "mrr": 0.5}}
    worse = {"metrics": {"hit@1": 0.4, "mrr": 0.6}}
    regs = compare(base, worse, 1e-9)
    assert len(regs) == 1 and regs[0].startswith("hit@1"), regs
    assert compare(base, {"metrics": {"hit@1": 0.5, "mrr": 0.9}}, 1e-9) == []

    print("eval-recall selftest: OK")
    return 0


# --------------------------------------------------------------------------
# CLI.
# --------------------------------------------------------------------------

def load_json(path: str, what: str):
    """Load a JSON file, turning IO/parse failures into a clean exit-1 message
    (an operational error) instead of a raw traceback — kept distinct from the
    regression exit 2 a gate wrapper keys on."""
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except FileNotFoundError:
        raise SystemExit(f"eval-recall: {what} not found: {path!r}")
    except (OSError, json.JSONDecodeError) as e:
        raise SystemExit(f"eval-recall: cannot read {what} {path!r}: {e}")


def load_jsonl(path: str, what: str) -> List[dict]:
    try:
        with open(path, encoding="utf-8") as f:
            lines = [ln for ln in f if ln.strip()]
    except FileNotFoundError:
        raise SystemExit(f"eval-recall: {what} not found: {path!r}")
    except OSError as e:
        raise SystemExit(f"eval-recall: cannot read {what} {path!r}: {e}")
    out: List[dict] = []
    for i, ln in enumerate(lines, 1):
        try:
            out.append(json.loads(ln))
        except json.JSONDecodeError as e:
            raise SystemExit(f"eval-recall: {what} {path!r} line {i} is not valid JSON: {e}")
    return out


def build_source(args):
    if args.results:
        results = load_json(args.results, "--results file")
        return lambda q, k: ranked_from_results(q, results)
    if args.log:
        log_lines = load_jsonl(args.log, "--log file")
        return lambda q, k: ranked_from_log(q, log_lines)
    if args.api:
        token = args.token or os.environ.get("LLM_WIKI_API_TOKEN")
        return lambda q, k: ranked_from_api(q, args.api, args.project, token, k)
    raise SystemExit("eval-recall: need one result source: --results | --log | --api")


def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser(description="dev_wiki retrieval-quality eval (P4②)")
    ap.add_argument("--queries", help="query-set JSON")
    ap.add_argument("--results", help="captured {query_id: [page_id,...]} JSON")
    ap.add_argument("--log", help="retrieval-log.jsonl to replay")
    ap.add_argument("--api", help="base URL of a running dev_wiki, e.g. http://127.0.0.1:19828")
    ap.add_argument("--project", default="current", help="project id for --api (default: current)")
    ap.add_argument("--token", default=None, help="bearer token for --api (or $LLM_WIKI_API_TOKEN)")
    ap.add_argument("--baseline", default=None, help="prior report JSON to gate against")
    ap.add_argument("--out", default=None, help="write the fresh report JSON here")
    ap.add_argument("--tolerance", type=float, default=1e-9, help="allowed metric drop before failing")
    ap.add_argument("--selftest", action="store_true", help="run metric self-tests and exit")
    args = ap.parse_args(argv)

    if args.selftest:
        return selftest()
    if not args.queries:
        ap.error("--queries is required (unless --selftest)")

    query_set = load_json(args.queries, "--queries file")
    # An empty/missing query list must be a hard error, not a silent pass:
    # aggregate({}) → no metrics → compare() finds no regressions → exit 0,
    # i.e. a green gate that evaluated nothing and would wave a real drop through.
    queries = query_set.get("queries")
    if not isinstance(queries, list) or not queries:
        print("eval-recall: query set has no queries — nothing to evaluate "
              "(refusing to report a green gate)", file=sys.stderr)
        return 1
    source = build_source(args)
    report = run_eval(query_set, source)

    print(f"queries: {report['n_queries']}   {fmt_metrics(report['metrics'])}", file=sys.stderr)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump(report, f, ensure_ascii=False, indent=2)
        print(f"eval-recall: wrote report → {args.out}", file=sys.stderr)

    if args.baseline:
        baseline = load_json(args.baseline, "--baseline file")
        regs = compare(baseline, report, args.tolerance)
        if regs:
            print("eval-recall: REGRESSION vs baseline:", file=sys.stderr)
            for r in regs:
                print(f"  - {r}", file=sys.stderr)
            return 2
        print("eval-recall: no regression vs baseline ✓", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
