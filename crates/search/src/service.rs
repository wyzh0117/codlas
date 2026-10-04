//! The three retrieval layers of §40, composed from ports only.
//!
//! ```text
//! L1 structured ── id / kind / status / title over the entity + event + record reads
//! L2 full text ── SearchIndex::search, the store's own ranking
//! L3 relation ─── SearchIndex::related + EntityRepository::links_from, one hop
//! ```
//!
//! Nothing in here mentions SQLite, rusqlite or a table name (§6 禁止 7): the crate's only
//! dependencies are the [`Repositories`] port bundle and the domain types. Which means the
//! same service works against an in-memory fake, and a store change cannot break search.
//!
//! §60's cheap-first rule decides the order of work: L1 is a bounded read of rows already
//! shaped for it, L2 is one indexed query, L3 is a graph hop from what L1/L2 already
//! found. Nothing re-scans the corpus to answer "what is near this?".

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use vibe_core::error::Result;
use vibe_core::ports::{Repositories, SearchHit};

use crate::catalog::{self, Record};

/// Score for an L1 exact id hit: the user asked for this record by name.
pub const SCORE_EXACT_ID: f64 = 1.0;
/// Score for an L1 title that is identical to the query, ignoring case.
pub const SCORE_EXACT_TITLE: f64 = 0.95;
/// Score for an L1 title containing the query.
pub const SCORE_TITLE_MATCH: f64 = 0.85;
/// Score for an L1 browse result — a kind/status filter with no text to rank by.
pub const SCORE_BROWSE: f64 = 0.8;
/// Top of the L2 band: full text never outranks a structured id match, but its own
/// ordering survives as a per-position decay.
pub const SCORE_FULL_TEXT: f64 = 0.6;
/// Top of the L3 band: related material is worth seeing, not worth believing.
pub const SCORE_RELATION: f64 = 0.35;
/// Per-position decay inside a layer, so a store's ranking order is preserved without
/// ever letting a later hit overtake an earlier one or a higher layer.
const POSITION_DECAY: f64 = 0.001;

/// Which layer produced a hit — the reason a result is on the list at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchLayer {
    Structured,
    FullText,
    Relation,
}

impl SearchLayer {
    pub fn as_str(&self) -> &'static str {
        match self {
            SearchLayer::Structured => "structured",
            SearchLayer::FullText => "full_text",
            SearchLayer::Relation => "relation",
        }
    }
}

/// One result, tagged with its layer and score.
#[derive(Debug, Clone)]
pub struct Found {
    pub hit: SearchHit,
    pub layer: SearchLayer,
    pub score: f64,
}

impl Found {
    fn new(hit: SearchHit, layer: SearchLayer, score: f64) -> Self {
        Self { hit, layer, score }
    }

    /// `kind:id` — the identity a merged result list dedups on.
    pub fn key(&self) -> (String, String) {
        (self.hit.kind.clone(), self.hit.id.clone())
    }
}

/// L1's filter: every field is optional, and the ones that are set are ANDed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredQuery {
    /// Also passed as an argument to [`SearchService::structured`]; the argument wins
    /// when both are set, so a CLI can reuse one query object across projects.
    #[serde(default)]
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A record kind slug (`task`, `decision`, `event`, `message`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A status slug (`todo`, `active`, `resolved`, `deleted`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub limit: usize,
}

impl Default for StructuredQuery {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            id: None,
            kind: None,
            title: None,
            status: None,
            limit: crate::DEFAULT_LIMIT,
        }
    }
}

impl StructuredQuery {
    /// Fetch one record by id, whatever kind it is.
    pub fn by_id(project_id: &str, id: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        }
    }

    /// Browse one kind by status — "every open issue", "every superseded decision".
    pub fn by_status(project_id: &str, kind: &str, status: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            kind: Some(kind.to_string()),
            status: Some(status.to_string()),
            ..Default::default()
        }
    }

    /// Match a title, optionally inside one kind.
    pub fn by_title(project_id: &str, title: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            title: Some(title.to_string()),
            ..Default::default()
        }
    }
}

/// Structured, full-text and relation retrieval over the same three ports (§40).
pub struct SearchService {
    repos: Repositories,
}

impl SearchService {
    pub fn new(repos: Repositories) -> Self {
        Self { repos }
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repos
    }

