//! Shared server-side pagination for SSR lists.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct Page<'a, T> {
    pub items: &'a [T],
    pub number: usize,
    pub count: usize,
    pub total: usize,
    pub start: usize,
}

pub(crate) fn paginate<T>(items: &[T], requested: usize, per_page: usize) -> Page<'_, T> {
    let total = items.len();
    let count = total.div_ceil(per_page).max(1);
    let number = requested.clamp(1, count);
    let start = (number - 1) * per_page;
    let end = start.saturating_add(per_page).min(total);
    Page {
        items: &items[start.min(total)..end],
        number,
        count,
        total,
        start,
    }
}

pub(crate) fn parse_query(raw: Option<&str>) -> HashMap<String, String> {
    raw.map(|raw| {
        url::form_urlencoded::parse(raw.as_bytes())
            .into_owned()
            .collect()
    })
    .unwrap_or_default()
}

pub(crate) fn page_number(query: &HashMap<String, String>, key: &str) -> usize {
    query
        .get(key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn pages_slice_and_clamp_real_rows() {
        let rows: Vec<usize> = (0..55).collect();
        let first = super::paginate(&rows, 1, 25);
        assert_eq!(first.items, &rows[..25]);
        assert_eq!(first.count, 3);
        let last = super::paginate(&rows, 99, 25);
        assert_eq!(last.number, 3);
        assert_eq!(last.start, 50);
        assert_eq!(last.items, &rows[50..]);
        assert_eq!(last.total, 55);
    }

    #[test]
    fn empty_and_invalid_pages_remain_stable() {
        let empty = super::paginate::<usize>(&[], 0, 25);
        assert_eq!(empty.number, 1);
        assert!(empty.items.is_empty());
        assert_eq!(empty.total, 0);
        let query = super::parse_query(Some("images=bad&tab=images"));
        assert_eq!(super::page_number(&query, "images"), 1);
    }
}
