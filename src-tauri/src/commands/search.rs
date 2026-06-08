use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use walkdir::WalkDir;

use crate::commands::vectorstore;
use crate::panic_guard::run_guarded_async;

const DEFAULT_RESULTS: usize = 20;
const MAX_RESULTS: usize = 50;
const RRF_K: f64 = 60.0;
const FILENAME_EXACT_BONUS: f64 = 200.0;
const PHRASE_IN_TITLE_BONUS: f64 = 50.0;
const PHRASE_IN_CONTENT_PER_OCC: f64 = 20.0;
const MAX_PHRASE_OCC_COUNTED: usize = 10;
// DEVWIKI (P4③): Okapi BM25 parameters. k1 controls term-frequency
// saturation, b controls length normalization (the standard defaults).
const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;
// DEVWIKI (P4③): field boosts folded into the BM25 term frequency. The
// document body counts a term once (weight 1); occurrences in the title and
// `summary` frontmatter add extra weight on top, so a title term effectively
// counts 3× and a summary term 2× (the plan's "summary 2× boost").
const BM25_TITLE_EXTRA_WEIGHT: f64 = 2.0;
const BM25_SUMMARY_EXTRA_WEIGHT: f64 = 1.0;
const SNIPPET_CONTEXT: usize = 80;
const SEARCH_EMBEDDING_TIMEOUT_SECS: u64 = 8;
const MAX_SEARCH_FILES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchImageRef {
    pub url: String,
    pub alt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchResult {
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub title_match: bool,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_score: Option<f32>,
    pub images: Vec<SearchImageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchResponse {
    pub mode: String,
    pub results: Vec<ProjectSearchResult>,
    pub token_hits: usize,
    pub vector_hits: usize,
    // DEVWIKI (P4①): structured retrieval trace, populated only when the
    // caller opts in (`with_trace`). Omitted from the response otherwise so
    // existing clients/tests are unaffected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<RetrievalTrace>,
}

// DEVWIKI (P4①): retrieval-path logging. A trace makes "why did this page
// rank here" reconstructable — per-candidate keyword/vector scores and ranks,
// plus the RRF score and final position. It is the instrument we read BEFORE
// swapping score_file() for BM25 in P4③, so the change's effect is measurable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceCandidate {
    /// File stem — the id vector results are keyed by.
    pub page_id: String,
    /// Project-relative path (e.g. `wiki/concept/foo.md`).
    pub path: String,
    /// Raw keyword score from `score_file` (None ⇒ vector-only candidate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_score: Option<f64>,
    /// 1-based rank in the keyword-only ordering (None ⇒ no keyword hit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_rank: Option<usize>,
    /// Cosine similarity from LanceDB (None ⇒ keyword-only candidate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_score: Option<f32>,
    /// 1-based rank in the vector-only ordering (None ⇒ no vector hit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_rank: Option<usize>,
    /// Final fused score (RRF in hybrid mode; raw keyword score otherwise).
    pub rrf_score: f64,
    /// 1-based position in the returned, truncated result list.
    pub final_rank: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalTrace {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// "keyword" | "vector" | "hybrid" — mirrors the response mode.
    pub mode: String,
    /// Retrieval paths attempted this query, e.g. ["keyword","vector"].
    pub paths_run: Vec<String>,
    pub top_k: usize,
    pub token_hits: usize,
    pub vector_hits: usize,
    pub candidates: Vec<TraceCandidate>,
    /// Unix epoch milliseconds when the trace was built.
    pub ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEmbeddingConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub output_dimensionality: Option<u32>,
    /// Extra HTTP headers to send with every embedding request, e.g.
    /// `X-Model-Provider-Id: siliconflow` for the mify gateway.
    /// Reserved names (Authorization, Content-Type, Host,
    /// Content-Length, x-goog-api-key) are skipped — they're managed
    /// by the client.
    #[serde(default)]
    pub extra_headers: Option<BTreeMap<String, String>>,
}

#[tauri::command]
pub async fn search_project(
    project_path: String,
    query: String,
    top_k: Option<usize>,
    include_content: Option<bool>,
    query_embedding: Option<Vec<f32>>,
    embedding_config: Option<SearchEmbeddingConfig>,
    // DEVWIKI: optional bounded-context filter; None preserves upstream behavior
    bc: Option<String>,
    // DEVWIKI (P4①): optional retrieval-trace switch + SDLC phase passthrough.
    with_trace: Option<bool>,
    phase: Option<String>,
) -> Result<ProjectSearchResponse, String> {
    run_guarded_async("search_project", async move {
        let query_embedding =
            resolve_query_embedding(&query, query_embedding, embedding_config).await?;
        search_project_inner(
            project_path,
            query,
            top_k.unwrap_or(DEFAULT_RESULTS),
            include_content.unwrap_or(false),
            query_embedding,
            bc, // DEVWIKI: pass bc filter through
            with_trace.unwrap_or(false),
            phase,
        )
        .await
    })
    .await
}

pub async fn resolve_query_embedding(
    query: &str,
    explicit_embedding: Option<Vec<f32>>,
    embedding_config: Option<SearchEmbeddingConfig>,
) -> Result<Option<Vec<f32>>, String> {
    if let Some(embedding) = explicit_embedding {
        return validate_query_embedding(embedding).map(Some);
    }
    let Some(cfg) = embedding_config else {
        return Ok(None);
    };
    if !cfg.enabled || cfg.endpoint.trim().is_empty() || cfg.model.trim().is_empty() {
        return Ok(None);
    }
    match fetch_embedding(query, &cfg).await {
        Ok(embedding) => validate_query_embedding(embedding).map(Some),
        Err(err) => {
            eprintln!("[Search] embedding disabled for this request: {err}");
            Ok(None)
        }
    }
}

fn validate_query_embedding(embedding: Vec<f32>) -> Result<Vec<f32>, String> {
    if embedding.is_empty() {
        return Err("queryEmbedding must not be empty".to_string());
    }
    if embedding.iter().any(|v| !v.is_finite()) {
        return Err("queryEmbedding must contain only finite numbers".to_string());
    }
    Ok(embedding)
}

pub async fn search_project_inner(
    project_path: String,
    query: String,
    top_k: usize,
    include_content: bool,
    query_embedding: Option<Vec<f32>>,
    // DEVWIKI: optional bounded-context filter; None preserves upstream behavior
    bc_filter: Option<String>,
    // DEVWIKI (P4①): when true, attach a structured RetrievalTrace to the
    // response. `phase` is recorded verbatim in the trace for forward-compat
    // with P4③ SDLC-phase weighting; it does not affect ranking yet.
    with_trace: bool,
    phase: Option<String>,
) -> Result<ProjectSearchResponse, String> {
    if query.trim().is_empty() {
        return Err("query is required".to_string());
    }
    // DEVWIKI (P4①): remember whether a vector path was attempted, before the
    // embedding is moved into the search below.
    let vector_attempted = query_embedding.as_ref().is_some_and(|e| !e.is_empty());
    // DEVWIKI: normalize bc filter once (case-insensitive, trimmed)
    let bc_filter = bc_filter
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let limit = top_k.clamp(1, MAX_RESULTS);
    let tokens = tokenize_query(&query);
    let effective_tokens = if tokens.is_empty() {
        vec![query.trim().to_lowercase()]
    } else {
        tokens
    };
    let query_phrase = trim_query_punctuation(&query.to_lowercase());
    let mut page_paths_by_stem = BTreeMap::new();

    // DEVWIKI (P4③): BM25 needs corpus statistics (N, document frequency per
    // term, average document length) that no single file knows, so the keyword
    // pass is now two phases. Phase 1 walks the (bc-eligible) corpus collecting
    // each candidate's weighted term frequencies + length and the global stats;
    // phase 2 computes IDF and scores. When a bc filter is set, "the corpus" is
    // that bounded context's pages only — IDF/avgdl are scoped to it.
    let mut collected: Vec<DocCollect> = Vec::new();
    let mut doc_freq: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_doc_len = 0.0_f64;
    let mut doc_count = 0_usize;

    let wiki_root = Path::new(&project_path).join("wiki");
    if wiki_root.exists() {
        let mut searched_files = 0usize;
        for entry in WalkDir::new(&wiki_root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file()
                || entry.path().extension().and_then(|s| s.to_str()) != Some("md")
            {
                continue;
            }
            searched_files += 1;
            if searched_files > MAX_SEARCH_FILES {
                eprintln!(
                    "[Search] stopped scanning wiki after {MAX_SEARCH_FILES} markdown files in {project_path}"
                );
                break;
            }
            let content = match fs::read_to_string(entry.path()) {
                Ok(content) => content,
                Err(_) => continue,
            };
            // DEVWIKI: skip pages whose frontmatter bc does not match the filter.
            // Skipping before page_paths_by_stem insertion also keeps vector-only
            // results out of scope, so the filter applies to keyword + vector paths.
            if let Some(ref wanted_bc) = bc_filter {
                let page_bc = frontmatter_field(&content, "bc").map(|s| s.to_lowercase());
                if page_bc.as_deref() != Some(wanted_bc.as_str()) {
                    continue;
                }
            }
            if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                let previous = page_paths_by_stem.insert(
                    stem.to_string(),
                    relative_to_project(&project_path, entry.path()),
                );
                if let Some(previous) = previous {
                    eprintln!(
                        "[Search] duplicate wiki page stem '{stem}': '{previous}' and '{}' share one vector page_id",
                        relative_to_project(&project_path, entry.path())
                    );
                }
            }
            // Every bc-eligible doc counts toward N and avgdl, even non-matches.
            let (doc_len, candidate) = collect_doc(
                &project_path,
                entry.path(),
                &content,
                &effective_tokens,
                &query_phrase,
                &query,
                include_content,
            );
            total_doc_len += doc_len;
            doc_count += 1;
            if let Some(candidate) = candidate {
                // df only comes from candidates (a term-bearing doc is one).
                for term in candidate.weighted_tf.keys() {
                    *doc_freq.entry(term.clone()).or_insert(0) += 1;
                }
                collected.push(candidate);
            }
        }
    }

    // Phase 2: IDF from corpus df, then BM25 + exact-match bonuses per candidate.
    let n_docs = doc_count as f64;
    let avg_doc_len = if doc_count > 0 {
        total_doc_len / n_docs
    } else {
        1.0
    };
    let idf: BTreeMap<String, f64> = doc_freq
        .iter()
        .map(|(term, &df)| {
            let df = df as f64;
            // Floored IDF: ln(1 + (N - df + 0.5)/(df + 0.5)) is always ≥ 0.
            (term.clone(), (1.0 + (n_docs - df + 0.5) / (df + 0.5)).ln())
        })
        .collect();

    let mut results: Vec<ProjectSearchResult> = collected
        .into_iter()
        .map(|c| {
            let bm25 = bm25_score(&c.weighted_tf, c.doc_len, avg_doc_len, &idf);
            // Exact-match signals BM25's bag of words misses stay additive.
            let score = bm25
                + if c.filename_exact { FILENAME_EXACT_BONUS } else { 0.0 }
                + if c.title_has_phrase { PHRASE_IN_TITLE_BONUS } else { 0.0 }
                + c.content_phrase_occ as f64 * PHRASE_IN_CONTENT_PER_OCC;
            ProjectSearchResult {
                path: c.path,
                title: c.title,
                snippet: c.snippet,
                title_match: c.title_token_hit || c.title_has_phrase,
                score,
                vector_score: None,
                images: c.images,
                content: c.content,
            }
        })
        .collect();

    let mut token_sorted = (0..results.len()).collect::<Vec<_>>();
    token_sorted.sort_by(|a, b| {
        results[*b]
            .score
            .partial_cmp(&results[*a].score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| results[*a].path.cmp(&results[*b].path))
    });
    let mut token_rank = BTreeMap::new();
    for (idx, result_idx) in token_sorted.iter().enumerate() {
        let result = &results[*result_idx];
        token_rank.insert(normalize_path(&result.path), idx + 1);
    }

    // DEVWIKI (P4①): snapshot the raw keyword scores now — `apply_rrf_scores`
    // overwrites `result.score` with the fused value, and vector-only results
    // (materialized below) carry a placeholder 0.0 that must not look like a
    // keyword score. At this point `results` holds keyword hits only.
    let keyword_scores: BTreeMap<String, f64> = if with_trace {
        results
            .iter()
            .map(|r| (normalize_path(&r.path), r.score))
            .collect()
    } else {
        BTreeMap::new()
    };

    let mut vector_rank: BTreeMap<String, usize> = BTreeMap::new();
    let mut vector_score: BTreeMap<String, f32> = BTreeMap::new();
    let mut vector_hits = 0;
    if let Some(embedding) = query_embedding {
        if !embedding.is_empty() {
            match search_by_embedding(&project_path, embedding, limit.max(10)).await {
                Ok(vector_results) => {
                    vector_hits = vector_results.len();
                    for (idx, vr) in vector_results.iter().enumerate() {
                        vector_rank.insert(vr.id.clone(), idx + 1);
                        vector_score.insert(vr.id.clone(), vr.score);
                    }
                    materialize_vector_only_results(
                        &vector_results,
                        &page_paths_by_stem,
                        &project_path,
                        &mut results,
                        include_content,
                    );
                }
                Err(err) => {
                    eprintln!(
                        "[Search] vector search failed; falling back to keyword results: {err}"
                    );
                }
            }
        }
    }

    if vector_hits == 0 {
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        results.truncate(limit);
        let trace = with_trace.then(|| {
            build_retrieval_trace(
                &results,
                &token_rank,
                &vector_rank,
                &keyword_scores,
                &query,
                &bc_filter,
                &phase,
                "keyword",
                vector_attempted,
                limit,
                token_rank.len(),
                vector_hits,
            )
        });
        return Ok(ProjectSearchResponse {
            mode: "keyword".to_string(),
            token_hits: token_rank.len(),
            vector_hits,
            results,
            trace,
        });
    }

    apply_rrf_scores(&mut results, &token_rank, &vector_rank, &vector_score);

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    results.truncate(limit);

    let mode = search_mode(token_rank.is_empty(), vector_hits).to_string();
    let trace = with_trace.then(|| {
        build_retrieval_trace(
            &results,
            &token_rank,
            &vector_rank,
            &keyword_scores,
            &query,
            &bc_filter,
            &phase,
            &mode,
            vector_attempted,
            limit,
            token_rank.len(),
            vector_hits,
        )
    });
    Ok(ProjectSearchResponse {
        mode,
        token_hits: token_rank.len(),
        vector_hits,
        results,
        trace,
    })
}

fn apply_rrf_scores(
    results: &mut [ProjectSearchResult],
    token_rank: &BTreeMap<String, usize>,
    vector_rank: &BTreeMap<String, usize>,
    vector_score: &BTreeMap<String, f32>,
) {
    for result in results {
        let token = token_rank.get(&normalize_path(&result.path)).copied();
        let vector = vector_rank.get(&file_stem(&result.path)).copied();
        let mut rrf = 0.0;
        if let Some(rank) = token {
            rrf += 1.0 / (RRF_K + rank as f64);
        }
        if let Some(rank) = vector {
            rrf += 1.0 / (RRF_K + rank as f64);
        }
        if let Some(score) = vector_score.get(&file_stem(&result.path)).copied() {
            result.vector_score = Some(score);
        }
        result.score = rrf;
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// DEVWIKI (P4①): assemble a per-candidate retrieval trace from the final,
// truncated result list and the rank/score lookups gathered during search.
// `keyword_scores` holds the raw `score_file` scores captured *before*
// `apply_rrf_scores` overwrote `result.score` with the fused RRF value.
#[allow(clippy::too_many_arguments)]
fn build_retrieval_trace(
    final_results: &[ProjectSearchResult],
    token_rank: &BTreeMap<String, usize>,
    vector_rank: &BTreeMap<String, usize>,
    keyword_scores: &BTreeMap<String, f64>,
    query: &str,
    bc: &Option<String>,
    phase: &Option<String>,
    mode: &str,
    vector_attempted: bool,
    top_k: usize,
    token_hits: usize,
    vector_hits: usize,
) -> RetrievalTrace {
    let mut paths_run = vec!["keyword".to_string()];
    if vector_attempted {
        paths_run.push("vector".to_string());
    }
    let candidates = final_results
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let np = normalize_path(&r.path);
            let stem = file_stem(&r.path);
            let keyword_rank = token_rank.get(&np).copied();
            TraceCandidate {
                page_id: stem.clone(),
                path: r.path.clone(),
                // Only report a keyword score when the page actually hit on
                // keywords; vector-only candidates have a placeholder 0.0 score.
                keyword_score: keyword_rank.and(keyword_scores.get(&np).copied()),
                keyword_rank,
                vector_score: r.vector_score,
                vector_rank: vector_rank.get(&stem).copied(),
                rrf_score: r.score,
                final_rank: idx + 1,
            }
        })
        .collect();
    RetrievalTrace {
        query: query.to_string(),
        bc: bc.clone(),
        phase: phase.clone(),
        mode: mode.to_string(),
        paths_run,
        top_k,
        token_hits,
        vector_hits,
        candidates,
        ts: now_millis(),
    }
}

fn search_mode(token_rank_empty: bool, vector_hits: usize) -> &'static str {
    if vector_hits == 0 {
        "keyword"
    } else if token_rank_empty {
        "vector"
    } else {
        "hybrid"
    }
}