    /// L2: the store's own ranking, scoped to one project and to `kinds` when given.
    pub fn text(
        &self,
        project_id: &str,
        query: &str,
        kinds: &[&str],
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let filters = catalog::filters(project_id, kinds, limit);
        let mut hits = self.repos.search.search(query, &filters)?;
        // The port already orders by rank; this only makes ties deterministic.
        hits.sort_by(|a, b| {
            b.rank
                .partial_cmp(&a.rank)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.id.cmp(&b.id))
        });
        let limit = if limit == 0 { crate::DEFAULT_LIMIT } else { limit };
        hits.truncate(limit);
        Ok(hits)
    }

    /// L1: id / kind / status / title against the records themselves.
    pub fn structured(
        &self,
        project_id: &str,
        query: &StructuredQuery,
    ) -> Result<Vec<SearchHit>> {
        let found = self.structured_found(project_id, query)?;
        Ok(found
            .into_iter()
            .map(|item| SearchHit {
                rank: item.score,
                ..item.hit
            })
            .collect())
    }

    /// L3: what the relation graph hangs off this record, inside this project only.
    pub fn related(&self, project_id: &str, kind: &str, id: &str) -> Result<Vec<SearchHit>> {
        let mut seen: HashMap<(String, String), SearchHit> = HashMap::new();

        // The index join answers "which indexed records link to this one?".
        for hit in self.repos.search.related(kind, id)? {
            // Project isolation lives here, not in the port: `related` is keyed by
            // (kind, id) alone, and ids are content-derived, so a cross-project row would
            // otherwise leak straight through (§7 privacy).
            if hit.project_id != project_id {
                continue;
            }
            seen.insert((hit.kind.clone(), hit.id.clone()), hit);
        }

        // Links the index does not cover still count: a relation is a first-class row.
        if let Some(entity_kind) = vibe_domain::EntityKind::parse(kind) {
            for (to_kind, to_id, _relation) in
                self.repos.entities.links_from(entity_kind, id)?
            {
                if seen.contains_key(&(to_kind.as_str().to_string(), to_id.clone())) {
                    continue;
                }
                if let Some(record) =
                    catalog::find_by_id(&self.repos, project_id, to_kind.as_str(), &to_id)?
                {
                    seen.insert(
                        (record.kind.clone(), record.id.clone()),
                        record.hit(project_id, crate::SNIPPET_CHARS),
                    );
                }
            }
        }

        let mut hits: Vec<SearchHit> = seen.into_values().collect();
        hits.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.id.cmp(&b.id)));
        Ok(hits)
    }

    /// The §40 pipeline: L1, then L2, then L3 from what they found, merged so a record
    /// appears once under the layer that ranked it best.
    pub fn find(&self, project_id: &str, query: &str, limit: usize) -> Result<Vec<Found>> {
        self.find_in(project_id, query, &[], limit)
    }

    /// [`find`] narrowed to the kinds the caller named. The filter has to be applied by
    /// every layer, not just the indexed one: a `--kind task` answer that still carried
    /// event rows from L1 or a relation hop would be a filter in name only.
    pub fn find_in(
        &self,
        project_id: &str,
        query: &str,
        kinds: &[String],
        limit: usize,
    ) -> Result<Vec<Found>> {
        let limit = page(limit);
        let needle = query.trim();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let wanted = |kind: &str| kinds.is_empty() || kinds.iter().any(|k| k == kind);

        // L1 first, in one pass over the records: an exact id is the strongest signal
        // retrieval can get, so it always outranks a text match.
        let records = catalog::records(&self.repos, project_id, kinds)?;
        let mut merged: HashMap<(String, String), Found> = HashMap::new();
        for record in records.iter().filter_map(|r| score_free_text(r, project_id, needle)) {
            merge(&mut merged, record);
        }

        // L2: one indexed query, no re-scanning per result.
        let scoped: Vec<&str> = kinds.iter().map(String::as_str).collect();
        let full_text = self.text(project_id, needle, &scoped, limit)?;
        for (position, hit) in full_text.into_iter().enumerate() {
            merge(
                &mut merged,
                Found::new(
                    hit,
                    SearchLayer::FullText,
                    SCORE_FULL_TEXT - position as f64 * POSITION_DECAY,
                ),
            );
        }

        // L3 follows links out of the hits above instead of re-running the full-text scan:
        // a graph hop is bounded by how many relations a record has, while bm25 over the
        // whole corpus is bounded by the corpus — and §60 says spend the cheap query first.
        let seeds: Vec<(String, String)> = ranked(&merged)
            .iter()
            .take(crate::RELATION_SEEDS)
            .map(|item| (item.hit.kind.clone(), item.hit.id.clone()))
            .collect();
        for (kind, id) in seeds {
            for hit in self.related(project_id, &kind, &id)? {
                if hit.project_id != project_id || !wanted(&hit.kind) {
                    continue;
                }
                merge(
                    &mut merged,
                    Found::new(hit, SearchLayer::Relation, SCORE_RELATION),
                );
            }
        }

        Ok(ranked(&merged)
            .into_iter()
            .take(limit)
            .collect::<Vec<_>>())
    }

    /// Rebuild the full-text layer from the ports (§40 "keep the index honest").
    ///
    /// Covers messages, events, tasks, issues, decisions, artifacts and AEPs — plus
    /// milestones, change proposals and drift alerts, since an index that misses a kind
    /// makes that kind unfindable. Idempotent: indexing is a replace, so a second run
    /// writes the same rows and returns the same count.
    pub fn reindex(&self, project_id: &str) -> Result<usize> {
        let records = catalog::records(&self.repos, project_id, &[])?;
        for record in &records {
            self.repos.search.index(
                &record.kind,
                &record.id,
                project_id,
                &record.title,
                &record.body,
            )?;
        }
        Ok(records.len())
    }

    /// Records that satisfy every field the query sets, scored by how exactly they match.
    fn structured_found(
        &self,
        project_id: &str,
        query: &StructuredQuery,
    ) -> Result<Vec<Found>> {
        let project = if project_id.trim().is_empty() {
            query.project_id.as_str()
        } else {
            project_id
        };
        let kinds: Vec<String> = match query.kind.as_deref() {
            Some(kind) => vec![kind.to_string()],
            None => Vec::new(),
        };
        let mut found: Vec<Found> = Vec::new();
        for record in catalog::records(&self.repos, project, &kinds)? {
            let Some(score) = score_record(&record, query) else {
                continue;
            };
            found.push(Found::new(
                record.hit(project, crate::SNIPPET_CHARS),
                SearchLayer::Structured,
                score,
            ));
        }
        found.sort_by(|a, b| compare(a, b));
        found.truncate(page(query.limit));
        Ok(found)
    }
}

