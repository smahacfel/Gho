//! UI configuration bridge for root config files.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

const ROOT_CONFIG_FILE: &str = "config.toml";
const GHOST_BRAIN_CONFIG_FILE: &str = "ghost-brain/ghost_brain_config.toml";
const DEFAULT_SECRET_ENV_FILE: &str = ".env";
const GHOST_ENV_FILE_VAR: &str = "GHOST_ENV_FILE";
const GHOST_TRIGGER_RPC_URL_VAR: &str = "GHOST_TRIGGER_RPC_URL";
const GHOST_TRIGGER_KEYPAIR_PATH_VAR: &str = "GHOST_TRIGGER_KEYPAIR_PATH";

const MAIN_FIELDS: [&str; 14] = [
    "logging.level",
    "trigger.jito_tip.base_tip_percent",
    "trigger.jito_tip.dynamic_tip_percent",
    "trigger.jito_tip.max_tip_percent",
    "trigger.jito_tip.max_tip_absolute_sol",
    "trigger.jito_tip.max_tip_ratio_percent",
    "trigger.jito_tip.fallback_tip_sol",
    "seer.source_mode",
    "seer.enable_pumpfun",
    "seer.enable_raydium",
    "trigger.max_position_size_sol",
    "trigger.slippage_tolerance",
    "trigger.max_concurrent_positions",
    "trigger.dry_run",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletRuntimeContext {
    pub rpc_url: String,
    pub keypair_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UiValue {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigResponse {
    pub main: BTreeMap<String, UiValue>,
    pub gatekeeper_v2: BTreeMap<String, UiValue>,
    pub iwim: BTreeMap<String, UiValue>,
    pub iwim_veto_gate: BTreeMap<String, UiValue>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemConfigUpdateRequest {
    pub main: BTreeMap<String, UiValue>,
    pub gatekeeper_v2: BTreeMap<String, UiValue>,
    pub iwim: BTreeMap<String, UiValue>,
    pub iwim_veto_gate: BTreeMap<String, UiValue>,
}

pub struct UiConfigStore {
    root_config_path: PathBuf,
    ghost_brain_config_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum SecretValueSource {
    ProcessEnv,
    DotEnv,
}

#[derive(Debug, Clone)]
struct ResolvedSecretValue {
    value: String,
    source: SecretValueSource,
}

#[derive(Debug, Default, Clone)]
struct LoadedSecretEnv {
    values: HashMap<String, String>,
    base_dir: Option<PathBuf>,
}

impl Default for UiConfigStore {
    fn default() -> Self {
        let workspace_root = detect_workspace_root();
        Self {
            root_config_path: workspace_root.join(ROOT_CONFIG_FILE),
            ghost_brain_config_path: workspace_root.join(GHOST_BRAIN_CONFIG_FILE),
        }
    }
}

impl UiConfigStore {
    pub fn load_system_config(&self) -> Result<SystemConfigResponse> {
        let root_doc = self.load_doc(&self.root_config_path)?;
        let brain_doc = self.load_doc(&self.ghost_brain_config_path)?;

        Ok(SystemConfigResponse {
            main: read_main_fields(&root_doc)?,
            gatekeeper_v2: read_table_scalars(&brain_doc, "gatekeeper_v2")?,
            iwim: read_table_scalars(&brain_doc, "iwim")?,
            iwim_veto_gate: read_table_scalars(&brain_doc, "iwim_veto_gate")?,
        })
    }

    pub fn save_system_config(
        &self,
        request: SystemConfigUpdateRequest,
    ) -> Result<SystemConfigResponse> {
        validate_main_keys(&request.main)?;

        let mut root_doc = self.load_doc(&self.root_config_path)?;
        let mut brain_doc = self.load_doc(&self.ghost_brain_config_path)?;

        for (path, value) in request.main {
            let segments: Vec<&str> = path.split('.').collect();
            set_path_value(&mut root_doc, &segments, value)?;
        }

        write_table_scalars(&mut brain_doc, "gatekeeper_v2", request.gatekeeper_v2)?;
        write_table_scalars(&mut brain_doc, "iwim", request.iwim)?;
        write_table_scalars(&mut brain_doc, "iwim_veto_gate", request.iwim_veto_gate)?;

        std::fs::write(&self.root_config_path, root_doc.to_string())
            .with_context(|| format!("Failed to write {}", self.root_config_path.display()))?;
        std::fs::write(&self.ghost_brain_config_path, brain_doc.to_string()).with_context(
            || format!("Failed to write {}", self.ghost_brain_config_path.display()),
        )?;

        self.load_system_config()
    }

    pub fn load_wallet_runtime_context(&self) -> Result<WalletRuntimeContext> {
        let root_doc = self.load_doc(&self.root_config_path)?;
        let config_dir = self
            .root_config_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let secret_env = load_secret_env(config_dir)?;

        let rpc_url = lookup_secret_env(GHOST_TRIGGER_RPC_URL_VAR, &secret_env)
            .map(|resolved| resolved.value)
            .unwrap_or(read_string_path(&root_doc, &["trigger", "rpc_url"])?);

        let keypair_path = if let Some(resolved) =
            lookup_secret_env(GHOST_TRIGGER_KEYPAIR_PATH_VAR, &secret_env)
        {
            resolve_secret_path_value(&resolved, config_dir, &secret_env)
        } else {
            let path = read_string_path(&root_doc, &["trigger", "keypair_path"])?;
            resolve_config_path_value(&path, config_dir)
        };

        Ok(WalletRuntimeContext {
            rpc_url,
            keypair_path,
        })
    }

    fn load_doc(&self, path: &Path) -> Result<DocumentMut> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        content
            .parse::<DocumentMut>()
            .with_context(|| format!("Failed to parse TOML {}", path.display()))
    }
}

fn detect_workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.to_path_buf())
}

fn load_secret_env(config_dir: &Path) -> Result<LoadedSecretEnv> {
    if let Ok(env_file) = env::var(GHOST_ENV_FILE_VAR) {
        let env_path = PathBuf::from(env_file.trim());
        let env_path = if env_path.is_absolute() {
            env_path
        } else {
            config_dir.join(env_path)
        };

        if env_path.exists() {
            return parse_secret_env_file(&env_path);
        }

        return Ok(LoadedSecretEnv::default());
    }

    for ancestor in config_dir.ancestors() {
        let candidate = ancestor.join(DEFAULT_SECRET_ENV_FILE);
        if candidate.exists() {
            return parse_secret_env_file(&candidate);
        }
    }

    Ok(LoadedSecretEnv::default())
}

fn parse_secret_env_file(path: &Path) -> Result<LoadedSecretEnv> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read dotenv file {}", path.display()))?;
    let mut values = HashMap::new();
    for (line_no, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, raw_value)) = line.split_once('=') else {
            return Err(anyhow!(
                "Invalid dotenv entry at {}:{}",
                path.display(),
                line_no + 1
            ));
        };

        let key = key.trim();
        if key.is_empty() {
            return Err(anyhow!(
                "Invalid dotenv entry with empty key at {}:{}",
                path.display(),
                line_no + 1
            ));
        }
        values.insert(key.to_string(), parse_secret_env_value(raw_value));
    }

    Ok(LoadedSecretEnv {
        values,
        base_dir: path.parent().map(Path::to_path_buf),
    })
}