#[derive(Debug, Clone)]
struct PageVectorResult {
    id: String,
    score: f32,
    chunk_text: String,
    heading_path: String,
}

async fn search_by_embedding(
    project_path: &str,
    query_embedding: Vec<f32>,
    top_k: usize,
) -> Result<Vec<PageVectorResult>, String> {
    let raw_chunks = vectorstore::vector_search_chunks(
        project_path.to_string(),
        query_embedding,
        (top_k * 3).max(30),
    )
    .await?;
    if raw_chunks.is_empty() {
        return Ok(vec![]);
    }

    let mut by_page: BTreeMap<String, Vec<vectorstore::ChunkSearchResult>> = BTreeMap::new();
    for chunk in raw_chunks {
        by_page
            .entry(chunk.page_id.clone())
            .or_default()
            .push(chunk);
    }

    let mut ranked = Vec::new();
    for (id, mut chunks) in by_page {
        chunks.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.chunk_index.cmp(&b.chunk_index))
        });
        let top_chunk = chunks[0].clone();
        let top = top_chunk.score;
        let tail: f32 = chunks.iter().skip(1).map(|chunk| chunk.score).sum();
        let blended = top + (tail * 0.3).min((1.0 - top).max(0.0));
        ranked.push(PageVectorResult {
            id,
            score: blended,
            chunk_text: top_chunk.chunk_text,
            heading_path: top_chunk.heading_path,
        });
    }
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    ranked.truncate(top_k);
    Ok(ranked)
}

