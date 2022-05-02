use once_cell::sync::Lazy;

#[cfg(feature = "tls")]
use std::sync::Arc;

use crate::error::Error;
use crate::request::Request;
use crate::response::Response;
use crate::url::Url;

pub type Result<T> = std::result::Result<T, Error>;

static USER_AGENT: Lazy<Agent> = Lazy::new(|| {
    #[cfg(feature = "tls")]
    let tls_config = {
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
    };
    Agent {
        user_agent: "ureq/2.3.1",
        #[cfg(feature = "tls")]
        tls_config,
    }
});

/// Config as built by AgentBuilder and then static for the lifetime of the Agent.
pub struct Agent {
    pub user_agent: &'static str,
    #[cfg(feature = "tls")]
    pub tls_config: Arc<rustls::ClientConfig>,
}

impl Agent {
    /// Make a GET request from this agent.
    pub fn get(u: &Url) -> Result<Response> {
        Request::call(&USER_AGENT, u)
    }
}
