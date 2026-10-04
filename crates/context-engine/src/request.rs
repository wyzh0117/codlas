//! What the user is actually asking, and what they pointed the engine at (§40).

use serde::{Deserialize, Serialize};
use vibe_domain::{ContextBudget, EntityKind};

/// A local object the user selected — in the app, on the command line, or by
/// replying to a prompt. §42.1 puts selected context at the very top of the
/// retrieval ladder, so a selection is a fact about intent, not a search term:
/// retrieval goes and reads that exact record rather than guessing from keywords.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// `issue` / `task` / `decision` / `artifact` / `aep` / `event` / `message` …
    pub kind: String,
    pub id: String,
    /// Short human label, used in the audit trail and the compiled line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Selection {
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
            label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The vocabulary this crate uses for a selection: the entity kinds that are
    /// stored generically. `event` / `message` / `map` / `charter` have their own
    /// stores, so they resolve to `None` and are read through their own port.
    pub fn entity_kind(&self) -> Option<EntityKind> {
        EntityKind::parse(&self.kind)
    }

    /// `<kind>:<id>`, the same shape every `source_ref` in the pack uses, so a
    /// selection can be matched against retrieval output and against the §53 gate.
    pub fn source_ref(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }
}

/// Text a caller already holds in memory, offered to one reference line.
///
/// This crate does not read files (§22 keeps file bodies out of the store, and the
/// engine stays free of the filesystem). An app or adapter that *has* already read a
/// file may attach the text here, which is also what gives §53 `neverSendFiles`
/// something to strip back off before the pack leaves the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Excerpt {
    /// The `<kind>:<id>` line this belongs to, e.g. `artifact:42`.
    pub source_ref: String,
    pub text: String,
}

impl Excerpt {
    pub fn new(source_ref: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            source_ref: source_ref.into(),
            text: text.into(),
        }
    }

    /// Excerpt for the artifact stored under `id`.
    pub fn artifact_ref(id: &str, text: impl Into<String>) -> Self {
        Self::new(format!("artifact:{id}"), text)
    }
}

/// One retrieval + compilation request (§40, §42).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextRequest {
    pub project_id: String,
    /// The user's own words. Always included (§41 "Request"), never truncated (§43).
    pub question: String,
    pub selections: Vec<Selection>,
    /// Free-text focus terms; only used to rank what retrieval already found.
    pub focus: Vec<String>,
    /// Where the pack is headed (`chatgpt-web`, `generic-cli`, …). Drives §53.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent: Option<String>,
    /// Token budget. `None` means the caller had no preference; the compiler then
    /// uses [`ContextCompiler::default_budget`] rather than an unbounded pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<ContextBudget>,
    /// How many recent items to read per §42 tier. A bounded window, never the
    /// whole history (§59).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<usize>,
    /// Locally-held text the caller volunteers for a compiled reference line. The
    /// engine never reads files to fill these; §53 `neverSendFiles` removes them
    /// again before anything is assembled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excerpts: Vec<Excerpt>,
}

impl ContextRequest {
    pub fn new(project_id: impl Into<String>, question: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            question: question.into(),
            selections: Vec::new(),
            focus: Vec::new(),
            target_agent: None,
            budget: None,
            window: None,
            excerpts: Vec::new(),
        }
    }

    /// Attach text this process already holds to the reference it belongs to.
    pub fn attach_excerpt(mut self, excerpt: Excerpt) -> Self {
        self.excerpts.push(excerpt);
        self
    }

    pub fn select(mut self, selection: Selection) -> Self {
        self.selections.push(selection);
        self
    }

    pub fn focused_on(mut self, term: impl Into<String>) -> Self {
        self.focus.push(term.into());
        self
    }

    pub fn for_agent(mut self, target: impl Into<String>) -> Self {
        self.target_agent = Some(target.into());
        self
    }

    pub fn with_budget(mut self, budget: ContextBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn with_window(mut self, window: usize) -> Self {
        self.window = Some(window);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_names_its_store() {
        assert_eq!(
            Selection::new("issue", "I-018").entity_kind(),
            Some(EntityKind::Issue)
        );
        assert_eq!(Selection::new("aep", "AEP-002").entity_kind(), Some(EntityKind::Aep));
        assert_eq!(Selection::new("message", "m-1").entity_kind(), None);
        assert_eq!(Selection::new("event", "e-1").entity_kind(), None);
    }

    #[test]
    fn a_selection_wears_the_ref_shape_the_gate_reads() {
        assert_eq!(
            vibe_security::ref_kind(&Selection::new("artifact", "a-9").source_ref()),
            "artifact"
        );
    }
}
