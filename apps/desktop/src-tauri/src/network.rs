//! Default cross-platform network configuration for agent containers.
//!
//! Goal: zero-config for users — agents can reach the internet but cannot
//! access the host machine or LAN. This is enforced via Docker network
//! settings and iptables rules injected into the container at startup.

use anyhow::Result;

/// Default network profile applied to every agent container.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkPolicy {
    /// Allow outbound access to the internet.
    pub allow_internet: bool,
    /// Block access to the host machine (172.17.0.1 / host.docker.internal).
    pub block_host: bool,
    /// Block access to private LAN ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16).
    pub block_lan: bool,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            allow_internet: true,
            block_host: true,
            block_lan: true,
        }
    }
}

#[allow(dead_code)] // Used when network isolation is wired into provisioning
impl NetworkPolicy {
    /// Generate iptables rules to inject into the container at startup.
    /// These are passed as the container's entrypoint wrapper.
    pub fn iptables_rules(&self) -> Vec<String> {
        let mut rules = Vec::new();

        if self.block_host {
            // Block Docker host gateway
            rules.push("iptables -A OUTPUT -d 172.17.0.1 -j DROP".into());
            rules.push("iptables -A OUTPUT -d host.docker.internal -j DROP".into());
        }

        if self.block_lan {
            rules.push("iptables -A OUTPUT -d 10.0.0.0/8 -j DROP".into());
            rules.push("iptables -A OUTPUT -d 172.16.0.0/12 -j DROP".into());
            rules.push("iptables -A OUTPUT -d 192.168.0.0/16 -j DROP".into());
        }

        if self.allow_internet {
            rules.push("iptables -A OUTPUT -j ACCEPT".into());
        }

        rules
    }

    /// Docker run args fragment to enforce network isolation.
    /// Returns extra args to pass to `docker run`.
    pub fn docker_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // Use a dedicated bridge network for isolation
        args.push("--network".into());
        args.push("agentbox-net".into());

        // Grant NET_ADMIN so the entrypoint can apply iptables rules
        args.push("--cap-add".into());
        args.push("NET_ADMIN".into());

        args
    }

    /// Ensure the Docker bridge network `agentbox-net` exists.
    /// Uses the provided ContainerRuntime to route through VM.
    pub async fn ensure_network_via(docker: &agentbox_docker::ContainerRuntime) -> Result<()> {
        docker.ensure_network("agentbox-net").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_blocks_host_and_lan() {
        let p = NetworkPolicy::default();
        assert!(p.allow_internet);
        assert!(p.block_host);
        assert!(p.block_lan);
    }

    #[test]
    fn docker_args_include_network_and_cap() {
        let p = NetworkPolicy::default();
        let args = p.docker_args();
        assert!(args.contains(&"--network".to_string()));
        assert!(args.contains(&"agentbox-net".to_string()));
        assert!(args.contains(&"--cap-add".to_string()));
        assert!(args.contains(&"NET_ADMIN".to_string()));
    }

    #[test]
    fn iptables_rules_block_host() {
        let p = NetworkPolicy {
            allow_internet: true,
            block_host: true,
            block_lan: false,
        };
        let rules = p.iptables_rules();
        assert!(rules.iter().any(|r| r.contains("172.17.0.1")));
        assert!(!rules.iter().any(|r| r.contains("10.0.0.0")));
    }

    #[test]
    fn iptables_rules_block_lan() {
        let p = NetworkPolicy {
            allow_internet: true,
            block_host: false,
            block_lan: true,
        };
        let rules = p.iptables_rules();
        assert!(rules.iter().any(|r| r.contains("10.0.0.0/8")));
        assert!(rules.iter().any(|r| r.contains("172.16.0.0/12")));
        assert!(rules.iter().any(|r| r.contains("192.168.0.0/16")));
        assert!(!rules.iter().any(|r| r.contains("172.17.0.1")));
    }

    #[test]
    fn iptables_rules_allow_internet() {
        let p = NetworkPolicy {
            allow_internet: true,
            block_host: false,
            block_lan: false,
        };
        let rules = p.iptables_rules();
        assert!(rules.iter().any(|r| r.contains("ACCEPT")));
    }

    #[test]
    fn iptables_rules_empty_when_nothing_enabled() {
        let p = NetworkPolicy {
            allow_internet: false,
            block_host: false,
            block_lan: false,
        };
        let rules = p.iptables_rules();
        assert!(rules.is_empty());
    }
}
