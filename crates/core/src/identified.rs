//! Minimal identity contract so generic entity storage can stay typed.

use vibe_domain::{
    ArchitectureEvolutionPlan, Artifact, ChangeProposal, Decision, DriftAlert, Evidence, Issue,
    Milestone, Task,
};

pub trait Identified {
    fn id(&self) -> &str;
    fn project_id(&self) -> &str;
}

macro_rules! identified {
    ($($t:ty),* $(,)?) => {
        $(
            impl Identified for $t {
                fn id(&self) -> &str { &self.id }
                fn project_id(&self) -> &str { &self.project_id }
            }
        )*
    };
}

identified!(
    Task,
    Issue,
    Decision,
    Artifact,
    Milestone,
    ChangeProposal,
    ArchitectureEvolutionPlan,
    DriftAlert,
    Evidence,
);
