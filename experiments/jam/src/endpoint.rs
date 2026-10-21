//! One canonical endpoint interpretation for clients and proxy-backed relays.
use crate::client::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    url: String,
    authority: String,
    remote: bool,
}
impl Endpoint {
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.len() > 2048 || !input.is_ascii() || input.contains(['\\', '@']) {
            return Err(Error::Invalid);
        }
        let url = url::Url::parse(input).map_err(|_| Error::Invalid)?;
        let remote = url.scheme() == "https";
        if !(remote
            || (url.scheme() == "http"
                && url.host_str() == Some("127.0.0.1")
                && url.port().is_some()))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port() == Some(0)
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Invalid);
        }
        // Do not silently normalize dot paths, alternate IPv4 notation, spaces,
        // default ports, case or URL parser repairs in security-sensitive input.
        let canonical = url.as_str().strip_suffix('/').ok_or(Error::Invalid)?;
        if input != canonical && input != url.as_str() {
            return Err(Error::Invalid);
        }
        Ok(Self {
            url: canonical.into(),
            authority: url[url::Position::BeforeHost..url::Position::AfterPort].into(),
            remote,
        })
    }
    pub fn as_str(&self) -> &str {
        &self.url
    }
    pub fn authority(&self) -> &str {
        &self.authority
    }
    pub fn is_remote(&self) -> bool {
        self.remote
    }
}