fn parse_secret_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn lookup_secret_env(var_name: &str, secret_env: &LoadedSecretEnv) -> Option<ResolvedSecretValue> {
    if let Ok(value) = env::var(var_name) {
        return Some(ResolvedSecretValue {
            value,
            source: SecretValueSource::ProcessEnv,
        });
    }

    secret_env
        .values
        .get(var_name)
        .cloned()
        .map(|value| ResolvedSecretValue {
            value,
            source: SecretValueSource::DotEnv,
        })
}

fn resolve_secret_path_value(
    resolved: &ResolvedSecretValue,
    config_dir: &Path,
    secret_env: &LoadedSecretEnv,
) -> String {
    let base_dir = match resolved.source {
        SecretValueSource::ProcessEnv => config_dir,
        SecretValueSource::DotEnv => secret_env.base_dir.as_deref().unwrap_or(config_dir),
    };
    resolve_config_path_value(&resolved.value, base_dir)
}

fn resolve_config_path_value(raw_path: &str, base_dir: &Path) -> String {
    let path = Path::new(raw_path);
    if path.is_relative() {
        base_dir.join(path).display().to_string()
    } else {
        raw_path.to_string()
    }
}

fn validate_main_keys(main: &BTreeMap<String, UiValue>) -> Result<()> {
    let allowed: HashSet<&str> = MAIN_FIELDS.iter().copied().collect();
    for key in main.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(anyhow!("Unsupported main config key: {}", key));
        }
    }
    Ok(())
}

fn read_main_fields(doc: &DocumentMut) -> Result<BTreeMap<String, UiValue>> {
    let mut result = BTreeMap::new();
    for field in MAIN_FIELDS {
        let segments: Vec<&str> = field.split('.').collect();
        let item = get_path_item(doc.as_item(), &segments)
            .ok_or_else(|| anyhow!("Missing required config key: {}", field))?;
        let value = item_to_ui_value(item)
            .ok_or_else(|| anyhow!("Unsupported value type for key: {}", field))?;
        result.insert(field.to_string(), value);
    }
    Ok(result)
}

fn read_table_scalars(doc: &DocumentMut, table_name: &str) -> Result<BTreeMap<String, UiValue>> {
    let table_item = doc
        .as_item()
        .get(table_name)
        .ok_or_else(|| anyhow!("Missing table [{}]", table_name))?;

    let table = table_item
        .as_table()
        .ok_or_else(|| anyhow!("Table [{}] is not a standard TOML table", table_name))?;

    let mut result = BTreeMap::new();
    for (key, item) in table.iter() {
        if let Some(value) = item_to_ui_value(item) {
            result.insert(key.to_string(), value);
        }
    }
    Ok(result)
}

