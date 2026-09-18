//! Where to attach: three forms, one string.

use std::str::FromStr;

use crate::error::ClientError;

/// One of the three ways to reach a kernel.
///
/// ```text
/// inproc:                          the kernel in this process
/// pipe:orrery-<session>            a named pipe or UDS on this machine
/// http://127.0.0.1:7777            AG-UI's own transport
/// http://t0ken@127.0.0.1:7777      …with the bearer token in the authority
/// ```
///
/// The token rides the authority rather than a query parameter because a query
/// string ends up in logs, in shell history and in `ps` output, and a bearer
/// token is the whole of phase 1's auth story.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// The kernel in this process. No serialisation.
    Inproc,
    /// A named pipe or UDS.
    Pipe {
        /// The name to connect to.
        name: String,
    },
    /// HTTP + SSE.
    Http {
        /// The base URL, without any credentials.
        url: String,
        /// The bearer token, when the endpoint carried one.
        token: Option<String>,
    },
}

impl FromStr for Endpoint {
    type Err = ClientError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "inproc:" || s == "inproc" {
            return Ok(Endpoint::Inproc);
        }
        if let Some(name) = s.strip_prefix("pipe:") {
            if name.is_empty() {
                return Err(ClientError::BadEndpoint(s.to_owned()));
            }
            return Ok(Endpoint::Pipe {
                name: name.to_owned(),
            });
        }
        for scheme in ["http://", "https://"] {
            if let Some(rest) = s.strip_prefix(scheme) {
                let (token, host) = match rest.split_once('@') {
                    Some((token, host)) if !token.is_empty() && !host.is_empty() => {
                        (Some(token.to_owned()), host)
                    }
                    Some(_) => return Err(ClientError::BadEndpoint(s.to_owned())),
                    None => (None, rest),
                };
                if host.is_empty() {
                    return Err(ClientError::BadEndpoint(s.to_owned()));
                }
                return Ok(Endpoint::Http {
                    url: format!("{scheme}{}", host.trim_end_matches('/')),
                    token,
                });
            }
        }
        Err(ClientError::BadEndpoint(s.to_owned()))
    }
}

impl std::fmt::Display for Endpoint {
    /// Never prints the token. An endpoint in a log is an endpoint in a bug
    /// report.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endpoint::Inproc => f.write_str("inproc:"),
            Endpoint::Pipe { name } => write!(f, "pipe:{name}"),
            Endpoint::Http { url, token: None } => f.write_str(url),
            Endpoint::Http { url, .. } => {
                let (scheme, host) = url.split_once("//").unwrap_or(("http:", url));
                write!(f, "{scheme}//<token>@{host}")
            }
        }
    }
}
