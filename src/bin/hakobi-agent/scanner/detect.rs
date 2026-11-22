use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};
use hakobi::{
    github::{example_file_path, parse_example_environment, parse_exposed_port},
    host_agent::PublicSourceApp,
    public_source::validate_app_path,
};

const MAX_APPS: usize = 16;
const MAX_DIRECTORIES: usize = 512;
const MAX_FILES: usize = 4096;
const MAX_DEPTH: usize = 8;
const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_PATH_BYTES: usize = 512;
const NIXPACKS_MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "Gemfile",
    "Package.swift",
    "Pipfile",
    "bb.edn",
    "build.gradle",
    "build.gradle.kts",
    "build.sbt",
    "build.zig",
    "composer.json",
    "deno.json",
    "deno.jsonc",
    "deps.edn",
    "go.mod",
    "mix.exs",
    "nixpacks.json",
    "nixpacks.toml",
    "package.json",
    "package.yaml",
    "pom.xml",
    "project.clj",
    "pubspec.yaml",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "shard.yml",
    "stack.yaml",
];

#[derive(Default)]
struct Candidates {
    dockerfiles: BTreeMap<String, PathBuf>,
    nixpacks: BTreeSet<String>,
    directories: usize,
    files: usize,
}

pub(super) fn apps(root: &Path) -> Result<Vec<PublicSourceApp>> {
    let root = root.canonicalize().context("canonicalize checkout")?;
    let mut candidates = Candidates::default();
    visit_directory(&root, &root, 0, &mut candidates)?;
    let mut apps = Vec::with_capacity(MAX_APPS);
    for (path, dockerfile) in &candidates.dockerfiles {
        if apps.len() == MAX_APPS {
            break;
        }
        apps.push(docker_app(&root, path, dockerfile)?);
    }
    for path in &candidates.nixpacks {
        if apps.len() == MAX_APPS {
            break;
        }
        if !candidates.dockerfiles.contains_key(path) {
            apps.push(nixpacks_app(&root, path)?);
        }
    }
    Ok(apps)
}

fn visit_directory(
    root: &Path,
    directory: &Path,
    depth: usize,
    candidates: &mut Candidates,
) -> Result<()> {
    ensure!(depth <= MAX_DEPTH, "repository nesting exceeds {MAX_DEPTH}");
    candidates.directories += 1;
    ensure!(
        candidates.directories <= MAX_DIRECTORIES,
        "repository has more than {MAX_DIRECTORIES} directories"
    );
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if entry.file_name() != ".git" {
                visit_directory(root, &entry.path(), depth + 1, candidates)?;
            }
        } else if file_type.is_file() {
            candidates.files += 1;
            ensure!(
                candidates.files <= MAX_FILES,
                "repository has too many files"
            );
            classify_file(root, &entry.path(), candidates)?;
        }
    }
    Ok(())
}

fn classify_file(root: &Path, file: &Path, candidates: &mut Candidates) -> Result<()> {
    let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
        return Ok(());
    };
    let is_dockerfile = name == "Dockerfile";
    if !is_dockerfile && !is_nixpacks_manifest(name) {
        return Ok(());
    }
    let path = relative_path(root, file.parent().context("file has no parent")?)?;
    if is_dockerfile {
        candidates.dockerfiles.insert(path, file.to_owned());
    } else {
        candidates.nixpacks.insert(path);
    }
    Ok(())
}

fn is_nixpacks_manifest(name: &str) -> bool {
    NIXPACKS_MANIFESTS.contains(&name)
        || name.ends_with(".csproj")
        || name.ends_with(".fsproj")
        || name.ends_with(".sln")
}

fn relative_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root)?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            bail!("checkout path contains a non-normal component");
        };
        parts.push(part.to_str().context("app path is not UTF-8")?);
    }
    let path = parts.join("/");
    ensure!(path.len() <= MAX_PATH_BYTES, "app path exceeds 512 bytes");
    validate_app_path(&path)?;
    Ok(path)
}

