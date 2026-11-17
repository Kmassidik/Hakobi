use anyhow::{bail, Result};

use super::super::deployments::validate_build_type;
use super::{
    app_name_taken, ensure_enabled_host, update_app_record, validate_app_name, validate_env_json,
    validate_port, validate_resource_limits, Catalog,
};

impl Catalog {
    /// Replaces the mutable fields of an app.
    #[allow(clippy::too_many_arguments)]
    pub fn update_app(
        &mut self,
        id: &str,
        name: &str,
        host_id: &str,
        build_type: &str,
        github_repo: Option<&str>,
        github_branch: Option<&str>,
        github_path: Option<&str>,
        github_dockerfile: Option<&str>,
        github_auto_deploy: bool,
        container_port: Option<u16>,
        requested_host_port: Option<u16>,
        cpu_limit: Option<f64>,
        memory_limit_bytes: Option<u64>,
        env_json: &str,
    ) -> Result<()> {
        let name = name.trim();
        validate_update_metadata(name, env_json)?;
        validate_build_type(build_type)?;
        validate_port(container_port)?;
        validate_port(requested_host_port)?;
        validate_resource_limits(cpu_limit, memory_limit_bytes)?;
        let transaction = self.connection.transaction()?;
        ensure_enabled_host(&transaction, host_id)?;
        if app_name_taken(&transaction, name, Some(id))? {
            bail!("app {name} already exists");
        }
        let updated = update_app_record(
            &transaction,
            id,
            name,
            host_id,
            build_type,
            github_repo,
            github_branch,
            github_path,
            github_dockerfile,
            github_auto_deploy,
            container_port,
            requested_host_port,
            cpu_limit,
            memory_limit_bytes,
            env_json,
        )?;
        ensure_updated(updated, id)?;
        super::super::record_audit(&transaction, "update", "app", id)?;
        transaction.commit()?;
        Ok(())
    }
}

fn validate_update_metadata(name: &str, env_json: &str) -> Result<()> {
    validate_app_name(name)?;
    validate_env_json(env_json)
}

fn ensure_updated(updated: usize, id: &str) -> Result<()> {
    if updated == 0 {
        bail!("app {id} not found");
    }
    Ok(())
}
