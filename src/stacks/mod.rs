//! Stack definitions: the declarative description behind a deployable stack.
//!
//! A stack always belongs to a provider connection. Compose and Kubernetes
//! stacks carry YAML and are syntax-checked before they are persisted;
//! Dockerfile and Nixpacks stacks carry build descriptions and are stored
//! verbatim.

use std::fmt;
use std::str::FromStr;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

pub mod docker_exec;

/// The build language a stack definition is written in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StackKind {
    Dockerfile,
    Compose,
    Kubernetes,
    Nixpacks,
}

impl StackKind {
    /// Stable database representation of the kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dockerfile => "dockerfile",
            Self::Compose => "compose",
            Self::Kubernetes => "kubernetes",
            Self::Nixpacks => "nixpacks",
        }
    }

    /// Whether the stack body must be a well-formed YAML document.
    pub fn requires_yaml(&self) -> bool {
        matches!(self, Self::Compose | Self::Kubernetes)
    }
}

impl fmt::Display for StackKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for StackKind {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "dockerfile" => Ok(Self::Dockerfile),
            "compose" => Ok(Self::Compose),
            "kubernetes" => Ok(Self::Kubernetes),
            "nixpacks" => Ok(Self::Nixpacks),
            other => bail!("unsupported stack kind: {other}"),
        }
    }
}