fn read_string_path(doc: &DocumentMut, path: &[&str]) -> Result<String> {
    let item = get_path_item(doc.as_item(), path)
        .ok_or_else(|| anyhow!("Missing required config key: {}", path.join(".")))?;

    item.as_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("Config key '{}' is not a string", path.join(".")))
}

fn write_table_scalars(
    doc: &mut DocumentMut,
    table_name: &str,
    values: BTreeMap<String, UiValue>,
) -> Result<()> {
    for (key, value) in values {
        set_path_value(doc, &[table_name, &key], value)?;
    }
    Ok(())
}

fn get_path_item<'a>(root: &'a Item, path: &[&str]) -> Option<&'a Item> {
    let mut current = root;
    for segment in path {
        current = current.get(segment)?;
    }
    Some(current)
}

fn set_path_value(doc: &mut DocumentMut, path: &[&str], field_value: UiValue) -> Result<()> {
    if path.is_empty() {
        return Err(anyhow!("Empty path cannot be set"));
    }

    let mut table = doc.as_table_mut();
    for segment in &path[..path.len() - 1] {
        if !table.contains_key(segment) {
            table.insert(segment, Item::Table(Table::new()));
        }

        let next = table
            .get_mut(segment)
            .ok_or_else(|| anyhow!("Missing table segment: {}", segment))?;

        if !next.is_table() {
            return Err(anyhow!("Config path segment '{}' is not a table", segment));
        }

        table = next
            .as_table_mut()
            .ok_or_else(|| anyhow!("Failed to open table segment: {}", segment))?;
    }

    let leaf = path[path.len() - 1];
    table[leaf] = match field_value {
        UiValue::String(v) => value(v),
        UiValue::Integer(v) => value(v),
        UiValue::Float(v) => value(v),
        UiValue::Bool(v) => value(v),
    };
    Ok(())
}

fn item_to_ui_value(item: &Item) -> Option<UiValue> {
    let value = item.as_value()?;

    if let Some(v) = value.as_bool() {
        return Some(UiValue::Bool(v));
    }
    if let Some(v) = value.as_integer() {
        return Some(UiValue::Integer(v));
    }
    if let Some(v) = value.as_float() {
        return Some(UiValue::Float(v));
    }
    if let Some(v) = value.as_str() {
        return Some(UiValue::String(v.to_string()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ghost_ui_config_{name}_{nanos}"));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    #[test]
    fn wallet_runtime_context_prefers_dotenv_and_rebases_relative_keypair_path() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        std::env::remove_var(GHOST_ENV_FILE_VAR);
        std::env::remove_var(GHOST_TRIGGER_RPC_URL_VAR);
        std::env::remove_var(GHOST_TRIGGER_KEYPAIR_PATH_VAR);

        let workspace_root = unique_temp_dir("dotenv_wallet_runtime");
        let config_dir = workspace_root.join("configs/rollout");
        std::fs::create_dir_all(&config_dir).expect("failed to create config dir");
        std::fs::create_dir_all(workspace_root.join("ghost-brain"))
            .expect("failed to create ghost-brain dir");
        std::fs::create_dir_all(workspace_root.join("wallets"))
            .expect("failed to create wallets dir");

        std::fs::write(
            workspace_root.join("config.toml"),
            "[trigger]\nrpc_url = \"https://config.invalid\"\nkeypair_path = \"wallets/fallback.json\"\n",
        )
        .expect("failed to write root config");
        std::fs::write(
            workspace_root.join("ghost-brain/ghost_brain_config.toml"),
            "[gatekeeper_v2]\nenabled = true\n[iwim]\nenabled = true\n[iwim_veto_gate]\nenabled = true\n",
        )
        .expect("failed to write ghost-brain config");
        std::fs::write(
            workspace_root.join(".env"),
            "GHOST_TRIGGER_RPC_URL=https://env.rpc.example/?api-key=secret\nGHOST_TRIGGER_KEYPAIR_PATH=wallets/hot.json\n",
        )
        .expect("failed to write dotenv");

        let store = UiConfigStore {
            root_config_path: workspace_root.join("config.toml"),
            ghost_brain_config_path: workspace_root.join("ghost-brain/ghost_brain_config.toml"),
        };

        let context = store
            .load_wallet_runtime_context()
            .expect("wallet runtime context should load");

        assert_eq!(context.rpc_url, "https://env.rpc.example/?api-key=secret");
        assert_eq!(
            context.keypair_path,
            workspace_root
                .join("wallets/hot.json")
                .display()
                .to_string()
        );

        std::fs::remove_dir_all(&workspace_root).expect("failed to clean temp dir");
    }
}
