//! Shared page totals and previous/next links for SSR lists.

use std::collections::HashMap;

use maud::{html, Markup};

use crate::web::pagination::Page;

pub(crate) fn pagination_controls<T>(
    base: &str,
    query: &HashMap<String, String>,
    key: &str,
    page: &Page<'_, T>,
    tab: Option<&str>,
) -> Markup {
    let first = if page.total == 0 { 0 } else { page.start + 1 };
    let last = first + page.items.len().saturating_sub(1);
    html! {
        nav class="mt-3 flex flex-wrap items-center justify-between gap-3 text-xs text-muted" aria-label="Pagination" {
            span { "Showing " (first) "–" (last) " of " (page.total) " · Page " (page.number) " of " (page.count) }
            @if page.count > 1 {
                div class="flex gap-2" {
                    @if page.number > 1 {
                        a class="rounded-md border border-line px-2.5 py-1 font-semibold no-underline" href=(page_url(base, query, key, page.number - 1, tab)) { "← Previous" }
                    }
                    @if page.number < page.count {
                        a class="rounded-md border border-line px-2.5 py-1 font-semibold no-underline" href=(page_url(base, query, key, page.number + 1, tab)) { "Next →" }
                    }
                }
            }
        }
    }
}

fn page_url(
    base: &str,
    query: &HashMap<String, String>,
    key: &str,
    page: usize,
    tab: Option<&str>,
) -> String {
    let mut values = query.clone();
    values.remove("notice");
    values.insert(key.into(), page.to_string());
    if let Some(tab) = tab {
        values.insert("tab".into(), tab.into());
    }
    let encoded = values
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{encoded}")
}
