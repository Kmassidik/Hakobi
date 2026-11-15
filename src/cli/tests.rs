use super::{save_cloudflare_settings, CloudflareSetupInput};
use crate::cloudflare::CreatedTunnel;
use crate::config::ConfigStore;
use zeroize::Zeroizing;

#[test]
fn setup_credentials_are_reusable_from_the_encrypted_catalog() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path().to_owned());
    let input = CloudflareSetupInput {
        base_url: "https://console.glicc.id".into(),
        account_id: "a".repeat(32),
        zone_id: "b".repeat(32),
        api_token: Zeroizing::new("setup-api-token".into()),
    };
    let tunnel = CreatedTunnel {
        id: uuid::Uuid::nil(),
        token: "connector-token".into(),
    };

    save_cloudflare_settings(&store, &input, &tunnel, "glicc.id").unwrap();

    let catalog = hakobi::catalog::Catalog::open(store.data_dir().join("catalog.sqlite3"))
        .expect("open setup catalog");
    let settings = crate::cloudflare::apps_route::load_settings(&catalog).unwrap();
    assert_eq!(settings.account_id, input.account_id);
    assert_eq!(settings.zone_id, input.zone_id);
    assert_eq!(settings.tunnel_id, tunnel.id.to_string());
    assert_eq!(settings.domain, "glicc.id");
    assert_eq!(settings.api_token, "setup-api-token");
    let encrypted = catalog.settings_get("cf_api_token").unwrap().unwrap();
    assert_ne!(encrypted, "setup-api-token");
}
