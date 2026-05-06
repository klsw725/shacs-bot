use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapabilityStatus {
    Available,
    Unavailable,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilityReport {
    pub component: String,
    pub status: RuntimeCapabilityStatus,
    pub reason: String,
}

impl RuntimeCapabilityReport {
    pub fn unsupported(component: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            status: RuntimeCapabilityStatus::Unsupported,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct McpLifecycle {
    configured_servers: usize,
    connected_servers: usize,
    failed_servers: usize,
}

impl McpLifecycle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn configured(configured_servers: usize) -> Self {
        Self {
            configured_servers,
            connected_servers: 0,
            failed_servers: 0,
        }
    }

    pub fn from_counts(
        configured_servers: usize,
        connected_servers: usize,
        failed_servers: usize,
    ) -> Self {
        Self {
            configured_servers,
            connected_servers,
            failed_servers,
        }
    }

    pub fn status(&self) -> RuntimeCapabilityReport {
        if self.configured_servers == 0 {
            RuntimeCapabilityReport {
                component: "mcp_lifecycle".to_owned(),
                status: RuntimeCapabilityStatus::Unavailable,
                reason: "No MCP servers configured".to_owned(),
            }
        } else {
            RuntimeCapabilityReport {
                component: "mcp_lifecycle".to_owned(),
                status: RuntimeCapabilityStatus::Available,
                reason: format!(
                    "MCP lifecycle available: {} configured, {} connected, {} failed",
                    self.configured_servers, self.connected_servers, self.failed_servers
                ),
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct DreamLifecycle {
    configured: bool,
}

impl DreamLifecycle {
    pub fn new() -> Self {
        Self { configured: false }
    }

    pub fn configured() -> Self {
        Self { configured: true }
    }

    pub fn status(&self) -> RuntimeCapabilityReport {
        if self.configured {
            RuntimeCapabilityReport {
                component: "dream_lifecycle".to_owned(),
                status: RuntimeCapabilityStatus::Available,
                reason: "Dream memory processor can run with a configured provider".to_owned(),
            }
        } else {
            RuntimeCapabilityReport {
                component: "dream_lifecycle".to_owned(),
                status: RuntimeCapabilityStatus::Unavailable,
                reason: "Dream memory processor requires a configured provider".to_owned(),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSelectionSnapshot {
    pub provider_id: String,
    pub model: String,
}

impl ProviderSelectionSnapshot {
    pub fn new(provider_id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model: model.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderHotSwapResult {
    Unsupported { current: ProviderSelectionSnapshot },
}

#[derive(Debug, Clone)]
pub struct StaticProviderSelector {
    current_turn: ProviderSelectionSnapshot,
}

impl StaticProviderSelector {
    pub fn new(current_turn: ProviderSelectionSnapshot) -> Self {
        Self { current_turn }
    }

    pub fn current_turn(&self) -> &ProviderSelectionSnapshot {
        &self.current_turn
    }

    pub fn select_snapshot(&self) -> ProviderSelectionSnapshot {
        self.current_turn.clone()
    }

    pub fn request_hot_swap(&mut self, _next: ProviderSelectionSnapshot) -> ProviderHotSwapResult {
        ProviderHotSwapResult::Unsupported {
            current: self.current_turn.clone(),
        }
    }
}
