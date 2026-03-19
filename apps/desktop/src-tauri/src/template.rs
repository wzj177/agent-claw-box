//! Agent template loader — parses agent.yaml from templates/ directory.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Parsed agent.yaml template definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub name: String,
    pub description: String,
    pub version: String,
    /// Installation method: "docker" | "compose" | "script" | "native"
    pub install_method: String,
    pub runtime: RuntimeConfig,
    pub ports: Vec<PortMapping>,
    #[serde(default)]
    pub volumes: Vec<VolumeMapping>,
    pub resources: ResourceConfig,
    pub health: HealthConfig,
    #[serde(default)]
    pub config_schema: Vec<ConfigField>,
    /// Whether this template is available in the marketplace (default true)
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// For install_method=docker: the image to pull
    #[serde(default)]
    pub image: Option<String>,
    /// For install_method=compose: the compose file name inside template dir
    #[serde(default)]
    pub compose_file: Option<String>,
    /// For install_method=script: base Docker image to use
    #[serde(default)]
    pub base_image: Option<String>,
    /// For install_method=script: install command to run inside container
    #[serde(default)]
    pub install_cmd: Option<String>,
    /// For install_method=script: command to start the service
    #[serde(default)]
    pub start_cmd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMapping {
    /// Suffix appended to the agent data dir (e.g. "copaw-data")
    pub host_suffix: String,
    /// Container path to mount to
    pub container: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    pub cpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    pub url: String,
    pub interval_secs: u64,
}

/// Config field definition for the configuration form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub default: Option<String>,
    /// Environment variable name to inject into container
    #[serde(default)]
    pub env_name: Option<String>,
}

/// Get the templates/ directory path relative to the executable.
/// In development, resolves relative to the project root.
pub fn templates_dir() -> PathBuf {
    // Try the path next to the executable first (production build)
    if let Ok(exe) = std::env::current_exe() {
        let prod_path = exe
            .parent()
            .unwrap_or(Path::new("."))
            .join("templates");
        if prod_path.exists() {
            return prod_path;
        }
    }

    // Development: look for templates/ relative to CARGO_MANIFEST_DIR or cwd
    let dev_candidates = [
        // From apps/desktop/src-tauri/ -> root/templates/
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../templates"),
        PathBuf::from("templates"),
    ];

    for c in &dev_candidates {
        let p = c.canonicalize().unwrap_or_default();
        if p.exists() {
            return p;
        }
    }

    PathBuf::from("templates")
}

/// Load a template by its directory name (e.g. "copaw", "nanobot", "openclaw").
pub fn load_template(template_id: &str) -> Result<AgentTemplate> {
    let dir = templates_dir().join(template_id);
    let yaml_path = dir.join("agent.yaml");

    let content = std::fs::read_to_string(&yaml_path)
        .with_context(|| format!("Failed to read {}", yaml_path.display()))?;

    let template: AgentTemplate = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", yaml_path.display()))?;

    Ok(template)
}

/// List all available template IDs by scanning the templates/ directory.
pub fn list_templates() -> Result<Vec<String>> {
    let dir = templates_dir();
    let mut ids = Vec::new();

    if !dir.exists() {
        return Ok(ids);
    }

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let yaml = entry.path().join("agent.yaml");
            if yaml.exists() {
                if let Some(name) = entry.file_name().to_str() {
                    ids.push(name.to_string());
                }
            }
        }
    }

    ids.sort();
    Ok(ids)
}

