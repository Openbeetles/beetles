use super::{
    build_search_snippet, contains_query_text, documents_search_match_kind,
    documents_search_match_score, DocumentsEntry, DocumentsSearchHit, DocumentsSearchQuery,
    PARTIAL_DOCUMENT_READ_WARNING,
};
use crate::error::Result;
use crate::office::OfficeHttpClient;
use std::collections::VecDeque;

/// Maximum number of search entries scanned before the helper stops.
pub const MAX_SEARCH_SCAN_ENTRIES: usize = 64;
/// Maximum bounded content read used by document search helpers.
pub const MAX_SEARCH_READ_BYTES: usize = 256 * 1024;
/// Warning emitted when a candidate is skipped because its content is too large.
pub const SEARCH_CONTENT_TOO_LARGE_WARNING: &str =
    "content not searched because the file is too large";

/// Result of a provider-specific search read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchReadOutcome {
    pub text: Option<String>,
    pub truncated: bool,
}

/// Shared scan and read limits for provider search helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchLimits {
    pub max_scan_entries: usize,
    pub max_read_bytes: usize,
}

/// Shared root folders and limits for breadth-first provider search.
pub struct SearchTreePlan<Folder> {
    pub initial_folders: Vec<Folder>,
    pub limits: SearchLimits,
}

impl SearchLimits {
    /// Builds a limit bundle for search traversal.
    pub fn new(max_scan_entries: usize, max_read_bytes: usize) -> Self {
        Self {
            max_scan_entries,
            max_read_bytes,
        }
    }
}

impl<Folder> SearchTreePlan<Folder> {
    /// Builds a tree traversal plan from initial folders and limits.
    pub fn new(initial_folders: impl IntoIterator<Item = Folder>, limits: SearchLimits) -> Self {
        Self {
            initial_folders: initial_folders.into_iter().collect(),
            limits,
        }
    }
}

impl SearchReadOutcome {
    /// Builds a search read result from decoded text and truncation state.
    pub fn new(text: Option<String>, truncated: bool) -> Self {
        Self { text, truncated }
    }
}

/// Computes the bounded read budget for search reads.
pub fn search_read_budget(max_read_bytes: usize) -> usize {
    if max_read_bytes == 0 {
        MAX_SEARCH_READ_BYTES
    } else {
        max_read_bytes.min(MAX_SEARCH_READ_BYTES)
    }
}

/// Runs scoped search over a candidate list while preserving source order.
pub fn search_scoped_candidates<Item, EntryFn, ReadFn, Iter>(
    http: &mut dyn OfficeHttpClient,
    query: &DocumentsSearchQuery,
    scope: &str,
    candidates: Iter,
    limits: SearchLimits,
    mut entry_of: EntryFn,
    mut read_content: ReadFn,
) -> Result<Vec<DocumentsSearchHit>>
where
    Iter: IntoIterator<Item = Item>,
    EntryFn: FnMut(&Item) -> DocumentsEntry,
    ReadFn: FnMut(
        &mut dyn OfficeHttpClient,
        &Item,
        &DocumentsEntry,
        usize,
    ) -> Result<SearchReadOutcome>,
{
    let mut hits = Vec::new();
    let mut scanned_entries = 0usize;
    for item in candidates {
        if search_budget_reached(
            scanned_entries,
            hits.len(),
            query.limit,
            limits.max_scan_entries,
        ) {
            break;
        }
        let entry = entry_of(&item);
        if !path_is_within_scope(&entry.path, scope) {
            continue;
        }
        scanned_entries += 1;
        if let Some(hit) = evaluate_search_entry(
            http,
            &item,
            entry,
            query,
            limits.max_read_bytes,
            &mut read_content,
        )? {
            hits.push(hit);
        }
    }
    finalize_search_hits(&mut hits, query.limit, SearchHitOrdering::Preserve);
    Ok(hits)
}

