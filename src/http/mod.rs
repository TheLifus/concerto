use crate::error::{ConcertoError, Result};
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

const USER_AGENT: &str = "concerto";
const MAX_IDLE_CONNECTIONS_PER_HOST: usize = 32;

static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

pub fn get_text(url: &str) -> Result<String> {
    if let Some(path) = file_url_path(url) {
        return std::fs::read_to_string(path)
            .map_err(|error| ConcertoError::http(format!("Could not read local file: {error}")));
    }

    client()
        .get(url)
        .send()
        .map_err(|error| ConcertoError::http(format!("Could not fetch url: {error}")))?
        .error_for_status()
        .map_err(|error| ConcertoError::http(format!("Request failed: {error}")))?
        .text()
        .map_err(|error| ConcertoError::http(format!("Could not read response: {error}")))
}

pub fn download_to_file(url: &str, destination: &Path) -> Result<()> {
    if let Some(source) = file_url_path(url) {
        return std::fs::copy(source, destination)
            .map(|_| ())
            .map_err(|error| ConcertoError::http(format!("Could not copy local file: {error}")));
    }

    let mut response = client()
        .get(url)
        .send()
        .map_err(|error| ConcertoError::http(format!("Could not download url: {error}")))?
        .error_for_status()
        .map_err(|error| ConcertoError::http(format!("Download failed: {error}")))?;
    let mut file = std::fs::File::create(destination)
        .map_err(|error| ConcertoError::http(format!("Could not create download file: {error}")))?;

    std::io::copy(&mut response, &mut file)
        .and_then(|_| file.flush())
        .map_err(|error| ConcertoError::http(format!("Could not write download file: {error}")))
}

fn file_url_path(url: &str) -> Option<&str> {
    url.strip_prefix("file://")
}

fn client() -> &'static reqwest::blocking::Client {
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .pool_max_idle_per_host(MAX_IDLE_CONNECTIONS_PER_HOST)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

#[cfg(test)]
mod tests;
