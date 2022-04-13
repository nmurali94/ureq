#[cfg(feature = "tls")]
use once_cell::sync::Lazy;
use std::sync::Arc;

use crate::error::Error;
use crate::request::Request;
use crate::response::Response;
use crate::url::Url;

pub type Result<T> = std::result::Result<T, Error>;

/// Config as built by AgentBuilder and then static for the lifetime of the Agent.
pub struct Agent {
    pub user_agent: Arc<str>,
    #[cfg(feature = "tls")]
    pub tls_config: Arc<rustls::ClientConfig>,
}

/// Container of the state
///

impl Agent {
    /// Make a GET request from this agent.
    pub fn get(u: Url) -> Result<Response> {
        let agent = Agent::build();
        Request::call(agent, u)
    }
    fn build() -> Agent {
        Agent {
            user_agent: Arc::from("ureq/2.3.1"),
            #[cfg(feature = "tls")]
            tls_config: TLS_CONF.clone(),
        }
    }
}

#[cfg(feature = "tls")]
static TLS_CONF: Lazy<Arc<rustls::ClientConfig>> = Lazy::new(|| {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));

    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Arc::new(config)
});