fn materialize_vector_only_results(
    vector_results: &[PageVectorResult],
    page_paths_by_stem: &BTreeMap<String, String>,
    project_path: &str,
    results: &mut Vec<ProjectSearchResult>,
    include_content: bool,
) {
    let mut known: BTreeSet<String> = results.iter().map(|r| file_stem(&r.path)).collect();
    for vr in vector_results {
        if known.contains(&vr.id) {
            continue;
        }
        if let Some(rel) = page_paths_by_stem.get(&vr.id) {
            let path = Path::new(project_path).join(rel);
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let file_name = Path::new(&rel)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let title = extract_title(&content, file_name);
            let snippet = build_vector_snippet(vr);
            results.push(ProjectSearchResult {
                path: rel.clone(),
                title,
                snippet,
                title_match: false,
                score: 0.0,
                vector_score: Some(vr.score),
                images: extract_image_refs(&content),
                content: include_content.then_some(content),
            });
            known.insert(vr.id.clone());
        }
    }
}

fn build_vector_snippet(result: &PageVectorResult) -> String {
    let mut text = result.chunk_text.trim().replace('\n', " ");
    if text.is_empty() {
        return String::new();
    }
    if text.chars().count() > SNIPPET_CONTEXT * 2 {
        text = text.chars().take(SNIPPET_CONTEXT * 2).collect::<String>();
        text.push_str("...");
    }
    let heading = result.heading_path.trim();
    if heading.is_empty() {
        text
    } else {
        format!("{heading}: {text}")
    }
}

