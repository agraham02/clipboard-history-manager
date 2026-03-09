use std::collections::VecDeque;

use sublime_fuzzy::best_match;

use crate::clipboard::ClipboardEntry;

const MAX_HISTORY_ITEMS: usize = 50;

/// Bounded, deduplicated clipboard history with fuzzy search.
pub struct ClipboardHistory {
    items: VecDeque<ClipboardEntry>,
}

impl ClipboardHistory {
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
        }
    }

    /// Push a new entry. Removes older duplicate if present, trims to max size.
    pub fn push(&mut self, entry: ClipboardEntry) {
        if let Some(pos) = self.items.iter().position(|e| e == &entry) {
            self.items.remove(pos);
        }
        self.items.push_front(entry);
        while self.items.len() > MAX_HISTORY_ITEMS {
            self.items.pop_back();
        }
    }

    pub fn get(&self, index: usize) -> Option<&ClipboardEntry> {
        self.items.get(index)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Return all items (unfiltered), as (index, entry) pairs.
    #[allow(dead_code)]
    pub fn all(&self) -> Vec<(usize, &ClipboardEntry)> {
        self.items.iter().enumerate().collect()
    }

    /// Fuzzy search across entries. Returns (original_index, entry, matched_indices) sorted by score.
    pub fn search(&self, query: &str) -> Vec<(usize, &ClipboardEntry, Vec<usize>)> {
        if query.is_empty() {
            return self
                .items
                .iter()
                .enumerate()
                .map(|(i, e)| (i, e, Vec::new()))
                .collect();
        }

        let mut scored: Vec<(usize, &ClipboardEntry, i64, Vec<usize>)> = Vec::new();

        for (idx, entry) in self.items.iter().enumerate() {
            let haystack = entry.searchable_text();
            if let Some(m) = best_match(query, haystack) {
                scored.push((idx, entry, m.score() as i64, m.matched_indices().cloned().collect()));
            }
        }

        scored.sort_by(|a, b| b.2.cmp(&a.2));
        scored.into_iter().map(|(i, e, _, indices)| (i, e, indices)).collect()
    }

    /// Move an entry at `index` to the front of the history.
    pub fn promote(&mut self, index: usize) {
        if index > 0 && index < self.items.len() {
            if let Some(entry) = self.items.remove(index) {
                self.items.push_front(entry);
            }
        }
    }
}
