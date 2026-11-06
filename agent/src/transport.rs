//! Bounded public JSON-RPC transport. Errors never include headers or request bodies.

use std::path::{Component, Path};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use near_sdk::base64::{Engine, engine::general_purpose::STANDARD};
use near_sdk::serde_json::{Value, json};
use serde::de::DeserializeOwned;

use crate::AgentError;

pub const MAX_PUBLIC_FILE_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct JsonRpc {
    endpoint: reqwest::Url,
    client: reqwest::Client,
    auth: Option<reqwest::header::HeaderValue>,
    max_bytes: usize,
}

impl JsonRpc {
    /// HTTP is allowed only for a loopback test server; public endpoints require HTTPS.
    pub fn new(endpoint: &str, auth: Option<&str>, max_bytes: usize) -> Result<Self, AgentError> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|_| "invalid public RPC URL")?;
        let local = endpoint.host_str().is_some_and(|host| {
            host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host == "::1"
        });
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !(endpoint.scheme() == "https" || (endpoint.scheme() == "http" && local))
        {
            return Err("public RPC URL must use HTTPS (or loopback HTTP), without credentials/query/fragment".into());
        }
        if !(1024..=1024 * 1024).contains(&max_bytes) {
            return Err("invalid response-size limit".into());
        }
        let auth = auth
            .map(reqwest::header::HeaderValue::from_str)
            .transpose()
            .map_err(|_| "invalid relay authentication configuration")?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "HTTP client initialization failed")?;
        Ok(Self { endpoint, client, auth, max_bytes })
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, AgentError> {
        let request = json!({"jsonrpc":"2.0", "id":1, "method":method, "params":params});
        let mut builder = self.client.post(self.endpoint.clone()).json(&request);
        if let Some(auth) = &self.auth {
            builder = builder.header("X-API-Key", auth);
        }
        let mut response = builder.send().await.map_err(|_| "public RPC transport failed")?;
        if !response.status().is_success() {
            return Err(format!("public RPC HTTP status {}", response.status().as_u16()).into());
        }
        if response.content_length().is_some_and(|length| length > self.max_bytes as u64) {
            return Err("public RPC response exceeds size limit".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) =
            response.chunk().await.map_err(|_| "public RPC response read failed")?
        {
            if chunk.len() > self.max_bytes.saturating_sub(bytes.len()) {
                return Err("public RPC response exceeds size limit".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let answer: Value = near_sdk::serde_json::from_slice(&bytes)
            .map_err(|_| "public RPC returned malformed JSON")?;
        if answer["jsonrpc"] != "2.0" || answer["id"] != 1 {
            return Err("public RPC returned a mismatched envelope".into());
        }
        if answer.get("error").is_some() {
            // Providers can echo request data in their error messages.
            return Err("public RPC returned an application error".into());
        }
        answer.get("result").cloned().ok_or_else(|| "public RPC result is missing".into())
    }
}

/// Reject forbidden files/components before opening user-selected public configuration.
pub fn validate_public_path(path: &Path) -> Result<(), AgentError> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("public path has no UTF-8 filename")?
        .to_ascii_lowercase();
    if file_name == ".env"
        || file_name.starts_with(".env.")
        || file_name.ends_with(".env")
        || ["key", "secret", "token", "credential", "mnemonic", "seed", "private"]
            .iter()
            .any(|word| file_name.contains(word))
    {
        return Err("refused prohibited public-file path".into());
    }
    for component in path.components() {
        if let Component::Normal(value) = component {
            let name = value.to_str().ok_or("public path is not UTF-8")?.to_ascii_lowercase();
            if name == ".env"
                || name.starts_with(".env.")
                || name.ends_with(".env")
                || [
                    ".config",
                    ".near-credentials",
                    ".ssh",
                    ".vercel",
                    ".gh",
                    ".aws",
                    "credentials",
                    "keys",
                ]
                .contains(&name.as_str())
            {
                return Err("refused prohibited public-file path".into());
            }
        }
    }
    Ok(())
}

pub fn create_public_dir(path: &Path) -> Result<std::path::PathBuf, AgentError> {
    validate_public_path(path)?;
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(|_| "current directory is unavailable")?.join(path)
    };
    // Resolve the existing ancestor before creating anything, including symlinks.
    let mut ancestor = absolute.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("public directory has no existing parent")?;
    }
    let resolved = ancestor.canonicalize().map_err(|_| "public directory is unavailable")?;
    validate_public_path(&resolved)?;
    std::fs::create_dir_all(path).map_err(|_| "public directory could not be created")?;
    let resolved = path.canonicalize().map_err(|_| "public directory is unavailable")?;
    validate_public_path(&resolved)?;
    Ok(resolved)
}

pub fn write_public(path: &Path, bytes: &[u8]) -> Result<(), AgentError> {
    validate_public_path(path)?;
    if bytes.len() > MAX_PUBLIC_FILE_BYTES {
        return Err("public output exceeds size limit".into());
    }
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("refused symbolic-link public output".into());
    }
    let parent = path.parent().ok_or("public output has no directory")?;
    let resolved = parent.canonicalize().map_err(|_| "public output directory is unavailable")?;
    validate_public_path(&resolved)?;
    std::fs::write(path, bytes).map_err(|_| "public output could not be written".into())
}

