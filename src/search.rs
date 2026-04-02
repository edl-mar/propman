/// Fuzzy search over the merged key index.
///
/// Currently implemented as a simple case-insensitive substring filter.
/// TODO: replace the inner matcher with nucleo for scored fuzzy ranking.
pub struct Search {
    query: String,
    query_lower: String,
}

impl Search {
    pub fn new() -> Self {
        Self { query: String::new(), query_lower: String::new() }
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.query_lower = query.to_lowercase();
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Returns the indices (into `keys`) that match the current query.
    /// When the query is empty every index is returned.
    pub fn matching_indices(&self, keys: &[String]) -> Vec<usize> {
        if self.query_lower.is_empty() {
            return (0..keys.len()).collect();
        }
        keys.iter()
            .enumerate()
            .filter(|(_, k)| k.to_lowercase().contains(&self.query_lower))
            .map(|(i, _)| i)
            .collect()
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}
