//! The servers over the packs given, or the store's installed packs, which they follow as
//! `tgd install` and `remove` change them (D-172):
//!
//! - `tgd serve`: the HTTP server (phase 6.2, [`teigeodata_server::http`]), bound to 127.0.0.1
//!   unless told otherwise; stops on Ctrl-C after the requests in flight end.
//! - `tgd mcp`: the MCP server on standard input and output (phase 6.3,
//!   [`teigeodata_server::mcp`]), for an MCP client to start; stops when the client closes
//!   them. Standard output is the protocol's: warnings go to standard error.

use std::io::Write;
#[cfg(feature = "serve")]
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "serve")]
use teigeodata_server::http::{self, App, Config};
use teigeodata_server::packs::Packs;
use teigeodata_server::seams::Seams;

use crate::Fail;

/// The packs given, or the store's installed packs; a pack that cannot be opened is left out
/// with a warning on standard error.
fn open(store: &Path, packs: &[PathBuf]) -> Result<Packs, Fail> {
    let (packs, followed) = if packs.is_empty() {
        Packs::open(&[], Some(store))
    } else {
        Packs::open(packs, None)
    }
    .map_err(|e| Fail::Refused(e.to_string()))?;
    for (path, e) in &followed.failed {
        let _ = writeln!(
            std::io::stderr(),
            "tgd: {}: {e} (left out: `tgd repair` it)",
            path.display()
        );
    }
    Ok(packs)
}

/// Serves MCP on standard input and output until the client closes them.
#[cfg(feature = "mcp")]
pub(crate) fn mcp(store: &Path, packs: &[PathBuf], follow_secs: u64) -> Result<(), Fail> {
    use teigeodata_server::mcp::{self, Mcp};
    use teigeodata_server::query::Limits;

    let packs = Arc::new(open(store, packs)?);
    let server = Mcp::new(packs, Seams::default(), Limits::SERVER)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let follow = (follow_secs > 0).then(|| Duration::from_secs(follow_secs));
    rt.block_on(mcp::serve_stdio(server, follow))?;
    // Tokio's standard input is a blocking read that the runtime waits for: end it now.
    rt.shutdown_background();
    Ok(())
}

/// How `tgd serve` runs.
#[cfg(feature = "serve")]
#[derive(Debug)]
pub(crate) struct Options {
    pub(crate) listen: SocketAddr,
    pub(crate) cors: Vec<String>,
    pub(crate) follow_secs: u64,
    pub(crate) timeout_secs: u64,
}

#[cfg(feature = "serve")]
pub(crate) fn run(
    store: &Path,
    packs: &[PathBuf],
    o: &Options,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let packs = open(store, packs)?;
    let cors_origins = o
        .cors
        .iter()
        .map(|c| {
            c.parse()
                .map_err(|_| Fail::Refused(format!("--cors-origin {c:?} is not a valid origin")))
        })
        .collect::<Result<_, _>>()?;
    let app = Arc::new(App {
        packs: Arc::new(packs),
        seams: Seams::default(),
        config: Config {
            cors_origins,
            timeout: Duration::from_secs(o.timeout_secs.max(1)),
            ..Config::default()
        },
    });
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let listener = tokio::net::TcpListener::bind(o.listen).await?;
        writeln!(out, "serving on http://{}", listener.local_addr()?)?;
        out.flush()?;
        let follow = (o.follow_secs > 0).then(|| Duration::from_secs(o.follow_secs));
        let stop = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        http::serve(listener, app, follow, stop).await?;
        Ok(())
    })
}
