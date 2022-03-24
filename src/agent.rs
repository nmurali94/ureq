#[cfg(feature = "tls")]
use once_cell::sync::Lazy;
use std::sync::Arc;

use crate::request::{Request, call_urls};
use crate::{error::Error};
use crate::stream::Stream;
use crate::response::Response;

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
    pub fn get(&self, path: &str) -> Result<Request> {
        let agent = Agent::build();
        Request::new(agent, path)
    }
    /// Make a GET request from this agent.
    pub fn get_multiple(&self, urls: Vec<String>) -> Result<Vec<Stream>> {
        let agent = Agent::build();
		call_urls(agent, urls)
    }
    /// Make a GET request from this agent.
    pub fn get_response(&self, stream: Stream) -> Result<Response> {
		Response::do_from_stream(stream)
    }
}

impl Agent {
    pub fn build() -> Agent {
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
    #[cfg(not(feature = "native-tls"))]
    root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));
    #[cfg(feature = "native-tls")]
    root_store.add_server_trust_anchors(
        rustls_native_certs::load_native_certs().expect("Could not load platform certs"),
    );

    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Arc::new(config)
});
