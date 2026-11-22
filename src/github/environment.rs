//! Safe parsing for example environment files and Dockerfile EXPOSE lines.

use std::collections::BTreeMap;

pub fn example_file_path(app_path: &str, filename: &str) -> String {
    if app_path.is_empty() {
        filename.to_owned()
    } else {
        format!("{app_path}/{filename}")
    }
}

pub(super) fn parse_exposed_port(dockerfile: &str) -> Option<u16> {
    dockerfile.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if !parts
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("EXPOSE"))
        {
            return None;
        }
        parts
            .next()?
            .split('/')
            .next()?
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
    })
}

pub fn parse_example_environment(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .map(|line| line.strip_prefix("export ").unwrap_or(line))
        .filter_map(|line| line.split_once('='))
        .map(|(key, _)| key.trim())
        .filter(|key| valid_environment_key(key))
        .map(|key| (key.to_owned(), String::new()))
        .collect()
}

fn valid_environment_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dockerfile_expose_sets_internal_port() {
        assert_eq!(
            super::parse_exposed_port("FROM python\nEXPOSE 3000/tcp\n"),
            Some(3000)
        );
        assert_eq!(super::parse_exposed_port("FROM scratch\n"), None);
    }

    #[test]
    fn example_environment_returns_keys_without_values() {
        let vars = super::parse_example_environment(
            "# comment\nAPI_URL=https://example.test\nexport TOKEN=secret\nINVALID-NAME=x\n",
        );
        assert_eq!(vars.keys().collect::<Vec<_>>(), ["API_URL", "TOKEN"]);
        assert!(vars.values().all(String::is_empty));
    }
}
