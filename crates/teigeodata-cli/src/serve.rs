//! `tgd serve`: the HTTP server (phase 6.2, [`teigeodata_server::http`]) over the packs given,
//! or the store's installed packs, which it follows as `tgd install` and `remove` change them
//! (D-172). Bound to 127.0.0.1 unless told otherwise; stops on Ctrl-C after the requests in
//! flight end.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use teigeodata_server::http::{self, App, Config};
use teigeodata_server::packs::Packs;
use teigeodata_server::seams::Seams;

use crate::Fail;

/// How `tgd serve` runs.
#[derive(Debug)]
pub(crate) struct Options {
    pub(crate) listen: SocketAddr,
    pub(crate) cors: Vec<String>,
    pub(crate) follow_secs: u64,
    pub(crate) timeout_secs: u64,
}

pub(crate) fn run(
    store: &Path,
    packs: &[PathBuf],
    o: &Options,
    out: &mut dyn Write,
) -> Result<(), Fail> {
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
    let cors_origins = o
        .cors
        .iter()
        .map(|c| {
            c.parse()
                .map_err(|_| Fail::Refused(format!("--cors-origin {c:?} is not a valid origin")))
        })
        .collect::<Result<_, _>>()?;
    let app = Arc::new(App {
        packs,
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