/// Runs breadth-first search over folder trees and sorts hits by specificity.
pub fn search_tree<Folder, Item, ListFn, EntryFn, NextFolderFn, ReadFn>(
    http: &mut dyn OfficeHttpClient,
    query: &DocumentsSearchQuery,
    plan: SearchTreePlan<Folder>,
    mut list_entries: ListFn,
    mut entry_of: EntryFn,
    mut next_folder: NextFolderFn,
    mut read_content: ReadFn,
) -> Result<Vec<DocumentsSearchHit>>
where
    Folder: Clone,
    ListFn: FnMut(&mut dyn OfficeHttpClient, &Folder) -> Result<Vec<Item>>,
    EntryFn: FnMut(&Folder, &Item) -> DocumentsEntry,
    NextFolderFn: FnMut(&Folder, &Item, &DocumentsEntry) -> Folder,
    ReadFn: FnMut(
        &mut dyn OfficeHttpClient,
        &Item,
        &DocumentsEntry,
        usize,
    ) -> Result<SearchReadOutcome>,
{
    let SearchTreePlan {
        initial_folders,
        limits,
    } = plan;
    let mut queue = initial_folders.into_iter().collect::<VecDeque<_>>();
    let mut hits = Vec::new();
    let mut scanned_entries = 0usize;

    while let Some(folder) = queue.pop_front() {
        if search_budget_reached(
            scanned_entries,
            hits.len(),
            query.limit,
            limits.max_scan_entries,
        ) {
            break;
        }
        let entries = list_entries(http, &folder)?;
        for item in entries {
            if search_budget_reached(
                scanned_entries,
                hits.len(),
                query.limit,
                limits.max_scan_entries,
            ) {
                break;
            }
            scanned_entries += 1;
            let entry = entry_of(&folder, &item);
            if entry.is_dir {
                if path_contains_query(&entry, query) {
                    hits.push(DocumentsSearchHit {
                        entry: entry.clone(),
                        match_kind: super::DOCUMENTS_SEARCH_MATCH_PATH.to_string(),
                        snippet: None,
                        warning: None,
                    });
                }
                queue.push_back(next_folder(&folder, &item, &entry));
                if search_budget_reached(
                    scanned_entries,
                    hits.len(),
                    query.limit,
                    limits.max_scan_entries,
                ) {
                    break;
                }
                continue;
            }

            if let Some(hit) = evaluate_search_entry(
                http,
                &item,
                entry,
                query,
                limits.max_read_bytes,
                &mut read_content,
            )? {
                hits.push(hit);
            }

            if search_budget_reached(
                scanned_entries,
                hits.len(),
                query.limit,
                limits.max_scan_entries,
            ) {
                break;
            }
        }
    }

    finalize_search_hits(
        &mut hits,
        query.limit,
        SearchHitOrdering::ByMatchKindThenPath,
    );
    Ok(hits)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchHitOrdering {
    Preserve,
    ByMatchKindThenPath,
}

fn evaluate_search_entry<Item, ReadFn>(
    http: &mut dyn OfficeHttpClient,
    item: &Item,
    entry: DocumentsEntry,
    query: &DocumentsSearchQuery,
    max_read_bytes: usize,
    read_content: &mut ReadFn,
) -> Result<Option<DocumentsSearchHit>>
where
    ReadFn: FnMut(
        &mut dyn OfficeHttpClient,
        &Item,
        &DocumentsEntry,
        usize,
    ) -> Result<SearchReadOutcome>,
{
    let path_hit = path_contains_query(&entry, query);
    let mut snippet = None;
    let mut warning = None;
    let mut content_hit = false;

    if !entry.is_dir {
        if entry
            .size_bytes
            .map(|size| size as usize <= max_read_bytes)
            .unwrap_or(true)
        {
            let read = read_content(http, item, &entry, max_read_bytes)?;
            if let Some(text) = read.text.as_deref() {
                if let Some(snippet_text) = search_snippet(text, &query.query, query.case_sensitive)
                {
                    content_hit = true;
                    snippet = Some(snippet_text);
                }
            }
            if read.truncated {
                warning = Some(PARTIAL_DOCUMENT_READ_WARNING.to_string());
            }
        } else {
            warning = Some(SEARCH_CONTENT_TOO_LARGE_WARNING.to_string());
        }
    }

    let Some(match_kind) = documents_search_match_kind(path_hit, content_hit) else {
        return Ok(None);
    };
    Ok(Some(DocumentsSearchHit {
        entry,
        match_kind: match_kind.to_string(),
        snippet,
        warning,
    }))
}

fn search_snippet(text: &str, query: &str, case_sensitive: bool) -> Option<String> {
    contains_query_text(text, query, case_sensitive)
        .then(|| build_search_snippet(text, query, case_sensitive))
}

fn path_contains_query(entry: &DocumentsEntry, query: &DocumentsSearchQuery) -> bool {
    contains_query_text(&entry.path, &query.query, query.case_sensitive)
        || contains_query_text(&entry.name, &query.query, query.case_sensitive)
}

fn search_budget_reached(
    scanned_entries: usize,
    hit_count: usize,
    limit: usize,
    max_scan_entries: usize,
) -> bool {
    scanned_entries >= max_scan_entries || hit_count >= limit
}

fn finalize_search_hits(
    hits: &mut Vec<DocumentsSearchHit>,
    limit: usize,
    ordering: SearchHitOrdering,
) {
    match ordering {
        SearchHitOrdering::Preserve => {}
        SearchHitOrdering::ByMatchKindThenPath => {
            hits.sort_by(|left, right| {
                documents_search_match_score(&right.match_kind)
                    .cmp(&documents_search_match_score(&left.match_kind))
                    .then_with(|| left.entry.path.cmp(&right.entry.path))
            });
        }
    }
    if hits.len() > limit {
        hits.truncate(limit);
    }
}

fn path_is_within_scope(path: &str, scope: &str) -> bool {
    let scope = normalize_relative_path(scope);
    if scope.is_empty() {
        true
    } else {
        let path = normalize_relative_path(path);
        path == scope || path.starts_with(&format!("{scope}/"))
    }
}

fn normalize_relative_path(path: &str) -> String {
    path.trim()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, is_dir: bool) -> DocumentsEntry {
        DocumentsEntry {
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            kind: if is_dir {
                "folder".to_string()
            } else {
                "file".to_string()
            },
            is_dir,
            content_type: None,
            size_bytes: Some(8),
        }
    }

    #[derive(Clone)]
    struct Candidate {
        label: &'static str,
        entry: DocumentsEntry,
    }

    #[test]
    fn scoped_candidates_preserve_backend_order() {
        let query = DocumentsSearchQuery {
            path: String::new(),
            query: "needle".to_string(),
            limit: 10,
            case_sensitive: false,
            max_read_bytes: 1024,
        };
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let hits = search_scoped_candidates(
            &mut http,
            &query,
            "",
            vec![
                Candidate {
                    label: "content",
                    entry: entry("notes/b.txt", false),
                },
                Candidate {
                    label: "path",
                    entry: entry("needle-a.txt", false),
                },
            ],
            SearchLimits::new(MAX_SEARCH_SCAN_ENTRIES, MAX_SEARCH_READ_BYTES),
            |item| item.entry.clone(),
            |_, item, _entry, _| {
                Ok(SearchReadOutcome::new(
                    Some(match item.label {
                        "content" => "prefix needle suffix".to_string(),
                        _ => "unrelated text".to_string(),
                    }),
                    false,
                ))
            },
        )
        .expect("search hits");

        assert_eq!(hits[0].entry.path, "notes/b.txt");
        assert_eq!(
            hits[0].match_kind,
            super::super::DOCUMENTS_SEARCH_MATCH_CONTENT
        );
        assert_eq!(hits[1].entry.path, "needle-a.txt");
        assert_eq!(
            hits[1].match_kind,
            super::super::DOCUMENTS_SEARCH_MATCH_PATH
        );
    }

    #[test]
    fn tree_search_sorts_hits_and_honors_scan_budget() {
        let query = DocumentsSearchQuery {
            path: String::new(),
            query: "needle".to_string(),
            limit: 10,
            case_sensitive: false,
            max_read_bytes: 1024,
        };
        let folders = vec!["root".to_string()];
        let mut read_calls = 0usize;
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let hits = search_tree(
            &mut http,
            &query,
            SearchTreePlan::new(folders, SearchLimits::new(2, MAX_SEARCH_READ_BYTES)),
            |_http, _| {
                Ok(vec![
                    entry("root/z-file.txt", false),
                    entry("root/needle-folder", true),
                    entry("root/a-file.txt", false),
                ])
            },
            |folder, item| {
                if item.is_dir {
                    entry(&format!("{folder}/{}", item.name), true)
                } else {
                    DocumentsEntry {
                        path: format!("{folder}/{}", item.name),
                        name: item.name.clone(),
                        kind: "file".to_string(),
                        is_dir: false,
                        content_type: None,
                        size_bytes: Some(8),
                    }
                }
            },
            |folder, item, _entry| format!("{folder}/{}", item.name),
            |_http, item, _entry, _| {
                read_calls += 1;
                Ok(SearchReadOutcome::new(
                    Some(match item.name.as_str() {
                        "z-file.txt" => "prefix needle suffix".to_string(),
                        "a-file.txt" => "needle".to_string(),
                        _ => String::new(),
                    }),
                    false,
                ))
            },
        )
        .expect("tree search");

        assert_eq!(read_calls, 1);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entry.path, "root/needle-folder");
        assert_eq!(
            hits[0].match_kind,
            super::super::DOCUMENTS_SEARCH_MATCH_PATH
        );
        assert_eq!(hits[1].entry.path, "root/z-file.txt");
        assert_eq!(
            hits[1].match_kind,
            super::super::DOCUMENTS_SEARCH_MATCH_CONTENT
        );
    }
}
