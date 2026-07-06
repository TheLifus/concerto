use crate::error::{ConcertoError, Result};

const USER_AGENT: &str = "concerto";

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

pub fn download_bytes(url: &str) -> Result<Vec<u8>> {
    if let Some(path) = file_url_path(url) {
        return std::fs::read(path)
            .map_err(|error| ConcertoError::http(format!("Could not read local file: {error}")));
    }

    client()
        .get(url)
        .send()
        .map_err(|error| ConcertoError::http(format!("Could not download url: {error}")))?
        .error_for_status()
        .map_err(|error| ConcertoError::http(format!("Download failed: {error}")))?
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|error| ConcertoError::http(format!("Could not read download response: {error}")))
}

fn file_url_path(url: &str) -> Option<&str> {
    url.strip_prefix("file://")
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

#[cfg(test)]
mod tests;
