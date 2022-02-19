use std::sync::Arc;

use crate::request::Request;
use std::time::Duration;
use crate::{error::Error};

pub type Result<T> = std::result::Result<T, Error>;

/// Accumulates options towards building an [Agent].
#[derive(Debug)]
pub struct AgentBuilder {
    config: AgentConfig,
}

/// Config as built by AgentBuilder and then static for the lifetime of the Agent.
#[derive(Debug, Clone)]
pub(crate) struct AgentConfig {
    pub timeout_connect: Duration,
    pub user_agent: String,
    #[cfg(feature = "tls")]
    pub tls_config: Option<TLSClientConfig>,
}

/// can keep a state.
///
///
/// Agent uses an inner Arc, so cloning an Agent results in an instance
/// that shares the same underlying connection pool and other state.
#[derive(Debug, Clone)]
pub struct Agent {
    pub(crate) config: Arc<AgentConfig>,
}

/// Container of the state
///
/// *Internal API*.
#[derive(Debug)]
pub(crate) struct AgentState {
}

impl Agent {
    /// Make a GET request from this agent.
    pub fn get(&self, path: &str) -> Result<Request> {
        let agent = AgentBuilder::new().build();
        Request::new(agent, "GET", path)
    }
}

impl AgentBuilder {
    pub fn new() -> Self {
        AgentBuilder {
            config: AgentConfig {
                timeout_connect: Duration::from_secs(30),
                user_agent: "ureq/2.3.1".into(),
                #[cfg(feature = "tls")]
                tls_config: None,
            },
        }
    }

    /// Create a new agent.
    // Note: This could take &self as the first argument, allowing one
    // AgentBuilder to be used multiple times, except CookieStore does
    // not implement clone, so we have to give ownership to the newly
    // built Agent.
    pub fn build(self) -> Agent {
        Agent {
            config: Arc::new(self.config),
        }
    }
}

#[cfg(feature = "tls")]
#[derive(Clone)]
pub(crate) struct TLSClientConfig(pub(crate) Arc<rustls::ClientConfig>);

#[cfg(feature = "tls")]
impl std::fmt::Debug for TLSClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TLSClientConfig").finish()
    }
}

