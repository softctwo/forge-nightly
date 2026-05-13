use std::collections::{HashMap, hash_map};

use serde::{Deserialize, Serialize};

use crate::{ServerName, ToolDefinition};

/// Describes a single MCP server whose connection was blocked by the default
/// permission policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpPermissionWarning {
    /// Name of the server as declared in the config file.
    pub server_name: ServerName,
}

/// Cache for MCP tool definitions
///
/// Simplified cache structure that stores only the essential data.
/// Validation and TTL checking are handled by the infrastructure layer
/// using cacache's built-in metadata capabilities.
#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, derive_setters::Setters)]
#[serde(rename_all = "camelCase")]
#[setters(strip_option, into)]
pub struct McpServers {
    /// Successfully loaded MCP servers with their tools
    servers: HashMap<ServerName, Vec<ToolDefinition>>,
    /// Failed MCP servers with their error messages
    #[serde(default)]
    failures: HashMap<ServerName, String>,
    /// Servers that were denied by the permission policy, one entry per
    /// blocked server. The UI uses these to emit a structured warning.
    #[serde(default)]
    warnings: Vec<McpPermissionWarning>,
}

impl McpServers {
    /// Create a new cache entry with servers and failures
    pub fn new(
        servers: HashMap<ServerName, Vec<ToolDefinition>>,
        failures: HashMap<ServerName, String>,
    ) -> Self {
        Self { servers, failures, warnings: Vec::new() }
    }

    /// Get the successful servers
    pub fn get_servers(&self) -> &HashMap<ServerName, Vec<ToolDefinition>> {
        &self.servers
    }

    /// Get the failed servers
    pub fn get_failures(&self) -> &HashMap<ServerName, String> {
        &self.failures
    }

    /// Get the permission-denied warnings, one entry per blocked server
    pub fn get_warnings(&self) -> &[McpPermissionWarning] {
        &self.warnings
    }
}

impl IntoIterator for McpServers {
    type Item = (ServerName, Vec<ToolDefinition>);
    type IntoIter = hash_map::IntoIter<ServerName, Vec<ToolDefinition>>;

    fn into_iter(self) -> Self::IntoIter {
        self.servers.into_iter()
    }
}