fn docker_app(root: &Path, path: &str, dockerfile: &Path) -> Result<PublicSourceApp> {
    let dockerfile_path = example_file_path(path, "Dockerfile");
    ensure!(
        dockerfile_path.len() <= MAX_PATH_BYTES,
        "Dockerfile path exceeds 512 bytes"
    );
    let text = read_text(root, dockerfile)?;
    let (environment, environment_found) = example_environment(root, path)?;
    Ok(PublicSourceApp {
        path: path.to_owned(),
        dockerfile: Some(dockerfile_path),
        build_type: "dockerfile".to_owned(),
        container_port: parse_exposed_port(&text),
        environment,
        environment_found,
    })
}

fn nixpacks_app(root: &Path, path: &str) -> Result<PublicSourceApp> {
    let (environment, environment_found) = example_environment(root, path)?;
    Ok(PublicSourceApp {
        path: path.to_owned(),
        dockerfile: None,
        build_type: "nixpacks".to_owned(),
        container_port: None,
        environment,
        environment_found,
    })
}

fn example_environment(root: &Path, app_path: &str) -> Result<(BTreeMap<String, String>, bool)> {
    for filename in [".env.example", ".env.sample"] {
        let path = root.join(example_file_path(app_path, filename));
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        let text = read_text(root, &path)?;
        return Ok((parse_example_environment(&text), true));
    }
    Ok((BTreeMap::new(), false))
}

fn read_text(root: &Path, path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_file(),
        "app file is not a regular file"
    );
    ensure!(
        metadata.len() <= MAX_FILE_BYTES as u64,
        "app file is too large"
    );
    let canonical = path.canonicalize()?;
    ensure!(canonical.starts_with(root), "app file escapes the checkout");
    let mut bytes = Vec::new();
    fs::File::open(canonical)?
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_FILE_BYTES, "app file is too large");
    String::from_utf8(bytes).context("app file is not UTF-8")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    fn write(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn docker_apps_precede_nixpacks_and_parse_metadata() {
        let checkout = tempfile::tempdir().unwrap();
        write(
            checkout.path(),
            "Dockerfile",
            "FROM alpine\nEXPOSE 3000/tcp\n",
        );
        write(
            checkout.path(),
            ".env.example",
            "ROOT_URL=https://example.test\n",
        );
        write(checkout.path(), ".env.sample", "IGNORED=yes\n");
        write(
            checkout.path(),
            "services/worker/Dockerfile",
            "EXPOSE 4000\n",
        );
        write(checkout.path(), "services/worker/package.json", "{}\n");
        write(checkout.path(), "services/api/package.json", "{}\n");
        write(
            checkout.path(),
            "services/api/.env.sample",
            "export API_TOKEN=secret\n",
        );
        let apps = super::apps(checkout.path()).unwrap();
        assert_eq!(apps.len(), 3);
        assert_eq!(apps[0].path, "");
        assert_eq!(apps[0].container_port, Some(3000));
        assert_eq!(apps[0].environment.get("ROOT_URL"), Some(&String::new()));
        assert_eq!(apps[1].path, "services/worker");
        assert_eq!(apps[1].build_type, "dockerfile");
        assert_eq!(apps[2].path, "services/api");
        assert_eq!(apps[2].build_type, "nixpacks");
        assert_eq!(apps[2].dockerfile, None);
        assert!(apps[2].environment.contains_key("API_TOKEN"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_expose_files_outside_the_checkout() {
        use std::os::unix::fs::symlink;
        let checkout = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "secret.env", "SECRET=outside\n");
        write(checkout.path(), "package.json", "{}\n");
        symlink(
            outside.path().join("secret.env"),
            checkout.path().join(".env.example"),
        )
        .unwrap();
        symlink(outside.path(), checkout.path().join("linked")).unwrap();
        let apps = super::apps(checkout.path()).unwrap();
        assert_eq!(apps.len(), 1);
        assert!(!apps[0].environment_found);
        assert!(apps[0].environment.is_empty());
    }

    #[test]
    fn app_limit_preserves_dockerfile_priority() {
        let checkout = tempfile::tempdir().unwrap();
        write(checkout.path(), "package.json", "{}\n");
        for index in 0..20 {
            write(
                checkout.path(),
                &format!("apps/{index:02}/Dockerfile"),
                "FROM scratch\n",
            );
        }
        let apps = super::apps(checkout.path()).unwrap();
        assert_eq!(apps.len(), 16);
        assert!(apps.iter().all(|app| app.build_type == "dockerfile"));
    }
}
