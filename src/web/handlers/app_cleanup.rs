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
