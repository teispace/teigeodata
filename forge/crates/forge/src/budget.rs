//! G-budget: a build's declared resource budget (`budget:` in `build.yaml`) against what
//! `forge make` measured. Peak memory is the process's resident high-water mark during each
//! stage that ran: Linux's `VmHWM` in `/proc/self/status`, reset before every stage through
//! `/proc/self/clear_refs` (Linux 4.0+), so a stage is charged for its own peak. Elsewhere the
//! peak is not measured and only disk is gated. Disk is the largest size of `data/build`
//! after any stage, which holds every stage's output; the fetched inputs in the store are
//! pinned sources, not build output, and are sized by `sources.lock`.

use std::fs;
use std::path::Path;

use serde::Deserialize;

const MB: u64 = 1_000_000;

/// The declared budget of a country's build, in megabytes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Budget {
    /// Peak resident memory of any one stage.
    rss_mb: u64,
    /// `data/build` after any stage.
    disk_mb: u64,
}

/// What a run measured: the stage with the highest memory peak (where measured) and the
/// largest build output.
#[derive(Default)]
pub(crate) struct Usage {
    pub(crate) peak: Option<(String, u64)>,
    pub(crate) disk: u64,
}

impl Usage {
    /// Records one stage's memory peak.
    pub(crate) fn stage(&mut self, name: &str, rss: Option<u64>) {
        if let Some(b) = rss
            && self.peak.as_ref().is_none_or(|(_, p)| b > *p)
        {
            self.peak = Some((name.to_string(), b));
        }
    }

    /// One line for the make summary.
    pub(crate) fn line(&self) -> String {
        let peak = self.peak.as_ref().map_or_else(
            || "peak memory not measured on this platform".to_string(),
            |(s, b)| format!("peak memory {} MB ({s})", b.div_ceil(MB)),
        );
        format!("{peak}, build output {} MB", self.disk.div_ceil(MB))
    }

    /// The overruns of `budget`: empty when the run kept within it.
    pub(crate) fn over(&self, budget: &Budget) -> Vec<String> {
        let mut out = Vec::new();
        if let Some((stage, b)) = &self.peak
            && *b > budget.rss_mb * MB
        {
            out.push(format!(
                "G-budget: {stage} peaked at {} MB, over the budget's {} MB",
                b.div_ceil(MB),
                budget.rss_mb
            ));
        }
        if self.disk > budget.disk_mb * MB {
            out.push(format!(
                "G-budget: the build output reached {} MB, over the budget's {} MB",
                self.disk.div_ceil(MB),
                budget.disk_mb
            ));
        }
        out
    }
}

/// Starts a stage's memory peak from the current resident size (Linux; a no-op elsewhere).
pub(crate) fn reset_peak() {
    // Writing 5 resets VmHWM to the current RSS. Failing leaves the process peak, which is
    // only ever higher: a run can be wrongly refused, never wrongly passed.
    let _ = fs::write("/proc/self/clear_refs", "5");
}

/// The process's resident high-water mark in bytes since the last [`reset_peak`], where the
/// platform reports it.
pub(crate) fn peak_rss() -> Option<u64> {
    vm_hwm(&fs::read_to_string("/proc/self/status").ok()?)
}

/// `VmHWM` of a `/proc/<pid>/status` text, in bytes.
fn vm_hwm(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line
        .trim_start_matches("VmHWM:")
        .trim()
        .strip_suffix("kB")?
        .trim()
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// The bytes of every file under `dir` (0 when it is missing); symbolic links are not followed.
pub(crate) fn tree_bytes(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => tree_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{Budget, MB, Usage, tree_bytes, vm_hwm};

    #[test]
    fn reads_the_peak_from_proc_status() {
        let status = "Name:\tforge\nVmPeak:\t 9000 kB\nVmHWM:\t  123456 kB\nVmRSS:\t 1000 kB\n";
        assert_eq!(vm_hwm(status), Some(123_456 * 1024));
        assert_eq!(vm_hwm("Name:\tforge\n"), None);
        assert_eq!(vm_hwm("VmHWM:\t12 MB\n"), None);
    }

    #[test]
    fn a_run_over_its_budget_is_refused() {
        let budget = Budget {
            rss_mb: 100,
            disk_mb: 50,
        };
        let mut u = Usage::default();
        u.stage("read geonames-np", Some(60 * MB));
        u.stage("conflate", Some(100 * MB));
        u.stage("dem", None);
        u.disk = 50 * MB;
        assert_eq!(u.over(&budget), Vec::<String>::new());
        assert!(
            u.line().starts_with("peak memory 100 MB (conflate)"),
            "{}",
            u.line()
        );
        u.stage("build", Some(100 * MB + 1));
        u.disk += 1;
        let over = u.over(&budget);
        assert_eq!(over.len(), 2, "{over:?}");
        assert!(over[0].contains("build peaked at 101 MB"), "{over:?}");
        assert!(over[1].contains("reached 51 MB"), "{over:?}");
        // Unmeasured memory gates disk only.
        let disk_only = Usage {
            peak: None,
            disk: 0,
        };
        assert_eq!(disk_only.over(&budget), Vec::<String>::new());
        assert!(disk_only.line().contains("not measured"));
    }

    #[test]
    fn sizes_a_tree() {
        let dir = std::env::temp_dir().join(format!("forge-budget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("x"), [0; 10]).unwrap();
        std::fs::write(dir.join("a/b/y"), [0; 32]).unwrap();
        assert_eq!(tree_bytes(&dir), 42);
        assert_eq!(tree_bytes(&dir.join("missing")), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// On Linux the peak is measured and a reset brings it near the current size.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_peak_is_measured_on_linux() {
        super::reset_peak();
        let before = super::peak_rss().unwrap();
        let v = vec![1u8; 64 * 1024 * 1024];
        std::hint::black_box(&v);
        assert!(super::peak_rss().unwrap() >= before + 60 * 1024 * 1024);
    }
}