/// Load all templates as a Vec of (template_id, AgentTemplate).
pub fn load_all_templates() -> Result<Vec<(String, AgentTemplate)>> {
    let ids = list_templates()?;
    let mut result = Vec::new();
    for id in ids {
        match load_template(&id) {
            Ok(t) => {
                if t.enabled {
                    result.push((id, t));
                } else {
                    tracing::info!(template = %id, "Template disabled, skipping");
                }
            }
            Err(e) => tracing::warn!(template = %id, error = %e, "Skipping invalid template"),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_agent_yaml() {
        let yaml = r#"
name: test-agent
description: A test agent
version: "1.0.0"
install_method: docker
runtime:
  image: "nginx:latest"
ports:
  - host: 3000
    container: 80
resources:
  cpus: 1
  memory_mb: 512
  disk_gb: 5
health:
  url: "http://localhost:80/health"
  interval_secs: 30
"#;
        let tmpl: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(tmpl.name, "test-agent");
        assert_eq!(tmpl.version, "1.0.0");
        assert_eq!(tmpl.install_method, "docker");
        assert_eq!(tmpl.runtime.image, Some("nginx:latest".into()));
        assert_eq!(tmpl.ports.len(), 1);
        assert_eq!(tmpl.ports[0].host, 3000);
        assert_eq!(tmpl.ports[0].container, 80);
        assert_eq!(tmpl.resources.cpus, 1);
        assert_eq!(tmpl.resources.memory_mb, 512);
        assert!(tmpl.config_schema.is_empty());
        assert!(tmpl.volumes.is_empty());
    }

    #[test]
    fn parse_yaml_with_config_schema() {
        let yaml = r#"
name: copaw
description: CoPaw Agent
version: "0.2.0"
install_method: docker
runtime:
  image: "copaw:latest"
ports:
  - host: 8088
    container: 8088
volumes:
  - host_suffix: data
    container: /app/data
resources:
  cpus: 2
  memory_mb: 4096
  disk_gb: 10
health:
  url: "http://localhost:8088/health"
  interval_secs: 30
config_schema:
  - key: api_key
    type: password
    label: API 密钥
    required: true
    env_name: COPAW_API_KEY
  - key: model
    type: select
    label: 模型
    required: false
    options: ["gpt-4", "gpt-3.5-turbo"]
    default: "gpt-4"
    env_name: COPAW_MODEL
"#;
        let tmpl: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(tmpl.name, "copaw");
        assert_eq!(tmpl.config_schema.len(), 2);
        assert_eq!(tmpl.config_schema[0].key, "api_key");
        assert_eq!(tmpl.config_schema[0].field_type, "password");
        assert!(tmpl.config_schema[0].required);
        assert_eq!(tmpl.config_schema[0].env_name, Some("COPAW_API_KEY".into()));
        assert_eq!(tmpl.config_schema[1].options, vec!["gpt-4", "gpt-3.5-turbo"]);
        assert_eq!(tmpl.volumes.len(), 1);
        assert_eq!(tmpl.volumes[0].container, "/app/data");
    }

    #[test]
    fn parse_compose_template() {
        let yaml = r#"
name: nanobot
description: Nanobot Agent
version: "1.0.0"
install_method: compose
runtime:
  compose_file: docker-compose.yml
ports:
  - host: 18790
    container: 18790
resources:
  cpus: 2
  memory_mb: 2048
  disk_gb: 10
health:
  url: "http://localhost:18790/api/v1/health"
  interval_secs: 30
"#;
        let tmpl: AgentTemplate = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(tmpl.install_method, "compose");
        assert_eq!(tmpl.runtime.compose_file, Some("docker-compose.yml".into()));
        assert!(tmpl.runtime.image.is_none());
    }

    #[test]
    fn load_real_templates_from_disk() {
        // This test only passes when run from the project root
        let ids = list_templates();
        if let Ok(ids) = ids {
            // Verify each template can parse
            for id in &ids {
                let tmpl = load_template(id);
                assert!(tmpl.is_ok(), "Failed to parse template {id}: {:?}", tmpl.err());

                let t = tmpl.unwrap();
                assert!(!t.name.is_empty(), "Template {id} has empty name");
                assert!(!t.version.is_empty(), "Template {id} has empty version");
                assert!(
                    ["docker", "compose", "script"].contains(&t.install_method.as_str()),
                    "Template {id} has invalid install_method: {}",
                    t.install_method
                );
                assert!(!t.ports.is_empty(), "Template {id} has no ports");
            }
        }
    }
}
