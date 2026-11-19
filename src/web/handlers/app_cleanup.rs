//! Snapshot-first cleanup of Docker resources explicitly owned by an app.

use super::AppState;

#[derive(Default)]
pub(super) struct OwnedResources {
    networks: Vec<String>,
    volumes: Vec<String>,
}

pub(super) async fn snapshot_owned_resources(
    state: &AppState,
    app: &hakobi::catalog::App,
    delete_data: bool,
) -> OwnedResources {
    let endpoint = format!("agent://{}", app.host_id);
    match state.backend.snapshot(&endpoint).await {
        Ok(snapshot) => OwnedResources {
            networks: snapshot
                .networks
                .into_iter()
                .filter(|network| network.app_id.as_deref() == Some(app.id.as_str()))
                .map(|network| network.name)
                .collect(),
            volumes: if delete_data {
                snapshot
                    .volumes
                    .into_iter()
                    .filter(|volume| volume.app_id.as_deref() == Some(app.id.as_str()))
                    .map(|volume| volume.name)
                    .collect()
            } else {
                Vec::new()
            },
        },
        Err(error) => {
            tracing::warn!(app = %app.id, %error, "app-owned resource snapshot unavailable");
            OwnedResources::default()
        }
    }
}

pub(super) async fn remove_owned_resources(
    state: &AppState,
    app: &hakobi::catalog::App,
    resources: OwnedResources,
) {
    let endpoint = format!("agent://{}", app.host_id);
    for network in resources.networks {
        if let Err(error) = state.backend.remove_network(&endpoint, &network).await {
            tracing::warn!(app = %app.id, %network, %error, "app-owned network removal skipped");
        }
    }
    for volume in resources.volumes {
        if let Err(error) = state.backend.remove_volume(&endpoint, &volume).await {
            tracing::warn!(app = %app.id, %volume, %error, "app-owned volume removal skipped");
        }
    }
}

pub(super) async fn app_image_tags(
    state: &AppState,
    app: &hakobi::catalog::App,
) -> anyhow::Result<Vec<String>> {
    let app_id = app.id.clone();
    let app_name = app.name.clone();
    super::with_catalog(state, move |catalog| {
        Ok(catalog
            .list_deployments(&app_id)?
            .into_iter()
            .map(|deployment| {
                deployment.image_tag.unwrap_or_else(|| {
                    crate::web::deployment::image_tag_for(&app_name, deployment.version)
                })
            })
            .collect())
    })
    .await
}

pub(super) async fn remove_app_container(
    state: &AppState,
    app: &hakobi::catalog::App,
) -> Option<String> {
    let app_id = app.id.clone();
    let processes = super::with_catalog(state, move |catalog| catalog.list_app_processes(&app_id))
        .await
        .unwrap_or_default();
    let mut containers = vec![crate::web::deployment::container_name_for(&app.id)];
    containers.extend(
        processes
            .into_iter()
            .map(|process| hakobi::catalog::worker_container_name(&app.id, &process.name)),
    );
    let endpoint = format!("agent://{}", app.host_id);
    for container in containers {
        let _ = state.backend.remove_container(&endpoint, &container).await;
    }
    app.subdomain.clone()
}

pub(super) async fn remove_app_images(
    state: &AppState,
    app: &hakobi::catalog::App,
    tags: &[String],
) {
    let endpoint = format!("agent://{}", app.host_id);
    for tag in tags {
        if let Err(error) = state.backend.remove_image(&endpoint, tag).await {
            tracing::warn!(app = %app.name, image = %tag, %error, "app image removal skipped");
        }
    }
}

pub(super) fn spawn_cloudflare_route_cleanup(state: &AppState, subdomain: String) {
    let cf_path = state.catalog_path.clone();
    tokio::spawn(async move {
        let settings = tokio::task::spawn_blocking(move || {
            let catalog = hakobi::catalog::Catalog::open(cf_path.as_ref())?;
            crate::cloudflare::apps_route::load_settings(&catalog)
        })
        .await
        .unwrap_or_else(|error| Err(anyhow::anyhow!(error.to_string())));
        let settings = match settings {
            Ok(settings) => settings,
            Err(_) => return,
        };
        if let Err(error) =
            crate::cloudflare::apps_route::remove_app_route_with(&settings, &subdomain).await
        {
            tracing::warn!(error = %error, app = %subdomain, "cloudflare route removal skipped");
        }
    });
}
