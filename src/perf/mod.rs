use crate::error::{ConcertoError, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const PERF_LOG_PATH: &str = ".concerto/logs/perf.log";
const PERF_ENV: &str = "CONCERTO_DEBUG_PERF";

pub(crate) struct PerfLogger {
    enabled: bool,
    started_at: Option<String>,
}

impl PerfLogger {
    pub(crate) fn from_env() -> Result<Self> {
        let enabled = std::env::var(PERF_ENV).is_ok_and(|value| value != "0");
        let started_at = if enabled {
            Some(formatted_now()?)
        } else {
            None
        };
        let logger = Self {
            enabled,
            started_at,
        };

        logger.start_run()?;

        Ok(logger)
    }

    pub(crate) fn log(
        &self,
        event: &str,
        duration: Duration,
        fields: &[(&str, String)],
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let mut line = format!("{event:<24} {:>6}ms", duration.as_millis());

        for (name, value) in fields {
            line.push_str(&format!("  {name}={value}"));
        }

        line.push('\n');

        self.append(&line)
    }

    pub(crate) fn finish_run(&self, duration: Duration, package_count: usize) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        self.append(&format!(
            "=== concerto install done {} total={}ms packages={} ===\n",
            formatted_now()?,
            duration.as_millis(),
            package_count
        ))
    }

    fn start_run(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let started_at = self
            .started_at
            .as_deref()
            .ok_or_else(|| ConcertoError::perf("Missing perf log start date"))?;

        self.append(&format!("\n=== concerto install run {started_at} ===\n"))
    }

    fn append(&self, content: &str) -> Result<()> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(perf_log_path()?)
            .and_then(|mut file| file.write_all(content.as_bytes()))
            .map_err(|error| ConcertoError::perf(format!("Could not write perf log: {error}")))
    }
}

fn formatted_now() -> Result<String> {
    let output = Command::new("date")
        .arg("+%d/%m/%Y %H:%M:%S")
        .output()
        .map_err(|error| ConcertoError::perf(format!("Could not format perf log date: {error}")))?;

    if !output.status.success() {
        return Err(ConcertoError::perf("Could not format perf log date"));
    }

    String::from_utf8(output.stdout)
        .map(|output| output.trim().to_string())
        .map_err(|error| ConcertoError::perf(format!("Invalid perf log date output: {error}")))
}

fn perf_log_path() -> Result<&'static Path> {
    let path = Path::new(PERF_LOG_PATH);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ConcertoError::perf(format!("Could not create perf log directory: {error}"))
        })?;
    }

    Ok(path)
}
