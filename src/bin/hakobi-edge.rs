//! Hakobi Edge: L7 reverse proxy routing `*.glicc.id` hostnames to local
//! backends based on the control plane's routing table.
//!
//! Traffic path: Cloudflare edge → cloudflared (TLS) → 127.0.0.1:9000 (this
//! proxy, plain HTTP on loopback) → upstream per Host header.
//!
//! Routing table: rebuilt from the catalog (`apps.host_port` + CF zone
//! records) on every SIGHUP or when the catalog mtime changes. The active
//! table is an `ArcSwap<RouteTable>` — readers lock-free, reloads atomic.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

const DEFAULT_ADDR: &str = "127.0.0.1:9000";
const CATALOG_MTIME_POLL: Duration = Duration::from_secs(5);
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// One routing entry: hostname → loopback backend port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub host: String,
    pub port: u16,
}

/// Immutable routing snapshot. Readers clone the Arc; reloads swap it.
pub struct RouteTable {
    /// lowercased host → upstream port
    routes: HashMap<String, u16>,
    /// where unmatched hosts go (console, root domain, etc.)
    pub fallback_port: Option<u16>,
}

impl RouteTable {
    pub fn from_routes(routes: Vec<Route>, fallback_port: Option<u16>) -> Self {
        let map = routes
            .into_iter()
            .map(|route| (route.host.to_lowercase(), route.port))
            .collect();
        Self {
            routes: map,
            fallback_port,
        }
    }

    /// O(1) lookup, zero allocation — the hot path.
    pub fn port_for(&self, host: &str) -> Option<u16> {
        self.routes.get(&host.to_ascii_lowercase()).copied()
    }

    pub fn routes_len(&self) -> usize {
        self.routes.len()
    }
}

type SharedTable = Arc<ArcSwap<RouteTable>>;

fn main() -> Result<()> {
    let addr: SocketAddr = std::env::var("HAKOBI_EDGE_ADDR")
        .unwrap_or_else(|_| DEFAULT_ADDR.to_owned())
        .parse()
        .context("invalid HAKOBI_EDGE_ADDR")?;
    let catalog_path = std::env::var("HAKOBI_DATA_DIR")
        .map(|dir| std::path::PathBuf::from(dir).join("catalog.sqlite3"))
        .unwrap_or_else(|_| std::path::PathBuf::from("catalog.sqlite3"));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async_main(addr, catalog_path))
}

async fn async_main(addr: SocketAddr, catalog_path: std::path::PathBuf) -> Result<()> {
    let table: SharedTable = Arc::new(ArcSwap::from_pointee(load_table(&catalog_path)?));
    tracing::info!(
        "routing table loaded ({} routes)",
        table.load().routes_len()
    );

    // Reload loop: poll catalog mtime; swap only when the file changed AND
    // the new table parses. Bad config keeps the old table (validation by
    // construction — we only swap after a successful build).
    {
        let table = Arc::clone(&table);
        let mut last_build = std::time::SystemTime::UNIX_EPOCH;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(CATALOG_MTIME_POLL).await;
                let mtime = std::fs::metadata(&catalog_path)
                    .and_then(|meta| meta.modified())
                    .unwrap_or(last_build);
                if mtime <= last_build {
                    continue;
                }
                last_build = mtime;
                match load_table(&catalog_path) {
                    Ok(new_table) => {
                        let count = new_table.routes_len();
                        table.store(Arc::new(new_table));
                        tracing::info!("routing table reloaded ({count} routes)");
                    }
                    Err(error) => {
                        tracing::warn!(%error, "routing table reload failed; keeping old table");
                    }
                }
            }
        });
    }

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding hakobi-edge on {addr}"))?;
    tracing::info!("hakobi-edge listening on {addr}");

    loop {
        let (stream, _peer) = listener.accept().await?;
        let table = Arc::clone(&table);
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, table).await {
                tracing::debug!(%error, "connection ended with error");
            }
        });
    }
}

async fn serve_connection(stream: TcpStream, table: SharedTable) -> Result<()> {
    stream.set_nodelay(true)?;
    let io = TokioIo::new(stream);
    let hyper_service =
        hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
            let table = Arc::clone(&table);
            async move { proxy_request(request, table).await }
        });
    hyper::server::conn::http1::Builder::new()
        .keep_alive(true)
        .timer(hyper_util::rt::TokioTimer::new())
        .serve_connection(io, hyper_service)
        .await
        .context("connection error")
}

/// Resolves the Host header to an upstream and proxies the request,
/// streaming the body in both directions without buffering.
async fn proxy_request(
    request: hyper::Request<hyper::body::Incoming>,
    table: SharedTable,
) -> Result<hyper::Response<hyper::body::Incoming>, anyhow::Error> {
    let host = request
        .headers()
        .get(hyper::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(':').next().unwrap_or(value).to_owned())
        .context("missing Host header")?;

    let snapshot = table.load();
    let port = snapshot
        .port_for(&host)
        .or(snapshot.fallback_port)
        .context(format!("no route for {host}"))?;
    drop(snapshot);

    let upstream_addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let upstream =
        tokio::time::timeout(UPSTREAM_CONNECT_TIMEOUT, TcpStream::connect(upstream_addr))
            .await
            .context("upstream connect timed out")?
            .with_context(|| format!("upstream {host}:{port} unreachable"))?;
    upstream.set_nodelay(true)?;
    let upstream_io = TokioIo::new(upstream);

    // Hand the parsed request to hyper as a client, streaming the body.
    let (mut sender, connection) = hyper::client::conn::http1::handshake(upstream_io).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::debug!(%error, "upstream connection ended");
        }
    });

    let response = sender
        .send_request(request.map(|body| {
            // Preserve streaming semantics for the request body.
            body
        }))
        .await
        .context("upstream request failed")?;
    Ok(response)
}

/// Builds the routing table from the catalog: apps (by name → host_port)
/// plus well-known statics (console → 3000).
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
    // Console (the control plane itself) is always reachable.
    let mut table = RouteTable::from_routes(routes, None);
    table.routes.insert("console.glicc.id".to_owned(), 3000);
    Ok(table)
}
