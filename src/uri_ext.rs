use crate::Error;
use once_cell::sync::Lazy;
use std::fmt;

const DEFAULT_PORT_HTTP: u16 = 80;
const DEFAULT_PORT_HTTPS: u16 = 443;
static DEFAULT_URI: Lazy<http::Uri> = Lazy::new(|| http::Uri::from_static("http://localhost/"));

pub trait MethodExt {
    fn indicates_body(&self) -> bool;
}

impl MethodExt for http::Method {
    fn indicates_body(&self) -> bool {
        match *self {
            http::Method::POST | http::Method::PUT | http::Method::PATCH => true,
            _ => false,
        }
    }
}

pub(crate) trait UriExt {
    /// host:port
    fn host_port(&self) -> Result<HostPort, Error>;
    /// Parse a uri relative to some other base uri. We can resolve
    /// a uri containing only a path relative to some uri having a host.
    fn parse_relative(&self, from: &str) -> Result<http::Uri, Error>;
    /// For cookie matching we parent host names. a.b.com -> b.com
    fn parent_host(&self) -> Option<http::Uri>;
    /// Tell if this URI is using a secure protocol (i.e. https).
    fn is_secure(&self) -> bool;
}

impl UriExt for http::Uri {
    fn host_port(&self) -> Result<HostPort, Error> {
        HostPort::from_uri(self)
    }
    fn parse_relative(&self, from: &str) -> Result<http::Uri, Error> {
        let uri_res: Result<http::Uri, http::Error> =
            from.parse::<http::Uri>().map_err(|e| e.into());
        let uri = uri_res?;
        match (uri.scheme(), uri.authority()) {
            (Some(_), Some(_)) => Ok(uri),
            (None, None) => {
                // it's relative to the original url
                let mut parts = uri.into_parts();
                parts.scheme = self.scheme().cloned();
                parts.authority = self.authority().cloned();
                Ok(http::Uri::from_parts(parts).unwrap())
            }
            _ => Err(Error::Proto(format!(
                "Failed to parse '{}' relative to: {}",
                uri, from
            ))),
        }
    }
    fn parent_host(&self) -> Option<http::Uri> {
        let mut parts = self.clone().into_parts();
        let auth = parts.authority?;

        // from the current host, try to figure out a parent host.
        let host = auth.host();
        if !host.contains('.') {
            // no parent to this uri
            return None;
        }
        let parent = host.split('.').skip(1).collect::<Vec<_>>().join(".");

        // http::uri::Authority doesn't give us easy access to this part sadly.
        let upwd = if auth.as_str().contains('@') {
            let upwd: String = auth.as_str().chars().take_while(|c| c != &'@').collect();
            Some(upwd)
        } else {
            None
        };

        // assemble the new authority
        let mut new_auth = parent;
        if let Some(upwd) = upwd {
            new_auth = format!("{}@{}", upwd, new_auth);
        };
        if let Some(port) = auth.port() {
            new_auth = format!("{}:{}", new_auth, port);
        }
        let fake_uri = format!("http://{}", new_auth);
        let new_auth = fake_uri
            .parse::<http::Uri>()
            .expect("Parse fake uri")
            .into_parts()
            .authority;

        // change only the authority of the parts
        parts.authority = new_auth;

        Some(http::Uri::from_parts(parts).expect("Parent uri"))
    }
    fn is_secure(&self) -> bool {
        self.host_port().ok().map(|x| x.is_tls()).unwrap_or(false)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostPort {
    host: String,
    port: u16,
    is_tls: bool,
}

impl HostPort {
    pub fn new(host: &str, port: u16, tls: bool) -> Self {
        HostPort {
            host: host.to_string(),
            port,
            is_tls: tls,
        }
    }
}

impl HostPort {
    pub fn from_uri(uri: &http::Uri) -> Result<Self, Error> {
        let scheme = uri
            .scheme()
            .unwrap_or_else(|| {
                let scheme = DEFAULT_URI.scheme().unwrap();
                debug!("No scheme in URI, using default: {}", scheme);
                scheme
            })
            .as_str();

        let authority = uri
            .authority()
            .unwrap_or_else(|| DEFAULT_URI.authority().unwrap());

        let scheme_default = match scheme {
            "http" => DEFAULT_PORT_HTTP,
            "https" => DEFAULT_PORT_HTTPS,
            _ => return Err(Error::User(format!("Unknown URI scheme: {}", uri))),
        };

        let hostport = HostPort {
            host: authority.host().to_string(),
            port: authority.port_u16().unwrap_or(scheme_default),
            is_tls: scheme == "https",
        };

        Ok(hostport)
    }

    #[cfg(feature = "tls")]
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn is_tls(&self) -> bool {
        self.is_tls
    }
}

impl fmt::Display for HostPort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const PARENT_HOST: &[(&str, Option<&str>)] = &[
        ("http://a.example.com/", Some("http://example.com/")),
        ("http://example.com/", Some("http://com/")),
        ("http://com/", None),
        (
            "http://user:pass@a.example.com:1234/path",
            Some("http://user:pass@example.com:1234/path"),
        ),
        ("/path", None),
    ];

    #[test]
    fn parent_host() {
        for (test, expect) in PARENT_HOST {
            let uri = test.parse::<http::Uri>().unwrap();
            let parent = uri.parent_host();
            assert_eq!(parent.map(|u| u.to_string()), expect.map(|s| s.to_string()));
        }
    }
}
