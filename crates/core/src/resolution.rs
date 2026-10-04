//! Entity Resolution (V1_DEVELOPMENT.md §28). Decides whether "auth",
//! "authentication" and "登录模块" name the same object.
//!
//! Anything short of an explicit id or an exact title match is reported as a
//! merge candidate instead of being merged.

use vibe_domain::{EntityKind, Issue, Task};

use crate::error::Result;
use crate::ports::{EntityRepository, EntityStore, Repositories, SearchIndex};

/// Below this, a merge is never automatic.
pub const AUTO_MERGE_CONFIDENCE: f64 = 0.90;

#[derive(Debug, Clone)]
pub struct Resolved {
    pub kind: EntityKind,
    pub id: String,
    pub confidence: f64,
    pub basis: &'static str,
}

impl Resolved {
    pub fn auto_mergeable(&self) -> bool {
        self.confidence >= AUTO_MERGE_CONFIDENCE
    }
}

pub struct EntityResolver {
    entities: std::sync::Arc<dyn EntityRepository>,
    search: std::sync::Arc<dyn SearchIndex>,
}

impl EntityResolver {
    pub fn new(repos: &Repositories) -> Self {
        Self {
            entities: repos.entities.clone(),
            search: repos.search.clone(),
        }
    }

    /// Order matters: explicit id, then stored relations, then exact title, then
    /// lexical overlap (§28 basis list).
    pub fn resolve_title(
        &self,
        project_id: &str,
        kind: EntityKind,
        title: &str,
    ) -> Result<Option<Resolved>> {
        let wanted = crate::project_service::tokenize(title);
        match kind {
            EntityKind::Task => {
                let tasks: Vec<Task> = self.entities.entities_of(kind, project_id)?;
                Ok(best_match(
                    tasks.iter().map(|t| (t.id.clone(), t.title.clone(), kind)),
                    title,
                    &wanted,
                ))
            }
            EntityKind::Issue => {
                let issues: Vec<Issue> = self.entities.entities_of(kind, project_id)?;
                Ok(best_match(
                    issues.iter().map(|i| (i.id.clone(), i.title.clone(), kind)),
                    title,
                    &wanted,
                ))
            }
            other => {
                // Decisions, artifacts and AEPs resolve through the index.
                let hits = self.search.search(
                    title,
                    &crate::ports::SearchFilters {
                        project_id: Some(project_id.to_string()),
                        kinds: vec![other.as_str().to_string()],
                        limit: 3,
                    },
                )?;
                Ok(hits.first().and_then(|hit| {
                    let confidence = if hit.rank >= 8.0 { 0.95 } else { 0.70 };
                    Some(Resolved {
                        kind: other,
                        id: hit.id.clone(),
                        confidence,
                        basis: "full-text index",
                    })
                }))
            }
        }
    }
}

fn best_match<I>(candidates: I, title: &str, wanted: &[&str]) -> Option<Resolved>
where
    I: Iterator<Item = (String, String, EntityKind)>,
{
    let mut fallback: Option<Resolved> = None;
    for (id, existing, kind) in candidates {
        if id == title {
            return Some(Resolved {
                kind,
                id,
                confidence: 1.0,
                basis: "explicit id",
            });
        }
        let norm = vibe_domain::normalize_for_hash(&existing).to_lowercase();
        if norm == vibe_domain::normalize_for_hash(title).to_lowercase() {
            return Some(Resolved {
                kind,
                id,
                confidence: 0.95,
                basis: "exact title",
            });
        }
        let score = crate::project_service::overlap(
            &wanted.iter().copied().collect::<Vec<_>>().join(" "),
            &existing.to_lowercase(),
        );
        if score >= 0.6 && fallback.is_none() {
            fallback = Some(Resolved {
                kind,
                id,
                confidence: 0.70,
                basis: "title overlap",
            });
        }
    }
    fallback
}
