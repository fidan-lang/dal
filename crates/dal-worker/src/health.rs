//! Local worker health reflects successful queue/DB progress, not merely a PID.
use anyhow::{Context, Result, ensure};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
// Allows a long poll and two lease-renewal intervals; stalled dependencies expire.
const MAX_PROGRESS_AGE: Duration = Duration::from_secs(150);
fn path() -> PathBuf {
    std::env::var_os("DAL_WORKER_HEALTH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("dal-worker-health"))
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn valid(record: &str, current: u64) -> bool {
    record
        .trim()
        .parse::<u64>()
        .ok()
        .is_some_and(|at| at <= current && current - at <= MAX_PROGRESS_AGE.as_secs())
}
pub fn check() -> Result<()> {
    let file = path();
    let info = std::fs::symlink_metadata(&file).context("worker has not made progress")?;
    ensure!(
        info.file_type().is_file() && info.len() <= 32,
        "invalid worker progress record"
    );
    ensure!(
        valid(&std::fs::read_to_string(file)?, now()?),
        "worker progress is stale"
    );
    Ok(())
}
pub fn record() -> Result<()> {
    let file = path();
    let pending = file.with_extension("pending");
    std::fs::write(&pending, now()?.to_string())?;
    std::fs::rename(pending, file)?;
    Ok(())
}
pub fn reset() -> Result<()> {
    match std::fs::remove_file(path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_recent_well_formed_progress_is_healthy() {
        assert!(valid("1000", 1000));
        assert!(valid("850\n", 1000));
        for value in ["849", "1001", "0", "", "not-a-time", "18446744073709551616"] {
            assert!(!valid(value, 1000));
        }
    }
}
