use anyhow::{bail, Result};

const MAX_IMAGE_LEN: usize = 255;
const MAX_NAME_LEN: usize = 128;

pub(in crate::providers::docker_exec) fn validate_image_reference(image: &str) -> Result<()> {
    if image.is_empty() || image.len() > MAX_IMAGE_LEN {
        bail!("image reference must be 1–{MAX_IMAGE_LEN} characters");
    }
    if image
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        bail!("image reference must not contain whitespace or control characters");
    }
    let mut characters = image.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("image reference must start with a letter or digit"),
    }
    if !image.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/' | ':' | '@')
    }) {
        bail!("image reference contains unsupported characters");
    }
    if image.contains("..") || image.contains("//") || image.contains("@@") {
        bail!("image reference contains an invalid separator sequence");
    }
    Ok(())
}

pub(in crate::providers::docker_exec) fn validate_container_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        bail!("container name must be 1–{MAX_NAME_LEN} characters");
    }
    match name.chars().next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("container name must start with a letter or digit"),
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        bail!("container name may only contain letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

pub(in crate::providers::docker_exec) fn validate_container_port(
    container_port: u16,
) -> Result<()> {
    if container_port == 0 {
        bail!("container port must be between 1 and 65535");
    }
    Ok(())
}