/// A persisted stack definition bound to a provider connection.
#[derive(Debug, Clone)]
pub struct StackDefinition {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub kind: StackKind,
    pub source: String,
    pub yaml: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Validates the body of a stack before it is written to the catalog.
pub(crate) fn validate_definition(kind: &StackKind, yaml: &str) -> Result<()> {
    if kind.requires_yaml() {
        validate_yaml(yaml)?;
    }
    Ok(())
}

/// Lightweight YAML syntax check for Compose and Kubernetes manifests.
///
/// This deliberately does not build a document tree or resolve aliases; it
/// verifies that indentation, block structure, quoting and flow collections
/// are well formed so malformed documents are rejected before persistence.
fn validate_yaml(source: &str) -> Result<()> {
    let lines = normalize(source)?;
    let documents = split_documents(&lines);
    if documents.is_empty() {
        bail!("YAML document is empty");
    }
    for document in documents {
        parse_document(document)?;
    }
    Ok(())
}

#[derive(Debug)]
struct Line {
    indent: usize,
    text: String,
    number: usize,
}

/// Splits the source into structural lines, expanding compact sequence
/// entries (`- key: value`) into a dash line plus an indented child line and
/// dropping block-scalar bodies, whose contents are opaque.
fn normalize(source: &str) -> Result<Vec<Line>> {
    let mut lines = Vec::new();
    let mut block_indent: Option<usize> = None;

    for (offset, raw) in source.lines().enumerate() {
        let number = offset + 1;
        let raw = raw.trim_end();

        if let Some(indent) = block_indent {
            let spaces = raw
                .chars()
                .take_while(|character| *character == ' ')
                .count();
            if raw.trim().is_empty() || spaces > indent {
                continue;
            }
            block_indent = None;
        }
        if raw.trim().is_empty() {
            continue;
        }

        let indent = leading_indent(raw, number)?;
        let content = &raw[indent..];
        if content.starts_with('#') {
            continue;
        }
        if indent == 0 {
            let marker = strip_comment(content).trim();
            if marker == "..." {
                continue;
            }
            if marker == "---" {
                lines.push(Line {
                    indent: 0,
                    text: "---".to_string(),
                    number,
                });
                continue;
            }
            if marker.starts_with('%') {
                continue;
            }
        }

        let mut current_indent = indent;
        let mut owner_indent = indent;
        let mut text = content.to_string();
        loop {
            if text == "-" {
                lines.push(Line {
                    indent: current_indent,
                    text: "-".to_string(),
                    number,
                });
                text.clear();
                break;
            }
            if let Some(rest) = text.strip_prefix("- ") {
                lines.push(Line {
                    indent: current_indent,
                    text: "-".to_string(),
                    number,
                });
                owner_indent = current_indent;
                let trimmed = rest.trim_start_matches(' ');
                let extra = rest.len() - trimmed.len();
                current_indent += 2 + extra;
                text = trimmed.to_string();
                if text.is_empty() || text.starts_with('#') {
                    text.clear();
                    break;
                }
                continue;
            }
            break;
        }
        if text.is_empty() {
            continue;
        }
        if text.starts_with('\t') || text.starts_with("-\t") {
            bail!("line {number}: tabs are not allowed for indentation");
        }

        let starts_block = block_indicator_at(&text);
        let block_owner = if split_mapping_key(strip_comment(&text)).is_some() {
            current_indent
        } else {
            owner_indent
        };
        lines.push(Line {
            indent: current_indent,
            text,
            number,
        });
        if starts_block {
            block_indent = Some(block_owner);
        }
    }

    Ok(lines)
}

fn leading_indent(raw: &str, number: usize) -> Result<usize> {
    let mut indent = 0;
    for character in raw.chars() {
        match character {
            ' ' => indent += 1,
            '\t' => bail!("line {number}: tabs are not allowed for indentation"),
            _ => break,
        }
    }
    Ok(indent)
}

fn split_documents(lines: &[Line]) -> Vec<&[Line]> {
    let mut documents = Vec::new();
    let mut start = 0;
    for (index, line) in lines.iter().enumerate() {
        if line.indent == 0 && line.text == "---" {
            if index > start {
                documents.push(&lines[start..index]);
            }
            start = index + 1;
        }
    }
    if start < lines.len() {
        documents.push(&lines[start..]);
    }
    documents
}

fn parse_document(lines: &[Line]) -> Result<()> {
    let mut index = 0;
    let indent = lines[0].indent;
    parse_block(lines, &mut index, indent)?;
    if index < lines.len() {
        bail!("line {}: unexpected content", lines[index].number);
    }
    Ok(())
}

fn parse_block(lines: &[Line], index: &mut usize, indent: usize) -> Result<()> {
    let line = &lines[*index];
    if line.indent != indent {
        bail!("line {}: unexpected indentation", line.number);
    }
    let text = strip_comment(&line.text).trim_end();

    if text == "-" {
        return parse_sequence(lines, index, indent, false);
    }
    if is_property_only(text) {
        *index += 1;
        if *index < lines.len() && lines[*index].indent > indent {
            let child = lines[*index].indent;
            return parse_block(lines, index, child);
        }
        return Ok(());
    }
    if split_mapping_key(text).is_some() {
        return parse_mapping(lines, index, indent);
    }

    validate_scalar(text).map_err(|error| anyhow!("line {}: {error:#}", line.number))?;
    *index += 1;
    if *index < lines.len() && lines[*index].indent > indent {
        bail!("line {}: unexpected indentation", lines[*index].number);
    }
    Ok(())
}

fn parse_mapping(lines: &[Line], index: &mut usize, indent: usize) -> Result<()> {
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            bail!("line {}: unexpected indentation", line.number);
        }

        let text = strip_comment(&line.text).trim_end();
        if text == "-" {
            bail!("line {}: unexpected sequence entry", line.number);
        }
        let Some((key, value)) = split_mapping_key(text) else {
            bail!("line {}: expected a mapping entry", line.number);
        };
        if key.trim().is_empty() {
            bail!("line {}: mapping key must not be empty", line.number);
        }

        *index += 1;
        if value_opens_block(value) {
            if *index < lines.len() && lines[*index].indent > indent {
                let child = lines[*index].indent;
                parse_block(lines, index, child)?;
            } else if *index < lines.len()
                && lines[*index].indent == indent
                && strip_comment(&lines[*index].text).trim_end() == "-"
            {
                // YAML permits a block sequence to sit at the same indentation
                // as the mapping key that owns it.
                parse_sequence(lines, index, indent, true)?;
            }
        } else {
            validate_scalar(value).map_err(|error| anyhow!("line {}: {error:#}", line.number))?;
            if *index < lines.len() && lines[*index].indent > indent {
                bail!("line {}: unexpected indentation", lines[*index].number);
            }
        }
    }
    Ok(())
}