pub fn read_public_json<T: DeserializeOwned>(path: &Path) -> Result<T, AgentError> {
    use std::io::Read;
    validate_public_path(path)?;
    let resolved = path.canonicalize().map_err(|_| "public file is unavailable")?;
    validate_public_path(&resolved)?;
    let file = std::fs::File::open(&resolved).map_err(|_| "public file could not be opened")?;
    let mut bytes = Vec::new();
    file.take((MAX_PUBLIC_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "public file could not be read")?;
    if bytes.len() > MAX_PUBLIC_FILE_BYTES {
        return Err("public file exceeds size limit".into());
    }
    near_sdk::serde_json::from_slice(&bytes).map_err(|_| "public file has invalid JSON".into())
}

#[derive(Clone)]
pub struct PublicReader {
    rpc: JsonRpc,
    pub vault: String,
    expected_chain: Option<String>,
    anchor: Arc<Mutex<Option<(String, u64)>>>,
}

impl PublicReader {
    pub fn new(rpc_url: &str, vault: &str) -> Result<Self, AgentError> {
        vault.parse::<near_sdk::AccountId>().map_err(|_| "invalid vault account")?;
        Ok(Self {
            rpc: JsonRpc::new(rpc_url, None, 1024 * 1024)?,
            vault: vault.to_owned(),
            expected_chain: None,
            anchor: Arc::new(Mutex::new(None)),
        })
    }

    pub fn with_chain(mut self, expected: Option<&str>) -> Result<Self, AgentError> {
        if expected.is_some_and(|s| {
            s.is_empty()
                || s.len() > 64
                || !s.is_ascii()
                || s.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        }) {
            return Err("invalid expected chain configuration".into());
        }
        self.expected_chain = expected.map(str::to_owned);
        Ok(self)
    }

    pub async fn final_time(&self) -> Result<(u64, String), AgentError> {
        let status = self.rpc.call("status", json!([])).await?;
        let chain = status["chain_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 64 && s.is_ascii())
            .ok_or("RPC network identity is missing")?;
        if self.expected_chain.as_deref().is_some_and(|expected| expected != chain) {
            return Err("RPC network differs from the configured expected chain".into());
        }
        let result = self.query(&self.vault, "get_chain_time_sec", json!({}), None).await?;
        let time: u64 = decode_view(&result)?;
        let block =
            result["block_hash"].as_str().ok_or("RPC block identity is missing")?.to_owned();
        validate_block_hash(&block)?;
        let height = result["block_height"].as_u64().ok_or("RPC block height is invalid")?;
        *self.anchor.lock().map_err(|_| "RPC anchor is unavailable")? =
            Some((block.clone(), height));
        Ok((time, block))
    }

    pub async fn view<T: DeserializeOwned>(
        &self,
        account: &str,
        method: &str,
        args: Value,
        block: &str,
    ) -> Result<T, AgentError> {
        account.parse::<near_sdk::AccountId>().map_err(|_| "invalid view account")?;
        let result = self.query(account, method, args, Some(block)).await?;
        validate_block_hash(block)?;
        let expected_height = self
            .anchor
            .lock()
            .map_err(|_| "RPC anchor is unavailable")?
            .as_ref()
            .filter(|(hash, _)| hash == block)
            .map(|(_, height)| *height)
            .ok_or("RPC view has no final anchor")?;
        if result["block_hash"].as_str() != Some(block)
            || result["block_height"].as_u64() != Some(expected_height)
        {
            return Err("RPC returned state from a different block".into());
        }
        decode_view(&result)
    }

    async fn query(
        &self,
        account: &str,
        method: &str,
        args: Value,
        block: Option<&str>,
    ) -> Result<Value, AgentError> {
        let encoded = STANDARD.encode(near_sdk::serde_json::to_vec(&args)?);
        let mut params = json!({"request_type":"call_function", "account_id":account,
            "method_name":method, "args_base64":encoded});
        if let Some(block) = block {
            params["block_id"] = json!(block);
        } else {
            params["finality"] = json!("final");
        }
        self.rpc.call("query", params).await
    }
}

pub fn validate_block_hash(value: &str) -> Result<(), AgentError> {
    if value.len() > 64 || bs58::decode(value).into_vec().map_or(true, |bytes| bytes.len() != 32) {
        return Err("RPC block hash is not a 32-byte Base58 identity".into());
    }
    Ok(())
}

fn decode_view<T: DeserializeOwned>(result: &Value) -> Result<T, AgentError> {
    let bytes: Vec<u8> = near_sdk::serde_json::from_value(result["result"].clone())
        .map_err(|_| "RPC view result is not bytes")?;
    if bytes.len() > MAX_PUBLIC_FILE_BYTES {
        return Err("RPC view exceeds size limit".into());
    }
    near_sdk::serde_json::from_slice(&bytes)
        .map_err(|_| "RPC view returned an invalid schema".into())
}
