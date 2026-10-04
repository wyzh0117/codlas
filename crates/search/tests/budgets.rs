//! §59's "Indexed Search", measured the only way a test can prove an index: cost that does
//! not track corpus size.
//!
//! A `LIKE '%term%'` scan and an FTS5 `MATCH` answer the same questions at this size, so
//! correctness alone cannot tell them apart. What they cannot fake is the ratio: a scan
//! reads every row, so 40 times the material costs 40 times the time; an index walks a
//! b-tree and a posting list, so it costs almost nothing more. Both halves of the
//! measurement run in the same process, on the same machine, minutes apart, which is what
//! makes a ratio a fair claim where an absolute millisecond figure would not be.
//!
//! The scale assertions ride along: a rare phrase still resolves to its one row in a
//! 2 000-row workbench, and a piece of a token still finds nothing, because the store
//! answers with tokens and never with a substring.

use std::sync::Arc;
use std::time::Instant;

use vibe_core::ports::{Repositories, SearchFilters};
use vibe_core::project_service::ProjectService;
use vibe_storage::SqliteStore;

/// Rows indexed in the small corpus, and in the large one. 20:1 is wide enough that a scan
/// cannot hide inside the noise, and small enough that building a corpus stays a test
/// rather than a benchmark run.
const SMALL: usize = 100;
const LARGE: usize = 2_000;

/// Queries per measurement, so one sub-millisecond call does not decide the outcome.
const REPEATS: usize = 60;

/// The large corpus may not cost more than this multiple of the small one. A scan would
/// land near 20; the FTS index measures around 1-3 on a laptop.
const RATIO_LIMIT: f64 = 6.0;

/// The phrase one row of each corpus carries, so the match list is one row long at any
/// size and only the reading cost can differ.
const RARE: &str = "zkphyre reconciliation";

fn corpus(size: usize) -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("store");
    let repos = store.repositories();
    let project = ProjectService::new(repos.clone())
        .create_project("Indexed", None, None, None)
        .expect("project");
    // Ordinary prose, so the index holds real bulk rather than the one rare row.
    for offset in 0..size {
        let body = if offset == size / 2 {
            format!("the {RARE} path needed a cursor and a dedup key")
        } else {
            format!("message {offset} about sync, replay, drift and the project map")
        };
        repos
            .search
            .index("message", &format!("m{offset}"), &project.id, "sync", &body)
            .expect("index");
    }
    (store, repos, project.id)
}

/// Seconds for `REPEATS` queries of the rare phrase, plus the hits the last one returned.
fn timed(repos: &Repositories, project_id: &str, query: &str) -> (f64, usize) {
    let filters = SearchFilters {
        project_id: Some(project_id.to_string()),
        kinds: Vec::new(),
        limit: 20,
    };
    let started = Instant::now();
    let mut hits = 0;
    for _ in 0..REPEATS {
        hits = repos.search.search(query, &filters).expect("search").len();
    }
    (started.elapsed().as_secs_f64(), hits)
}

#[test]
fn an_indexed_query_does_not_cost_more_because_the_workbench_grew() {
    let (_small, small_repos, small_project) = corpus(SMALL);
    let (_large, large_repos, large_project) = corpus(LARGE);

    let (small_cost, small_hits) = timed(&small_repos, &small_project, RARE);
    let (large_cost, large_hits) = timed(&large_repos, &large_project, RARE);

    // Correctness at scale first: the ratio means nothing if the answer is wrong.
    assert_eq!(small_hits, 1, "the rare row is findable in a small workbench");
    assert_eq!(large_hits, 1, "…and in a {LARGE}-row one, where a scan would find it too");

    let ratio = large_cost / small_cost.max(f64::EPSILON);
    assert!(
        ratio < RATIO_LIMIT,
        "{LARGE} rows cost {ratio:.1}x what {SMALL} rows cost ({large_cost:.3?} vs \
         {small_cost:.3?} for {REPEATS} queries) — that is a scan reading every row, not \
         §59's index"
    );

    // A substring of an indexed token is not a token: proof the query went through the
    // FTS index rather than a comparison that would have matched.
    let filters = SearchFilters {
        project_id: Some(large_project.clone()),
        kinds: Vec::new(),
        limit: 20,
    };
    assert!(
        large_repos
            .search
            .search("zkphyre reconcil", &filters)
            .expect("search")
            .is_empty(),
        "a piece of a word finds nothing, so this is an index and not a substring scan"
    );
}

/// §59 Indexed Search, the cheap half: the index is a *narrow* answer, not a page of
/// everything. A limit is honoured at volume, so no caller has to fetch a corpus to show a
/// screenful.
#[test]
fn a_query_at_volume_returns_what_it_was_asked_for_and_no_more() {
    let (_store, repos, project) = corpus(LARGE);
    let filters = SearchFilters {
        project_id: Some(project.clone()),
        kinds: Vec::new(),
        limit: 5,
    };
    let page = repos.search.search("sync", &filters).expect("search");
    assert_eq!(page.len(), 5, "the limit is a promise, not a hint");
    assert!(
        page.iter().all(|hit| hit.project_id == project),
        "and every hit belongs to the project that asked"
    );
    assert!(
        page.iter().all(|hit| !hit.snippet.trim().is_empty()),
        "a hit says something, rather than being a row id with a rank"
    );

    // One project's index cannot answer another's question, even with an identical corpus.
    let other = ProjectService::new(repos.clone())
        .create_project("Elsewhere", None, None, None)
        .expect("second project");
    let foreign = SearchFilters {
        project_id: Some(other.id.clone()),
        kinds: Vec::new(),
        limit: 5,
    };
    assert!(
        repos.search.search(RARE, &foreign).expect("search").is_empty(),
        "the second project has no rows indexed, so it must get nothing back"
    );
}
