const USER_AGENT: &str = "concerto";

pub fn get_text(url: &str) -> Result<String, String> {
    client()
        .get(url)
        .send()
        .map_err(|error| format!("Could not fetch url: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Request failed: {error}"))?
        .text()
        .map_err(|error| format!("Could not read response: {error}"))
}

pub fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    client()
        .get(url)
        .send()
        .map_err(|error| format!("Could not download url: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Download failed: {error}"))?
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("Could not read download response: {error}"))
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}