/// One record's L1 score, or `None` when it fails a filter the query set.
///
/// A text query is matched against the *title* only: body matching is L2's job, and
/// letting L1 read every body would quietly turn the cheap layer into the expensive one
/// (§60).
fn score_record(record: &Record, query: &StructuredQuery) -> Option<f64> {
    let id = query.id.as_deref().map(str::trim).unwrap_or("");
    if !id.is_empty() && record.id != id {
        return None;
    }
    if let Some(status) = query.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !record.status_matches(status) {
            return None;
        }
    }
    let title = query.title.as_deref().map(str::trim).unwrap_or("");
    if !title.is_empty() {
        if record.id == title {
            return Some(SCORE_EXACT_ID);
        }
        if record.title_matches(title) {
            return Some(if record.title.trim().eq_ignore_ascii_case(title) {
                SCORE_EXACT_TITLE
            } else {
                SCORE_TITLE_MATCH
            });
        }
        return None;
    }
    // Nothing textual left: an id lookup, or a kind/status browse.
    Some(if id.is_empty() {
        SCORE_BROWSE
    } else {
        SCORE_EXACT_ID
    })
}

/// L1 scoring for [`SearchService::find`]: the free text is tried as an id and as a title.
fn score_free_text(record: &Record, project_id: &str, needle: &str) -> Option<Found> {
    let score = if record.id == needle {
        SCORE_EXACT_ID
    } else if record.title_matches(needle) {
        if record.title.trim().eq_ignore_ascii_case(needle) {
            SCORE_EXACT_TITLE
        } else {
            SCORE_TITLE_MATCH
        }
    } else {
        return None;
    };
    Some(Found::new(
        record.hit(project_id, crate::SNIPPET_CHARS),
        SearchLayer::Structured,
        score,
    ))
}

/// Insert `item` unless the same `(kind, id)` already arrived from a better layer.
fn merge(merged: &mut HashMap<(String, String), Found>, item: Found) {
    match merged.get(&item.key()) {
        Some(existing) if existing.score >= item.score => {}
        Some(_) => {
            merged.insert(item.key(), item);
        }
        None => {
            merged.insert(item.key(), item);
        }
    }
}

/// The merged map as a deterministic list: score first, then kind and id, so two runs
/// over the same data answer in the same order (§18 determinism, applied to retrieval).
fn ranked(merged: &HashMap<(String, String), Found>) -> Vec<Found> {
    let mut out: Vec<Found> = merged.values().cloned().collect();
    out.sort_by(|a, b| compare(a, b));
    out
}

fn compare(a: &Found, b: &Found) -> std::cmp::Ordering {
    b.score
        .partial_cmp(&a.score)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a.layer.cmp(&b.layer))
        .then_with(|| a.hit.kind.cmp(&b.hit.kind))
        .then_with(|| a.hit.id.cmp(&b.hit.id))
}

fn page(limit: usize) -> usize {
    if limit == 0 {
        crate::DEFAULT_LIMIT
    } else {
        limit
    }
}