// DEVWIKI (P4③): a keyword candidate gathered in phase 1 of the BM25 pass.
// Holds everything needed to score (weighted term frequencies + length) and to
// build the result (title/snippet/images), so phase 2 needs no second file read.
struct DocCollect {
    path: String,
    title: String,
    snippet: String,
    images: Vec<SearchImageRef>,
    content: Option<String>,
    weighted_tf: BTreeMap<String, f64>,
    doc_len: f64,
    filename_exact: bool,
    title_has_phrase: bool,
    title_token_hit: bool,
    content_phrase_occ: usize,
}

// DEVWIKI (P4③): approximate document length in "terms" for BM25 length
// normalization — each CJK character is one term, each run of Latin
// letters/digits is one term. Cheap (one pass), and only needs to be
// consistent across documents, not linguistically exact.
fn approx_doc_length(text: &str) -> f64 {
    let mut len = 0usize;
    let mut in_word = false;
    for c in text.chars() {
        if ('\u{3400}'..='\u{9fff}').contains(&c) {
            len += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                len += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    len as f64
}

// DEVWIKI (P4③): Okapi BM25 over field-weighted term frequencies.
//   score = Σ_t IDF(t) · tf~(t)·(k1+1) / (tf~(t) + k1·(1 − b + b·dl/avgdl))
// where tf~ already folds in the title/summary field boosts. Terms with
// non-positive IDF (present in (nearly) every doc) contribute nothing.
fn bm25_score(
    weighted_tf: &BTreeMap<String, f64>,
    doc_len: f64,
    avg_doc_len: f64,
    idf: &BTreeMap<String, f64>,
) -> f64 {
    let avg_doc_len = avg_doc_len.max(1.0);
    let norm = 1.0 - BM25_B + BM25_B * (doc_len / avg_doc_len);
    let mut score = 0.0;
    for (term, &tf) in weighted_tf {
        let idf_t = idf.get(term).copied().unwrap_or(0.0);
        if idf_t <= 0.0 || tf <= 0.0 {
            continue;
        }
        score += idf_t * (tf * (BM25_K1 + 1.0)) / (tf + BM25_K1 * norm);
    }
    score
}

// DEVWIKI (P4③): phase 1 of the keyword pass — detect a candidate and gather
// its weighted term frequencies + length. Returns the document length for every
// doc (so non-matches still feed N/avgdl) plus a candidate when there is any
// keyword signal (term hit, exact filename, or phrase match).
fn collect_doc(
    project_path: &str,
    path: &Path,
    content: &str,
    tokens: &[String],
    query_phrase: &str,
    query: &str,
    include_content: bool,
) -> (f64, Option<DocCollect>) {
    let doc_len = approx_doc_length(content);
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let title = extract_title(content, file_name);
    let title_text = format!("{title} {file_name}");
    let title_lower = title_text.to_lowercase();
    let content_lower = content.to_lowercase();
    let summary_lower = frontmatter_field(content, "summary")
        .unwrap_or_default()
        .to_lowercase();
    let stem = file_name.trim_end_matches(".md").to_lowercase();

    let filename_exact = !query_phrase.is_empty() && stem == query_phrase;
    let title_has_phrase = !query_phrase.is_empty() && title_lower.contains(query_phrase);
    let content_phrase_occ =
        count_occurrences(&content_lower, query_phrase).min(MAX_PHRASE_OCC_COUNTED);

    // Field-weighted term frequency: the body counts a term once; title and
    // summary occurrences add extra weight (effective title 3×, summary 2×).
    let mut weighted_tf: BTreeMap<String, f64> = BTreeMap::new();
    let mut title_token_hit = false;
    for token in tokens {
        let body_occ = count_occurrences(&content_lower, token);
        let title_occ = count_occurrences(&title_lower, token);
        let summary_occ = count_occurrences(&summary_lower, token);
        if title_occ > 0 {
            title_token_hit = true;
        }
        let tf = body_occ as f64
            + BM25_TITLE_EXTRA_WEIGHT * title_occ as f64
            + BM25_SUMMARY_EXTRA_WEIGHT * summary_occ as f64;
        if tf > 0.0 {
            weighted_tf.insert(token.clone(), tf);
        }
    }

    let has_signal =
        filename_exact || title_has_phrase || content_phrase_occ > 0 || !weighted_tf.is_empty();
    if !has_signal {
        return (doc_len, None);
    }

    let snippet_anchor = if content_phrase_occ > 0 {
        query_phrase.to_string()
    } else {
        tokens
            .iter()
            .find(|token| content_lower.contains(token.as_str()))
            .cloned()
            .unwrap_or_else(|| query.to_string())
    };

    let candidate = DocCollect {
        path: relative_to_project(project_path, path),
        title,
        snippet: build_snippet(content, &snippet_anchor),
        images: extract_image_refs(content),
        content: include_content.then(|| content.to_string()),
        weighted_tf,
        doc_len,
        filename_exact,
        title_has_phrase,
        title_token_hit,
        content_phrase_occ,
    };
    (doc_len, Some(candidate))
}

pub fn tokenize_query(query: &str) -> Vec<String> {
    let raw = query
        .to_lowercase()
        .split(is_query_separator)
        .filter(|token| token.chars().count() > 1)
        .filter(|token| !is_stop_word(token))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    let mut out = Vec::new();
    for token in raw {
        let chars = token.chars().collect::<Vec<_>>();
        let has_cjk = chars.iter().any(|c| ('\u{3400}'..='\u{9fff}').contains(c));
        if has_cjk && chars.len() > 2 {
            for pair in chars.windows(2) {
                out.push(pair.iter().collect());
            }
            for ch in &chars {
                let s = ch.to_string();
                if !is_stop_word(&s) {
                    out.push(s);
                }
            }
            out.push(token);
        } else {
            out.push(token);
        }
    }
    out.into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_query_separator(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '，' | '。'
                | '！'
                | '？'
                | '、'
                | '；'
                | '：'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '（'
                | '）'
                | '·'
                | '～'
                | '…'
        )
}

fn is_stop_word(token: &str) -> bool {
    matches!(
        token,
        "的" | "是"
            | "了"
            | "什么"
            | "在"
            | "有"
            | "和"
            | "与"
            | "对"
            | "从"
            | "the"
            | "is"
            | "a"
            | "an"
            | "what"
            | "how"
            | "are"
            | "was"
            | "were"
            | "do"
            | "does"
            | "did"
            | "be"
            | "been"
            | "being"
            | "have"
            | "has"
            | "had"
            | "it"
            | "its"
            | "in"
            | "on"
            | "at"
            | "to"
            | "for"
            | "of"
            | "with"
            | "by"
            | "this"
            | "that"
            | "these"
            | "those"
    )
}

fn trim_query_punctuation(value: &str) -> String {
    value.trim_matches(is_query_separator).to_string()
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.match_indices(needle).count()
}

// DEVWIKI: read a scalar YAML frontmatter field (e.g. `bc:`) from a page.
// Returns the trimmed, unquoted value, or None if there is no frontmatter or
// the key is absent. Only the leading `---`-delimited block is inspected.
pub fn frontmatter_field(content: &str, key: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }
    let prefix = format!("{key}:");
    for line in content.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let value = rest
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim();
            if value.is_empty() {
                return None;
            }
            return Some(value.to_string());
        }
    }
    None
}