fn parse_sequence(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
    indentless: bool,
) -> Result<()> {
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            bail!("line {}: unexpected indentation", line.number);
        }
        if strip_comment(&line.text).trim_end() != "-" {
            if indentless {
                break;
            }
            bail!("line {}: expected a sequence entry", line.number);
        }

        *index += 1;
        if *index < lines.len() && lines[*index].indent > indent {
            let child = lines[*index].indent;
            parse_block(lines, index, child)?;
        }
    }
    Ok(())
}

/// True when a mapping value has no inline content and therefore owns the
/// following indented block (or indentless sequence).
fn value_opens_block(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || is_property_only(value)
}

/// True when the text consists only of YAML node properties (anchors/tags).
fn is_property_only(text: &str) -> bool {
    let mut rest = text.trim();
    if rest.is_empty() {
        return false;
    }
    loop {
        let Some(first) = rest.chars().next() else {
            return true;
        };
        if first == '&' || first == '!' {
            if first == '!' && rest.starts_with("!<") {
                match rest.find('>') {
                    Some(end) => rest = rest[end + 1..].trim_start(),
                    None => return true,
                }
            } else {
                match rest[1..].find(char::is_whitespace) {
                    Some(offset) => rest = rest[offset + 1..].trim_start(),
                    None => return true,
                }
            }
        } else {
            return false;
        }
    }
}

fn skip_properties(mut rest: &str) -> Result<&str> {
    loop {
        let Some(first) = rest.chars().next() else {
            return Ok(rest);
        };
        match first {
            '&' | '!' => {
                if first == '!' && rest.starts_with("!<") {
                    match rest.find('>') {
                        Some(end) => rest = rest[end + 1..].trim_start(),
                        None => bail!("unterminated tag"),
                    }
                } else {
                    match rest[1..].find(char::is_whitespace) {
                        Some(offset) => rest = rest[offset + 1..].trim_start(),
                        None => return Ok(""),
                    }
                }
            }
            _ => return Ok(rest),
        }
    }
}

fn validate_scalar(value: &str) -> Result<()> {
    let rest = value.trim();
    if rest.is_empty() {
        return Ok(());
    }
    let rest = skip_properties(rest)?;
    if rest.is_empty() {
        return Ok(());
    }
    if rest.starts_with('[') || rest.starts_with('{') {
        return validate_flow(rest);
    }
    if rest.starts_with('"') || rest.starts_with('\'') {
        return validate_quoted(rest);
    }
    if rest.starts_with('@') || rest.starts_with('`') {
        bail!("reserved indicator at the start of a scalar");
    }
    if rest.starts_with(',') || rest.starts_with(']') || rest.starts_with('}') {
        bail!("unexpected flow indicator at the start of a scalar");
    }
    Ok(())
}

fn validate_quoted(value: &str) -> Result<()> {
    let end = if value.as_bytes()[0] == b'"' {
        skip_double_quoted(value, 0)?
    } else {
        skip_single_quoted(value, 0)?
    };
    if !value[end..].trim().is_empty() {
        bail!("unexpected text after quoted scalar");
    }
    Ok(())
}

fn skip_double_quoted(value: &str, start: usize) -> Result<usize> {
    let bytes = value.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                if index + 1 >= bytes.len() {
                    bail!("unterminated double-quoted scalar");
                }
                index += 2;
            }
            b'"' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    bail!("unterminated double-quoted scalar")
}

fn skip_single_quoted(value: &str, start: usize) -> Result<usize> {
    let bytes = value.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\'' {
            if bytes.get(index + 1) == Some(&b'\'') {
                index += 2;
            } else {
                return Ok(index + 1);
            }
        } else {
            index += 1;
        }
    }
    bail!("unterminated single-quoted scalar")
}

