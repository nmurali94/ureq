use std::sync::Arc;

use crate::proxy::Proxy;
use crate::request::Request;
use std::time::Duration;

/// Accumulates options towards building an [Agent].
#[derive(Debug)]
pub struct AgentBuilder {
    config: AgentConfig,
}

/// Config as built by AgentBuilder and then static for the lifetime of the Agent.
#[derive(Debug, Clone)]
pub(crate) struct AgentConfig {
    pub proxy: Option<Proxy>,
    pub timeout_connect: Option<Duration>,
    pub timeout_read: Option<Duration>,
    pub timeout_write: Option<Duration>,
    pub timeout: Option<Duration>,
    pub redirects: u32,
    pub user_agent: String,
    #[cfg(feature = "tls")]
    pub tls_config: Option<TLSClientConfig>,
}

/// can keep a state.
///
/// ```
/// # fn main() -> Result<(), ureq::Error> {
/// # ureq::is_test(true);
/// let mut agent = ureq::agent();
///
/// agent
///     .post("http://example.com/login")
///     .call()?;
///
/// let secret = agent
///     .get("http://example.com/my-protected-page")
///     .call()?
///     .into_string()?;
///
///   println!("Secret is: {}", secret);
/// # Ok(())
/// # }
/// ```
///
/// Agent uses an inner Arc, so cloning an Agent results in an instance
/// that shares the same underlying connection pool and other state.
#[derive(Debug, Clone)]
pub struct Agent {
    pub(crate) config: Arc<AgentConfig>,
    /// Reused agent state for repeated requests from this agent.
    pub(crate) state: Arc<AgentState>,
}

/// Container of the state
///
/// *Internal API*.
#[derive(Debug)]
pub(crate) struct AgentState {
}

impl Agent {
    /// Make a GET request from this agent.
    pub fn get(&self, path: &str) -> Request {
        let agent = AgentBuilder::new().build();
        Request::new(agent, "GET", path)
    }
}

impl AgentBuilder {
    pub fn new() -> Self {
        AgentBuilder {
            config: AgentConfig {
                proxy: None,
                timeout_connect: Some(Duration::from_secs(30)),
                timeout_read: None,
                timeout_write: None,
                timeout: None,
                redirects: 5,
                user_agent: format!("ureq/{}", env!("CARGO_PKG_VERSION")),
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
            state: Arc::new(AgentState {
            }),
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

#[cfg(test)]
mod tests {
    use super::*;

    ///////////////////// AGENT TESTS //////////////////////////////

    #[test]
    fn agent_implements_send_and_sync() {
        let _agent: Box<dyn Send> = Box::new(AgentBuilder::new().build());
        let _agent: Box<dyn Sync> = Box::new(AgentBuilder::new().build());
    }
}