pub fn extract_title(content: &str, file_name: &str) -> String {
    let has_frontmatter = content.starts_with("---");
    let mut in_frontmatter = has_frontmatter;
    let mut frontmatter_closed = false;
    for line in content.lines().skip(if has_frontmatter { 1 } else { 0 }) {
        let trimmed = line.trim();
        if in_frontmatter && trimmed == "---" {
            in_frontmatter = false;
            frontmatter_closed = true;
            continue;
        }
        if in_frontmatter && trimmed.starts_with("title:") {
            return trimmed
                .trim_start_matches("title:")
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
        }
        if has_frontmatter && !frontmatter_closed {
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("# ") {
            return title.trim().to_string();
        }
    }
    file_name.trim_end_matches(".md").replace('-', " ")
}

pub fn extract_image_refs(content: &str) -> Vec<SearchImageRef> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut rest = content;
    while let Some(start) = rest.find("![") {
        rest = &rest[start + 2..];
        let Some(alt_end) = rest.find("](") else {
            break;
        };
        let alt = &rest[..alt_end];
        rest = &rest[alt_end + 2..];
        let Some(url_end) = rest.find(')') else {
            break;
        };
        let url = &rest[..url_end];
        if !url.trim().is_empty()
            && !url.contains(char::is_whitespace)
            && seen.insert(url.to_string())
        {
            out.push(SearchImageRef {
                url: url.to_string(),
                alt: alt.to_string(),
            });
        }
        rest = &rest[url_end + 1..];
    }
    out
}

async fn fetch_embedding(text: &str, cfg: &SearchEmbeddingConfig) -> Result<Vec<f32>, String> {
    let is_google = is_google_embedding_config(cfg);
    let endpoint = if is_google {
        google_embedding_endpoint(cfg)
    } else {
        cfg.endpoint.trim().to_string()
    };
    let mut req = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            SEARCH_EMBEDDING_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| format!("Embedding HTTP client error: {e}"))?
        .post(endpoint)
        .header("Content-Type", "application/json");
    if !cfg.api_key.trim().is_empty() {
        if is_google {
            req = req.header("x-goog-api-key", cfg.api_key.trim());
        } else {
            req = req.bearer_auth(cfg.api_key.trim());
        }
    }
    if let Some(extra) = cfg.extra_headers.as_ref() {
        for (name, value) in extra {
            let trimmed = name.trim();
            let value = value.trim();
            if trimmed.is_empty() || value.is_empty() || !is_safe_extra_header_name(trimmed) {
                continue;
            }
            if is_reserved_extra_header_name(trimmed) {
                continue;
            }
            req = req.header(trimmed, value);
        }
    }
    let body = if is_google {
        google_embedding_body(&cfg.model, text, cfg.output_dimensionality)
    } else {
        json!({ "model": cfg.model, "input": text })
    };
    let resp = req
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Embedding request failed: {e}"))?;
    let status = resp.status();
    let data: Value = resp
        .json()
        .await
        .map_err(|e| format!("Embedding response parse failed: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "Embedding API HTTP {status}: {}",
            data.to_string().chars().take(200).collect::<String>()
        ));
    }
    let values = if is_google {
        data.get("embedding")
            .and_then(|v| v.get("values"))
            .and_then(Value::as_array)
    } else {
        data.get("data")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("embedding"))
            .and_then(Value::as_array)
    }
    .ok_or_else(|| "Embedding response missing vector".to_string())?;
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let n = value
            .as_f64()
            .ok_or_else(|| "Embedding response contains non-number values".to_string())?;
        if !n.is_finite() {
            return Err("Embedding response contains non-finite values".to_string());
        }
        out.push(n as f32);
    }
    if out.is_empty() {
        return Err("Embedding response vector is empty".to_string());
    }
    Ok(out)
}

fn is_safe_extra_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            matches!(
                b,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
                    | b'0'..=b'9'
                    | b'A'..=b'Z'
                    | b'a'..=b'z'
            )
        })
}

fn is_reserved_extra_header_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization" | "content-type" | "host" | "content-length" | "x-goog-api-key"
    )
}

fn is_google_embedding_config(cfg: &SearchEmbeddingConfig) -> bool {
    let endpoint = cfg.endpoint.to_lowercase();
    endpoint.contains("generativelanguage.googleapis.com") || endpoint.contains(":embedcontent")
}

fn google_embedding_endpoint(cfg: &SearchEmbeddingConfig) -> String {
    let raw = strip_google_api_key_query(cfg.endpoint.trim())
        .trim_end_matches('/')
        .to_string();
    if raw.to_lowercase().contains(":batchembedcontents") {
        return raw
            .replace(":batchEmbedContents", ":embedContent")
            .replace(":batchembedcontents", ":embedContent");
    }
    if raw.to_lowercase().contains(":embedcontent") {
        return raw;
    }
    let model = cfg.model.trim().trim_start_matches("models/");
    if raw.to_lowercase().contains("/models/") {
        format!("{raw}:embedContent")
    } else {
        format!("{raw}/models/{model}:embedContent")
    }
}

fn strip_google_api_key_query(endpoint: &str) -> String {
    if !endpoint.contains('?') {
        return endpoint.to_string();
    }
    match reqwest::Url::parse(endpoint) {
        Ok(mut url) => {
            let kept = url
                .query_pairs()
                .filter(|(key, _)| !key.eq_ignore_ascii_case("key"))
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect::<Vec<_>>();
            url.query_pairs_mut().clear().extend_pairs(kept);
            url.to_string().trim_end_matches('?').to_string()
        }
        Err(_) => endpoint
            .split_once('?')
            .map(|(base, query)| {
                let kept = query
                    .split('&')
                    .filter(|pair| {
                        let key = pair.split_once('=').map(|(k, _)| k).unwrap_or(*pair);
                        !key.eq_ignore_ascii_case("key")
                    })
                    .collect::<Vec<_>>();
                if kept.is_empty() {
                    base.to_string()
                } else {
                    format!("{base}?{}", kept.join("&"))
                }
            })
            .unwrap_or_else(|| endpoint.to_string()),
    }
}

