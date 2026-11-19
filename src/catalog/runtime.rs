use std::collections::HashSet;

use anyhow::{bail, Result};
use rusqlite::params;

use super::Catalog;

const MAX_COMMAND_LEN: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppVolume {
    pub name: String,
    pub mount_path: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppProcess {
    pub name: String,
    pub command: String,
}

pub fn worker_container_name(app_id: &str, name: &str) -> String {
    format!("hakobi-{app_id}-worker-{name}")
}

pub fn validate_command(command: Option<&str>) -> Result<()> {
    let Some(command) = command else {
        return Ok(());
    };
    if command.len() > MAX_COMMAND_LEN {
        bail!("commands must be at most {MAX_COMMAND_LEN} bytes");
    }
    if command.trim().is_empty() || command.contains('\0') {
        bail!("commands must be non-empty and contain no NUL bytes");
    }
    Ok(())
}

pub fn volume_name_for(app_id: &str, name: &str) -> String {
    format!("hakobi-{app_id}-{name}")
}

fn validate_volume(volume: &AppVolume) -> Result<()> {
    if volume.name.is_empty()
        || volume.name.len() > 64
        || !volume.name.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !volume
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("volume names must be 1-64 ASCII letters, digits, '-', '_', or '.'");
    }
    let path = std::path::Path::new(&volume.mount_path);
    if !path.is_absolute()
        || volume.mount_path == "/"
        || volume.mount_path.contains(['\0', ':'])
        || path.components().any(|part| {
            !matches!(
                part,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
    {
        bail!("volume mount paths must be absolute, clean destinations below '/'");
    }
    Ok(())
}

fn validate_volumes(volumes: &[AppVolume]) -> Result<()> {
    let mut names = HashSet::new();
    let mut paths = HashSet::new();
    for volume in volumes {
        validate_volume(volume)?;
        if !names.insert(&volume.name) || !paths.insert(&volume.mount_path) {
            bail!("volume names and mount paths must be unique per app");
        }
    }
    Ok(())
}

fn validate_processes(processes: &[AppProcess]) -> Result<()> {
    let mut names = HashSet::new();
    for process in processes {
        if process.name.is_empty()
            || process.name.len() > 32
            || !process
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            bail!("process names must be 1-32 lowercase letters, digits, or '-'");
        }
        validate_command(Some(&process.command))?;
        if !names.insert(&process.name) || process.name == "web" {
            bail!("process names must be unique and cannot be 'web'");
        }
    }
    Ok(())
}

pub fn validate_runtime_options(
    start_command: Option<&str>,
    release_command: Option<&str>,
    volumes: &[AppVolume],
    processes: &[AppProcess],
) -> Result<()> {
    validate_command(start_command)?;
    validate_command(release_command)?;
    validate_volumes(volumes)?;
    validate_processes(processes)
}

impl Catalog {
    pub fn list_app_processes(&self, app_id: &str) -> Result<Vec<AppProcess>> {
        let mut statement = self
            .connection
            .prepare("SELECT name, command FROM app_processes WHERE app_id = ?1 ORDER BY name")?;
        let rows = statement.query_map(params![app_id], |row| {
            Ok(AppProcess {
                name: row.get(0)?,
                command: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_app_volumes(&self, app_id: &str) -> Result<Vec<AppVolume>> {
        let mut statement = self.connection.prepare(
            "SELECT name, mount_path, read_only FROM app_volumes WHERE app_id = ?1 ORDER BY name",
        )?;
        let rows = statement.query_map(params![app_id], |row| {
            Ok(AppVolume {
                name: row.get(0)?,
                mount_path: row.get(1)?,
                read_only: row.get::<_, i64>(2)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_app_runtime(
        &mut self,
        app_id: &str,
        start_command: Option<&str>,
        release_command: Option<&str>,
        volumes: &[AppVolume],
        processes: &[AppProcess],
    ) -> Result<()> {
        validate_runtime_options(start_command, release_command, volumes, processes)?;
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE apps SET start_command = ?2, release_command = ?3 WHERE id = ?1",
            params![app_id, start_command, release_command],
        )?;
        if updated == 0 {
            bail!("app {app_id} not found");
        }
        transaction.execute("DELETE FROM app_volumes WHERE app_id = ?1", params![app_id])?;
        for volume in volumes {
            transaction.execute(
                "INSERT INTO app_volumes (app_id, name, mount_path, read_only) VALUES (?1, ?2, ?3, ?4)",
                params![app_id, volume.name, volume.mount_path, volume.read_only as i64],
            )?;
        }
        transaction.execute(
            "DELETE FROM app_processes WHERE app_id = ?1",
            params![app_id],
        )?;
        for process in processes {
            transaction.execute(
                "INSERT INTO app_processes (app_id, name, command) VALUES (?1, ?2, ?3)",
                params![app_id, process.name, process.command],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (tempfile::TempDir, Catalog, String) {
        let directory = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(directory.path().join("catalog.sqlite3")).unwrap();
        catalog.connection.execute(
            "INSERT INTO host_agents (id, name, agent_url, enabled, created_at) VALUES ('host', 'Host', 'http://host', 1, 'now')",
            [],
        ).unwrap();
        let app_id = catalog
            .create_app(
                "demo",
                "host",
                "dockerfile",
                Some("owner/repo"),
                Some("main"),
                Some("."),
                Some("Dockerfile"),
                false,
                Some(8080),
                None,
                None,
                None,
            )
            .unwrap();
        (directory, catalog, app_id)
    }

    #[test]
    fn runtime_options_round_trip() {
        let (_directory, mut catalog, app_id) = app();
        let volume = AppVolume {
            name: "data".into(),
            mount_path: "/var/lib/demo".into(),
            read_only: false,
        };
        let process = AppProcess {
            name: "worker".into(),
            command: "npm run worker".into(),
        };
        catalog
            .set_app_runtime(
                &app_id,
                Some("npm start"),
                Some("npm run migrate"),
                std::slice::from_ref(&volume),
                std::slice::from_ref(&process),
            )
            .unwrap();
        assert_eq!(catalog.list_app_volumes(&app_id).unwrap(), vec![volume]);
        assert_eq!(catalog.list_app_processes(&app_id).unwrap(), vec![process]);
        assert_eq!(
            catalog
                .get_app(&app_id)
                .unwrap()
                .unwrap()
                .start_command
                .as_deref(),
            Some("npm start")
        );
    }

    #[test]
    fn unsafe_mount_is_rejected() {
        let (_directory, mut catalog, app_id) = app();
        let volume = AppVolume {
            name: "data".into(),
            mount_path: "../host".into(),
            read_only: false,
        };
        assert!(catalog
            .set_app_runtime(&app_id, None, None, &[volume], &[])
            .is_err());
    }
}