fn validate_flow(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let mut stack = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_double_quoted(value, index)?,
            b'\'' => index = skip_single_quoted(value, index)?,
            b'[' | b'{' => {
                stack.push(bytes[index]);
                index += 1;
            }
            b']' | b'}' => {
                let opening = if bytes[index] == b']' { b'[' } else { b'{' };
                if stack.pop() != Some(opening) {
                    bail!("mismatched flow collection delimiter");
                }
                index += 1;
                if stack.is_empty() {
                    if !value[index..].trim().is_empty() {
                        bail!("unexpected text after flow collection");
                    }
                    return Ok(());
                }
            }
            b'#' if index == 0 || bytes[index - 1].is_ascii_whitespace() => {
                bail!("unexpected comment inside flow collection");
            }
            _ => index += 1,
        }
    }

    if stack.is_empty() {
        bail!("expected a flow collection");
    }
    bail!("unclosed flow collection")
}

fn is_block_indicator(value: &str) -> bool {
    let mut characters = value.chars();
    match characters.next() {
        Some('|') | Some('>') => {}
        _ => return false,
    }
    characters.all(|character| {
        character == '+' || character == '-' || character.is_ascii_digit() || character == ' '
    })
}

fn block_indicator_at(text: &str) -> bool {
    let text = strip_comment(text).trim_end();
    if let Some((_, value)) = split_mapping_key(text) {
        let value = value.trim();
        let value = skip_properties(value).unwrap_or(value);
        return is_block_indicator(value.trim());
    }
    let text = text.trim();
    let text = skip_properties(text).unwrap_or(text);
    is_block_indicator(text.trim())
}

/// Removes a trailing comment that starts outside a quoted scalar.
fn strip_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut quote: Option<u8> = None;

    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(b'"') => {
                if byte == b'\\' {
                    index += 2;
                    continue;
                }
                if byte == b'"' {
                    quote = None;
                }
            }
            Some(_) => {
                if byte == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 2;
                        continue;
                    }
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'#' if index == 0 || bytes[index - 1].is_ascii_whitespace() => {
                    return &text[..index];
                }
                _ => {}
            },
        }
        index += 1;
    }

    text
}

/// Splits a block mapping line into its key and value. Only a colon followed
/// by whitespace (or the end of the line) terminates a plain or quoted key.
fn split_mapping_key(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;

    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(current) = quote {
            if current == b'"' {
                if byte == b'\\' {
                    index += 2;
                    continue;
                }
                if byte == b'"' {
                    quote = None;
                }
            } else if byte == b'\'' {
                if bytes.get(index + 1) == Some(&b'\'') {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'[' | b'{' => depth += 1,
            b']' | b'}' => depth -= 1,
            b':' if depth == 0
                && (index + 1 == bytes.len() || bytes[index + 1].is_ascii_whitespace()) =>
            {
                return Some((&text[..index], &text[index + 1..]));
            }
            _ => {}
        }
        index += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_compose_and_kubernetes_documents() {
        let compose =
            "services:\n  web:\n    image: nginx:latest\n    ports:\n      - \"8080:80\"\n";
        assert!(validate_definition(&StackKind::Compose, compose).is_ok());

        let kubernetes = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  template:\n    spec:\n      containers:\n      - name: web\n        image: nginx\n";
        assert!(validate_definition(&StackKind::Kubernetes, kubernetes).is_ok());
    }

    #[test]
    fn rejects_malformed_yaml() {
        assert!(validate_definition(&StackKind::Compose, "services: [web\n").is_err());
        assert!(validate_definition(
            &StackKind::Compose,
            "services:\n  web:\n    image: nginx\n   broken: indent\n"
        )
        .is_err());
        assert!(validate_definition(&StackKind::Compose, "image: \"unterminated\n").is_err());
        assert!(validate_definition(&StackKind::Kubernetes, "metadata: {name: web\n").is_err());
        assert!(validate_definition(&StackKind::Compose, "").is_err());
    }

    #[test]
    fn accepts_dockerfile_text_without_yaml_validation() {
        let dockerfile = "FROM rust:1.88\nRUN echo 'hello: [world'\n";
        assert!(validate_definition(&StackKind::Dockerfile, dockerfile).is_ok());
    }

    #[test]
    fn stack_kinds_round_trip_through_strings() {
        for kind in [
            StackKind::Dockerfile,
            StackKind::Compose,
            StackKind::Kubernetes,
            StackKind::Nixpacks,
        ] {
            assert_eq!(kind.as_str().parse::<StackKind>().unwrap(), kind);
        }
    }
}
