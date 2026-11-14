//! Hakobi Edge: L7 reverse proxy routing `*.glicc.id` hostnames to local
//! backends based on the control plane's routing table.
//!
//! Built on Cloudflare Pingora 0.9 (Apache-2.0). The request lifecycle is
//! adapted from Pingora's documented `ProxyHttp` quick start and modified
//! for Hakobi's catalog-driven Host routing and atomic reloads.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use pingora::prelude::*;
use pingora::server::configuration::ServerConf;
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;

const DEFAULT_ADDR: &str = "127.0.0.1:9000";
const CATALOG_MTIME_POLL: Duration = Duration::from_secs(5);
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const UPSTREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const UPSTREAM_POOL_SIZE_PER_THREAD: usize = 128;

/// One routing entry: hostname → loopback backend port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Copy)]
struct Upstream {
    addr: SocketAddr,
}

impl Upstream {
    fn new(port: u16) -> Self {
        Self {
            addr: ([127, 0, 0, 1], port).into(),
        }
    }
}

/// Immutable routing snapshot. Readers are lock-free; reloads are atomic.
pub struct RouteTable {
    routes: HashMap<String, Upstream>,
    fallback: Option<Upstream>,
}

impl RouteTable {
    pub fn from_routes(routes: Vec<Route>, fallback_port: Option<u16>) -> Self {
        let map = routes
            .into_iter()
            .map(|route| (route.host.to_lowercase(), Upstream::new(route.port)))
            .collect();
        Self {
            routes: map,
            fallback: fallback_port.map(Upstream::new),
        }
    }

    /// Normal lowercase Host lookups allocate nothing and parse nothing.
    fn upstream_for(&self, host: &str) -> Option<&Upstream> {
        self.routes
            .get(host)
            .or_else(|| {
                host.bytes()
                    .any(|byte| byte.is_ascii_uppercase())
                    .then(|| host.to_ascii_lowercase())
                    .and_then(|lowercase| self.routes.get(&lowercase))
            })
            .or(self.fallback.as_ref())
    }

    pub fn routes_len(&self) -> usize {
        self.routes.len()
    }
}

type SharedTable = Arc<ArcSwap<RouteTable>>;

struct HakobiProxy {
    table: SharedTable,
}

impl HakobiProxy {
    fn upstream_addr(&self, session: &Session) -> Result<SocketAddr, Box<pingora::Error>> {
        let request = session.req_header();
        let host = request
            .uri
            .authority()
            .map(|authority| authority.host())
            .or_else(|| {
                request
                    .headers
                    .get(http::header::HOST)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.split(':').next().unwrap_or(value))
            })
            .ok_or_else(|| Error::explain(HTTPStatus(400), "missing Host header"))?;
        self.table
            .load()
            .upstream_for(host)
            .map(|upstream| upstream.addr)
            .ok_or_else(|| Error::explain(HTTPStatus(404), "no route for Host"))
    }
}

#[async_trait]
impl ProxyHttp for HakobiProxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<pingora::Error>> {
        let addr = self.upstream_addr(session)?;
        let mut peer = Box::new(HttpPeer::new(addr, false, String::new())?);
        peer.options.connection_timeout = Some(UPSTREAM_CONNECT_TIMEOUT);
        peer.options.total_connection_timeout = Some(UPSTREAM_CONNECT_TIMEOUT);
        peer.options.idle_timeout = Some(UPSTREAM_IDLE_TIMEOUT);
        Ok(peer)
    }
}

struct CatalogReload {
    table: SharedTable,
    catalog_path: std::path::PathBuf,
}

impl CatalogReload {
    fn reload_if_changed(&self, last_build: &mut std::time::SystemTime) {
        let mtime = std::fs::metadata(&self.catalog_path)
            .and_then(|meta| meta.modified())
            .unwrap_or(*last_build);
        if mtime <= *last_build {
            return;
        }
        *last_build = mtime;
        match load_table(&self.catalog_path) {
            Ok(new_table) => {
                let count = new_table.routes_len();
                self.table.store(Arc::new(new_table));
                log::info!("routing table reloaded ({count} routes)");
            }
            Err(error) => {
                log::warn!("routing table reload failed; keeping old table: {error:#}");
            }
        }
    }
}

#[async_trait]
impl BackgroundService for CatalogReload {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        let mut last_build = std::fs::metadata(&self.catalog_path)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tokio::time::sleep(CATALOG_MTIME_POLL) => {
                    self.reload_if_changed(&mut last_build);
                }
            }
        }
    }
}

fn worker_threads() -> usize {
    std::env::var("HAKOBI_EDGE_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .min(14)
        })
}

fn main() -> Result<()> {
    env_logger::init();
    let addr: SocketAddr = std::env::var("HAKOBI_EDGE_ADDR")
        .unwrap_or_else(|_| DEFAULT_ADDR.to_owned())
        .parse()
        .context("invalid HAKOBI_EDGE_ADDR")?;
    let catalog_path = std::env::var("HAKOBI_DATA_DIR")
        .map(|dir| std::path::PathBuf::from(dir).join("catalog.sqlite3"))
        .unwrap_or_else(|_| std::path::PathBuf::from("catalog.sqlite3"));
    let table: SharedTable = Arc::new(ArcSwap::from_pointee(load_table(&catalog_path)?));

    let configuration = ServerConf {
        threads: worker_threads(),
        upstream_keepalive_pool_size: UPSTREAM_POOL_SIZE_PER_THREAD,
        work_stealing: false,
        max_retries: 1,
        ..ServerConf::default()
    };

    let mut server = Server::new_with_opt_and_conf(None, configuration);
    server.bootstrap();
    let mut proxy = http_proxy_service(
        &server.configuration,
        HakobiProxy {
            table: Arc::clone(&table),
        },
    );
    proxy.add_tcp(&addr.to_string());
    server.add_service(proxy);
    server.add_service(background_service(
        "catalog reload",
        CatalogReload {
            table,
            catalog_path,
        },
    ));
    log::info!("hakobi-edge listening on {addr}");
    server.run_forever()
}

/// Builds the routing table from the catalog: apps (by name → host_port)
/// plus the control-plane console.
fn load_table(catalog_path: &std::path::Path) -> Result<RouteTable> {
    let catalog = hakobi::catalog::Catalog::open(catalog_path)?;
    let apps = catalog.list_apps()?;
    let routes: Vec<Route> = apps
        .iter()
        .filter_map(|app| {
            let port = app.host_port?;
            Some(Route {
                host: format!("{}.glicc.id", app.name),
                port,
            })
        })
        .collect();
    let mut table = RouteTable::from_routes(routes, None);
    table
        .routes
        .insert("console.glicc.id".to_owned(), Upstream::new(3000));
    Ok(table)
}