fn google_embedding_body(model: &str, text: &str, output_dimensionality: Option<u32>) -> Value {
    let model_path = if model.trim().starts_with("models/") {
        model.trim().to_string()
    } else {
        format!("models/{}", model.trim())
    };
    let mut body = json!({
        "model": model_path,
        "content": { "parts": [{ "text": text }] },
    });
    if let Some(dim) = output_dimensionality.filter(|dim| *dim > 0) {
        body["output_dimensionality"] = json!(dim);
    }
    body
}

pub fn build_snippet(content: &str, query: &str) -> String {
    let lower = content.to_lowercase();
    let q = query.to_lowercase();
    let idx = lower.find(&q).unwrap_or(0);
    let char_positions: Vec<usize> = content.char_indices().map(|(idx, _)| idx).collect();
    if char_positions.is_empty() {
        return String::new();
    }
    let match_char = char_positions
        .iter()
        .position(|byte| *byte >= idx)
        .unwrap_or(char_positions.len().saturating_sub(1));
    let query_chars = query.chars().count().max(1);
    let start_char = match_char.saturating_sub(SNIPPET_CONTEXT);
    let end_char = (match_char + query_chars + SNIPPET_CONTEXT).min(char_positions.len());
    let start = char_positions[start_char];
    let end = if end_char < char_positions.len() {
        char_positions[end_char]
    } else {
        content.len()
    };
    let mut snippet = content[start..end].replace('\n', " ");
    if start > 0 {
        snippet = format!("...{snippet}");
    }
    if end < content.len() {
        snippet.push_str("...");
    }
    snippet
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn relative_to_project(project_path: &str, path: &Path) -> String {
    let root = Path::new(project_path);
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tmp_project() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("llm-wiki-search-test-{id}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("wiki/concepts")).unwrap();
        path
    }

    fn write_page(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn result(path: &str) -> ProjectSearchResult {
        ProjectSearchResult {
            path: path.to_string(),
            title: path.to_string(),
            snippet: String::new(),
            title_match: false,
            score: 0.0,
            vector_score: None,
            images: vec![],
            content: None,
        }
    }

    #[test]
    fn tokenizes_cjk_bigrams_and_chars() {
        let tokens = tokenize_query("默会知识");
        assert!(tokens.contains(&"默会".to_string()));
        assert!(tokens.contains(&"知识".to_string()));
        assert!(tokens.contains(&"默".to_string()));
    }

    #[test]
    fn extracts_image_refs_without_duplicates() {
        let refs = extract_image_refs("![a](wiki/media/x.png)\n![b](wiki/media/x.png)");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].alt, "a");
    }

    #[test]
    fn extract_title_uses_frontmatter_or_heading_not_body_title_lines() {
        let with_frontmatter = "---\ntitle: Real Title\n---\n\ntitle: Body Label\n# Heading";
        assert_eq!(
            extract_title(with_frontmatter, "fallback-name.md"),
            "Real Title"
        );

        let without_frontmatter = "intro\ntitle: Body Label\n# Real Heading";
        assert_eq!(
            extract_title(without_frontmatter, "fallback-name.md"),
            "Real Heading"
        );

        assert_eq!(
            extract_title("plain body", "vector-database.md"),
            "vector database"
        );
    }

    #[test]
    fn explicit_query_embedding_is_validated() {
        assert!(validate_query_embedding(vec![0.1, 0.2]).is_ok());
        assert!(validate_query_embedding(vec![]).is_err());
        assert!(validate_query_embedding(vec![f32::NAN]).is_err());
        assert!(validate_query_embedding(vec![f32::INFINITY]).is_err());
    }

    #[test]
    fn google_embedding_endpoint_strips_key_and_normalizes_batch_endpoint() {
        let cfg = SearchEmbeddingConfig {
            enabled: true,
            endpoint: "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:batchEmbedContents?key=URL_KEY&alt=json".to_string(),
            api_key: "HEADER_KEY".to_string(),
            model: "gemini-embedding-001".to_string(),
            output_dimensionality: Some(768),
            extra_headers: None,
        };

        let endpoint = google_embedding_endpoint(&cfg);
        assert!(endpoint.contains(":embedContent"));
        assert!(!endpoint.contains(":batchEmbedContents"));
        assert!(!endpoint.contains("URL_KEY"));
        assert!(endpoint.contains("alt=json"));

        let body = google_embedding_body("gemini-embedding-001", "hello", Some(768));
        assert_eq!(body["model"], "models/gemini-embedding-001");
        assert_eq!(body["output_dimensionality"], 768);
    }

    #[test]
    fn extra_embedding_header_names_are_validated_and_reserved_names_are_skipped() {
        assert!(is_safe_extra_header_name("X-Model-Provider-Id"));
        assert!(is_safe_extra_header_name("x_trace.id"));
        assert!(!is_safe_extra_header_name(""));
        assert!(!is_safe_extra_header_name("Bad Header"));
        assert!(!is_safe_extra_header_name("中文"));

        assert!(is_reserved_extra_header_name("Authorization"));
        assert!(is_reserved_extra_header_name("content-type"));
        assert!(is_reserved_extra_header_name("X-Goog-Api-Key"));
        assert!(!is_reserved_extra_header_name("X-Model-Provider-Id"));
    }

    #[test]
    fn rrf_combines_token_and_vector_ranks_and_keeps_vector_score() {
        let mut results = vec![
            result("wiki/concepts/both.md"),
            result("wiki/concepts/token-only.md"),
            result("wiki/concepts/vector-only.md"),
        ];
        let token_rank = BTreeMap::from([
            ("wiki/concepts/both.md".to_string(), 1),
            ("wiki/concepts/token-only.md".to_string(), 2),
        ]);
        let vector_rank = BTreeMap::from([("both".to_string(), 1), ("vector-only".to_string(), 2)]);
        let vector_score =
            BTreeMap::from([("both".to_string(), 0.95), ("vector-only".to_string(), 0.8)]);

        apply_rrf_scores(&mut results, &token_rank, &vector_rank, &vector_score);
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        assert_eq!(results[0].path, "wiki/concepts/both.md");
        assert!((results[0].score - (1.0 / 61.0 + 1.0 / 61.0)).abs() < 0.000001);
        assert_eq!(results[0].vector_score, Some(0.95));
        assert!((results[1].score - (1.0 / 62.0)).abs() < 0.000001);
        assert!((results[2].score - (1.0 / 62.0)).abs() < 0.000001);
    }

    #[test]
    fn search_mode_distinguishes_keyword_vector_and_hybrid() {
        assert_eq!(search_mode(false, 0), "keyword");
        assert_eq!(search_mode(true, 3), "vector");
        assert_eq!(search_mode(false, 3), "hybrid");
    }

    #[test]
    fn vector_only_materialization_uses_chunk_snippet_and_any_wiki_subdir() {
        let root = tmp_project();
        write_page(
            &root,
            "wiki/custom/deep-page.md",
            "---\ntitle: Deep Page\n---\n\n# Deep Page\n\nThe literal query is absent here.",
        );
        let vector_results = vec![PageVectorResult {
            id: "deep-page".to_string(),
            score: 0.91,
            chunk_text: "A semantic chunk explains the actual reason for retrieval.".to_string(),
            heading_path: "Section > Detail".to_string(),
        }];
        let mut results = Vec::new();
        let pages = BTreeMap::from([(
            "deep-page".to_string(),
            "wiki/custom/deep-page.md".to_string(),
        )]);

        materialize_vector_only_results(
            &vector_results,
            &pages,
            &root.to_string_lossy(),
            &mut results,
            false,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "wiki/custom/deep-page.md");
        assert_eq!(results[0].title, "Deep Page");
        assert_eq!(results[0].vector_score, Some(0.91));
        assert!(results[0].snippet.contains("Section > Detail"));
        assert!(results[0].snippet.contains("semantic chunk"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn vector_snippet_empty_chunk_does_not_echo_query() {
        let vector = PageVectorResult {
            id: "empty".to_string(),
            score: 0.5,
            chunk_text: "  ".to_string(),
            heading_path: "Heading".to_string(),
        };

        assert_eq!(build_vector_snippet(&vector), "");
    }

    #[tokio::test]
    async fn keyword_search_prefers_filename_exact_match() {
        let root = tmp_project();
        write_page(
            &root,
            "wiki/concepts/attention.md",
            "---\ntitle: Attention\n---\n\n# Attention\n\nbody about attention.",
        );
        write_page(
            &root,
            "wiki/concepts/random.md",
            "---\ntitle: Random\n---\n\n# Random\n\nattention is mentioned briefly.",
        );

        let out = search_project_inner(
            root.to_string_lossy().to_string(),
            "attention".into(),
            20,
            false,
            None,
            None,  // DEVWIKI: no bc filter in this test
            false, // DEVWIKI: no trace
            None,  // DEVWIKI: no phase
        )
        .await
        .unwrap();

        assert_eq!(out.mode, "keyword");
        assert_eq!(out.results[0].title, "Attention");
        assert!(out.results[0].title_match);
        assert!(out.results[0].score > 100.0);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn keyword_search_handles_cjk_bigram_queries() {
        let root = tmp_project();
        write_page(
            &root,
            "wiki/concepts/tacit.md",
            "---\ntitle: 默会知识\n---\n\n# 默会知识\n\n默会知识强调难以言明的实践经验。",
        );

        let out = search_project_inner(
            root.to_string_lossy().to_string(),
            "默会知识".into(),
            20,
            false,
            None,
            None,  // DEVWIKI: no bc filter in this test
            false, // DEVWIKI: no trace
            None,  // DEVWIKI: no phase
        )
        .await
        .unwrap();

        assert_eq!(out.results[0].title, "默会知识");
        assert!(out.results[0].snippet.contains("默会知识"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn keyword_search_phrase_in_content_beats_scattered_tokens() {
        let root = tmp_project();
        write_page(
            &root,
            "wiki/concepts/phrase.md",
            "---\ntitle: Phrase\n---\n\n# Phrase\n\nThe phrase vector database appears together.",
        );
        write_page(
            &root,
            "wiki/concepts/scattered.md",
            "---\ntitle: Scattered\n---\n\n# Scattered\n\nvector appears here. database appears later.",
        );

        let out = search_project_inner(
            root.to_string_lossy().to_string(),
            "vector database".into(),
            20,
            false,
            None,
            None,  // DEVWIKI: no bc filter in this test
            false, // DEVWIKI: no trace
            None,  // DEVWIKI: no phase
        )
        .await
        .unwrap();

        assert_eq!(out.results[0].title, "Phrase");
        let _ = fs::remove_dir_all(root);
    }

    // DEVWIKI: bounded-context filter unit tests
    #[test]
    fn frontmatter_field_reads_scalar_bc() {
        let page = "---\ntype: decision\nbc: payment\ntitle: X\n---\n\n# X\nbody";
        assert_eq!(frontmatter_field(page, "bc").as_deref(), Some("payment"));
        let quoted = "---\nbc: \"payment\"\n---\nbody";
        assert_eq!(frontmatter_field(quoted, "bc").as_deref(), Some("payment"));
        let no_fm = "# X\nbc: payment";
        assert_eq!(frontmatter_field(no_fm, "bc"), None);
        let absent = "---\ntype: concept\n---\nbody";
        assert_eq!(frontmatter_field(absent, "bc"), None);
    }

    #[tokio::test]
    async fn bc_filter_only_returns_matching_pages() {
        let root = tmp_project();
        write_page(
            &root,
            "wiki/decisions/pay.md",
            "---\ntype: decision\nbc: payment\ntitle: Retry\n---\n\n# Retry\n\n支付失败重试机制说明。",
        );
        write_page(
            &root,
            "wiki/decisions/ship.md",
            "---\ntype: decision\nbc: shipping\ntitle: Retry\n---\n\n# Retry\n\n支付失败重试机制说明。",
        );

        // Without filter both pages match the query.
        let all = search_project_inner(
            root.to_string_lossy().to_string(),
            "支付失败重试".into(),
            20,
            false,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        assert_eq!(all.results.len(), 2);

        // With bc=payment only the payment page survives.
        let filtered = search_project_inner(
            root.to_string_lossy().to_string(),
            "支付失败重试".into(),
            20,
            false,
            None,
            Some("Payment".into()), // case-insensitive
            false,
            None,
        )
        .await
        .unwrap();
        assert_eq!(filtered.results.len(), 1);
        assert!(filtered.results[0].path.ends_with("pay.md"));
        let _ = fs::remove_dir_all(root);
    }

    // DEVWIKI (P4①): retrieval-trace tests.
    #[tokio::test]
    async fn keyword_trace_reconstructs_scores_and_ranks() {
        let root = tmp_project();
        write_page(
            &root,
            "wiki/concepts/attention.md",
            "---\ntitle: Attention\n---\n\n# Attention\n\nbody about attention.",
        );
        write_page(
            &root,
            "wiki/concepts/random.md",
            "---\ntitle: Random\n---\n\n# Random\n\nattention is mentioned briefly.",
        );

        // with_trace = false ⇒ no trace attached (backward-compatible default).
        let without = search_project_inner(
            root.to_string_lossy().to_string(),
            "attention".into(),
            20,
            false,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        assert!(without.trace.is_none());

        // with_trace = true ⇒ keyword-mode trace with per-candidate detail.
        let out = search_project_inner(
            root.to_string_lossy().to_string(),
            "attention".into(),
            20,
            false,
            None,
            None,
            true,
            Some("dev".into()),
        )
        .await
        .unwrap();

        let trace = out.trace.expect("trace requested");
        assert_eq!(trace.mode, "keyword");
        assert_eq!(trace.paths_run, vec!["keyword".to_string()]);
        assert_eq!(trace.phase.as_deref(), Some("dev"));
        assert_eq!(trace.candidates.len(), out.results.len());

        // The filename-exact page ranks first and carries a keyword score but
        // no vector score (vector path never ran).
        let top = &trace.candidates[0];
        assert_eq!(top.page_id, "attention");
        assert_eq!(top.final_rank, 1);
        assert_eq!(top.keyword_rank, Some(1));
        assert!(top.keyword_score.unwrap() > 100.0);
        assert_eq!(top.rrf_score, top.keyword_score.unwrap()); // no RRF in keyword mode
        assert!(top.vector_score.is_none());
        assert!(top.vector_rank.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_trace_gates_keyword_score_on_keyword_hit() {
        // Simulate the hybrid end-state: `both` hit on keyword+vector,
        // `vector-only` hit on vector alone (placeholder 0.0 keyword score).
        let mut both = result("wiki/concepts/both.md");
        both.score = 1.0 / 61.0 + 1.0 / 61.0; // RRF score after fusion
        both.vector_score = Some(0.95);
        let mut vector_only = result("wiki/concepts/vector-only.md");
        vector_only.score = 1.0 / 62.0;
        vector_only.vector_score = Some(0.8);
        let final_results = vec![both, vector_only];

        let token_rank = BTreeMap::from([("wiki/concepts/both.md".to_string(), 1)]);
        let vector_rank =
            BTreeMap::from([("both".to_string(), 1), ("vector-only".to_string(), 2)]);
        // Raw keyword score snapshot — only `both` had a keyword hit.
        let keyword_scores = BTreeMap::from([("wiki/concepts/both.md".to_string(), 207.0)]);

        let trace = build_retrieval_trace(
            &final_results,
            &token_rank,
            &vector_rank,
            &keyword_scores,
            "vector database",
            &Some("payment".to_string()),
            &None,
            "hybrid",
            true,
            10,
            1,
            2,
        );

        assert_eq!(trace.paths_run, vec!["keyword".to_string(), "vector".to_string()]);
        assert_eq!(trace.bc.as_deref(), Some("payment"));

        let c0 = &trace.candidates[0];
        assert_eq!(c0.page_id, "both");
        assert_eq!(c0.keyword_rank, Some(1));
        assert_eq!(c0.keyword_score, Some(207.0));
        assert_eq!(c0.vector_rank, Some(1));
        assert_eq!(c0.vector_score, Some(0.95));
        assert_eq!(c0.final_rank, 1);

        // Vector-only candidate: NO keyword score despite a 0.0 placeholder.
        let c1 = &trace.candidates[1];
        assert_eq!(c1.page_id, "vector-only");
        assert_eq!(c1.keyword_rank, None);
        assert_eq!(c1.keyword_score, None);
        assert_eq!(c1.vector_rank, Some(2));
        assert_eq!(c1.final_rank, 2);
    }

    // DEVWIKI (P4③): BM25 tests.
    #[test]
    fn bm25_score_saturates_and_normalizes() {
        let idf = BTreeMap::from([("t".to_string(), 1.0)]);
        let tf1 = BTreeMap::from([("t".to_string(), 1.0)]);
        let tf2 = BTreeMap::from([("t".to_string(), 2.0)]);
        let avg = 10.0;

        // More term frequency scores higher, but sub-linearly (TF saturation).
        let s1 = bm25_score(&tf1, 10.0, avg, &idf);
        let s2 = bm25_score(&tf2, 10.0, avg, &idf);
        assert!(s2 > s1);
        assert!(s2 < 2.0 * s1, "doubling tf should less-than-double score");

        // Length normalization: a shorter-than-average doc beats a longer one
        // for the same term frequency.
        let short = bm25_score(&tf1, 2.0, avg, &idf);
        let long = bm25_score(&tf1, 50.0, avg, &idf);
        assert!(short > long);

        // A term with non-positive IDF (in every doc) contributes nothing.
        let idf0 = BTreeMap::from([("t".to_string(), 0.0)]);
        assert_eq!(bm25_score(&tf1, 10.0, avg, &idf0), 0.0);
    }

    #[test]
    fn collect_doc_weights_title_and_summary_above_body() {
        let path = Path::new("wiki/concepts/foo.md");
        let toks = vec!["lancedb".to_string()];
        let call = |content: &str| {
            collect_doc(".", path, content, &toks, "lancedb", "lancedb", false)
                .1
                .expect("candidate")
                .weighted_tf["lancedb"]
        };

        // Body-only occurrence → weight 1.
        let body = call("---\ntitle: Foo\n---\n\n# Foo\n\nlancedb appears in the body.");
        assert_eq!(body, 1.0);

        // Same term in the `summary` frontmatter → effective weight 2
        // (1 from the body scan of the frontmatter line + 1 summary boost).
        let summary = call("---\nsummary: lancedb vector store\ntitle: Foo\n---\n\n# Foo\n\nbody text.");
        assert_eq!(summary, 2.0);

        // In the title it outweighs the summary (title boost is larger).
        let title = call("---\ntitle: lancedb guide\n---\n\n# lancedb guide\n\nbody text.");
        assert!(title > summary, "title weight {title} should exceed summary {summary}");
    }

    #[tokio::test]
    async fn bm25_idf_favors_rarer_query_term() {
        let root = tmp_project();
        // Four pages share a common term; one also carries a rare term.
        for i in 0..4 {
            write_page(
                &root,
                &format!("wiki/concepts/common{i}.md"),
                "---\ntitle: Common\n---\n\n# Common\n\nThis page is about kubernetes orchestration.",
            );
        }
        write_page(
            &root,
            "wiki/concepts/rare.md",
            "---\ntitle: Rare\n---\n\n# Rare\n\nkubernetes plus the rare term photosynthesis here.",
        );

        // "kubernetes" is in every doc (low IDF); "photosynthesis" in one (high
        // IDF). The doc matching the rare term must rank first.
        let out = search_project_inner(
            root.to_string_lossy().to_string(),
            "kubernetes photosynthesis".into(),
            20,
            false,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();

        assert_eq!(out.mode, "keyword");
        assert!(out.results[0].path.ends_with("rare.md"));
        // And it strictly outscores every common-only page.
        assert!(out.results[1..].iter().all(|r| out.results[0].score > r.score));
        let _ = fs::remove_dir_all(root);
    }
}
