//! The network, which the broker does not reach on its own.
//!
//! There is no transport in this crate. [`Broker::net`](crate::Broker::net)
//! answers [`BrokerError::NoTransport`] until something installs one, so a
//! default build — and every test in this repository — cannot dial anywhere at
//! all. The seam is a trait so a deployment chooses its own client, its own TLS
//! and its own proxy rules, and so the tests can drive a recorded one.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::creds::RequestSlot;
use crate::error::BrokerError;

/// A request, either plain or already authorised in a [`RequestSlot`].
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum NetRequest {
    /// A request with nothing secret in it.
    Plain {
        /// The method.
        method: String,
        /// Where to.
        url: String,
        /// Headers, all of them readable.
        headers: BTreeMap<String, String>,
        /// The body.
        body: Vec<u8>,
    },
    /// A request a credential has been applied to. Its header values are not
    /// readable by the caller; only a transport ever sees them.
    Prepared(RequestSlot),
}

impl NetRequest {
    /// A GET.
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        NetRequest::Plain {
            method: "GET".to_owned(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    /// Where this is going, which is what a `net(domain: ...)` rule matched.
    #[must_use]
    pub fn url(&self) -> String {
        match self {
            NetRequest::Plain { url, .. } => url.clone(),
            NetRequest::Prepared(slot) => slot.url(),
        }
    }

    /// The host, for the token's scope check.
    #[must_use]
    pub fn host(&self) -> String {
        let url = self.url();
        let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
        rest.split(['/', '?', '#'])
            .next()
            .unwrap_or(rest)
            .split('@')
            .next_back()
            .unwrap_or(rest)
            .split(':')
            .next()
            .unwrap_or(rest)
            .to_owned()
    }

    pub(crate) fn parts(&self) -> (String, String, BTreeMap<String, String>, Vec<u8>) {
        match self {
            NetRequest::Plain {
                method,
                url,
                headers,
                body,
            } => (method.clone(), url.clone(), headers.clone(), body.clone()),
            NetRequest::Prepared(slot) => slot.take(),
        }
    }
}

/// What came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetResponse {
    /// The status code.
    pub status: u16,
    /// The headers.
    pub headers: BTreeMap<String, String>,
    /// The body, already bounded by the budget.
    pub body: Vec<u8>,
    /// Whether the ceiling cut the body short.
    pub truncated: bool,
}

/// Something that can actually send a request.
#[async_trait]
pub trait NetTransport: Send + Sync + std::fmt::Debug {
    /// Send it.
    ///
    /// # Errors
    ///
    /// Whatever the transport could not do.
    async fn send(
        &self,
        method: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: &[u8],
        ceiling: u64,
    ) -> Result<NetResponse, BrokerError>;
}

/// The default: there is no transport, and nothing is dialled.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTransport;

#[async_trait]
impl NetTransport for NoTransport {
    async fn send(
        &self,
        _method: &str,
        _url: &str,
        _headers: &BTreeMap<String, String>,
        _body: &[u8],
        _ceiling: u64,
    ) -> Result<NetResponse, BrokerError> {
        Err(BrokerError::NoTransport)
    }
}
