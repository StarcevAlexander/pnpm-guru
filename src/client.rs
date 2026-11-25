use crate::registry::RegistryIndex;
use serde_json::Value;

pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";
const ABBREVIATED_ACCEPT: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("http status {status} for {url}")]
    Status { status: u16, url: String },
    #[error("network error for {url}: {source}")]
    Network { url: String, source: std::io::Error },
    #[error("invalid packument for {name}: {problem}")]
    InvalidPackument { name: String, problem: String },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct RegistryClient {
    base: String,
    agent: ureq::Agent,
}

impl RegistryClient {
    pub fn new(base: impl Into<String>) -> Self {
        let base = base.into();
        let trimmed = base.trim_end_matches('/').to_string();
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let config = ureq::config::Config::builder()
            .tls_config(tls)
            .max_redirects(10)
            .build();
        Self {
            base: trimmed,
            agent: ureq::Agent::new_with_config(config),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn package_url(&self, name: &str) -> String {
        let encoded = name.replace('/', "%2F");
        format!("{}/{}", self.base, encoded)
    }

    pub fn packument(&self, name: &str) -> Result<Value, RegistryError> {
        let url = self.package_url(name);
        let response = self
            .agent
            .get(&url)
            .header("Accept", ABBREVIATED_ACCEPT)
            .call()
            .map_err(|error| convert_ureq_error(&url, error))?;
        let status = response.status();
        if status != 200 {
            return Err(RegistryError::Status {
                status: status.as_u16(),
                url,
            });
        }
        let body = response
            .into_body()
            .read_to_vec()
            .map_err(|error| convert_ureq_error(&url, error))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn packument_index(&self, name: &str) -> Result<RegistryIndex, RegistryError> {
        let packument = self.packument(name)?;
        if !packument.is_object() {
            return Err(RegistryError::InvalidPackument {
                name: name.to_string(),
                problem: String::from("response is not an object"),
            });
        }
        Ok(RegistryIndex::from_packument(&packument, name))
    }

    pub fn tarball(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|error| convert_ureq_error(url, error))?;
        let status = response.status();
        if status != 200 {
            return Err(RegistryError::Status {
                status: status.as_u16(),
                url: url.to_string(),
            });
        }
        response
            .into_body()
            .read_to_vec()
            .map_err(|error| convert_ureq_error(url, error))
    }
}

fn convert_ureq_error(url: &str, error: ureq::Error) -> RegistryError {
    match error {
        ureq::Error::StatusCode(status) => RegistryError::Status {
            status,
            url: url.to_string(),
        },
        ureq::Error::Io(source) => RegistryError::Network {
            url: url.to_string(),
            source,
        },
        other => RegistryError::Network {
            url: url.to_string(),
            source: std::io::Error::other(other.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};

    fn serve_once(responses: Vec<(u16, String, Vec<u8>)>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for (status, content_type, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = Vec::new();
                let mut byte = [0u8; 1];
                loop {
                    match stream.read(&mut byte) {
                        Ok(0) => break,
                        Ok(_) => {
                            buffer.push(byte[0]);
                            if buffer.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    #[test]
    fn packument_is_parsed_into_index() {
        let packument = serde_json::json!({
            "name": "tiny",
            "dist-tags": {"latest": "1.1.0"},
            "versions": {
                "1.0.0": {
                    "dependencies": {"lefty": "^1.0.0"},
                    "dist": {"integrity": "sha512-abc", "tarball": "http://x/tiny-1.0.0.tgz"}
                },
                "1.1.0": {
                    "optionalDependencies": {"opt": "~2.0.0"},
                    "peerDependencies": {"peer": "*"},
                    "dist": {"integrity": "sha512-def", "tarball": "http://x/tiny-1.1.0.tgz"}
                }
            }
        });
        let base = serve_once(vec![(
            200,
            String::from("application/json"),
            serde_json::to_vec(&packument).unwrap(),
        )]);
        let client = RegistryClient::new(base);
        let index = client.packument_index("tiny").unwrap();
        assert_eq!(index.package_count(), 1);
        let entry = index.entry("tiny", "1.0.0").unwrap();
        assert_eq!(entry.dependencies["lefty"], "^1.0.0");
        assert_eq!(entry.integrity.as_deref(), Some("sha512-abc"));
        assert_eq!(entry.tarball.as_deref(), Some("http://x/tiny-1.0.0.tgz"));
        let latest = index.entry("tiny", "1.1.0").unwrap();
        assert_eq!(latest.optional_dependencies["opt"], "~2.0.0");
        assert_eq!(latest.peer_dependencies["peer"], "*");
    }

    #[test]
    fn status_errors_are_reported() {
        let base = serve_once(vec![(404, String::from("text/plain"), b"nope".to_vec())]);
        let client = RegistryClient::new(base);
        let error = client.packument("missing").unwrap_err();
        assert!(error.to_string().contains("404"));
    }

    #[test]
    fn scoped_names_are_url_encoded() {
        let base = serve_once(vec![(
            200,
            String::from("application/json"),
            serde_json::to_vec(&serde_json::json!({"versions": {}})).unwrap(),
        )]);
        let client = RegistryClient::new(base);
        client.packument("@scope/pkg").unwrap();
    }
}
