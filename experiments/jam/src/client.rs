use crate::endpoint::Endpoint;
use crate::protocol::*;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Invalid,
    Transport,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "Jam room or membership is unavailable",
            Self::Invalid => "Jam protocol validation failed",
            Self::Transport => "Jam relay did not respond safely",
        })
    }
}
impl std::error::Error for Error {}

pub struct Client {
    endpoint: Endpoint,
    agent: ureq::Agent,
}
impl Client {
    pub fn new(endpoint: &str) -> Result<Self, Error> {
        Self::build(
            endpoint,
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .root_certs(ureq::tls::RootCerts::WebPki)
                .build(),
        )
    }
    fn build(endpoint: &str, tls: ureq::tls::TlsConfig) -> Result<Self, Error> {
        let endpoint = Endpoint::parse(endpoint)?;
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(3)))
            .max_idle_connections(0)
            .max_response_header_size(8192)
            .input_buffer_size(4096)
            .output_buffer_size(4096)
            .build()
            .into();
        Ok(Self { endpoint, agent })
    }
    pub fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        auth: Option<&Secret>,
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        if body.len() > MAX_BODY {
            return Err(Error::Invalid);
        }
        let mut request = ureq::http::Request::builder()
            .method(method)
            .uri(format!("{}{path}", self.endpoint.as_str()))
            .header("x-tide-jam-protocol", "1")
            .header("content-type", "application/json")
            .body(body)
            .map_err(|_| Error::Invalid)?;
        if let Some(auth) = auth {
            let value = Zeroizing::new(format!("Bearer {}", auth.expose()));
            let mut header =
                ureq::http::HeaderValue::from_str(&value).map_err(|_| Error::Invalid)?;
            header.set_sensitive(true);
            request
                .headers_mut()
                .insert(ureq::http::header::AUTHORIZATION, header);
        }
        let mut response = self.agent.run(request).map_err(|error| match error {
            ureq::Error::StatusCode(404 | 401 | 403) => Error::Unavailable,
            _ => Error::Transport,
        })?;
        if !response.status().is_success() {
            return Err(Error::Transport);
        }
        response
            .body_mut()
            .with_config()
            .limit(MAX_BODY as u64)
            .read_to_vec()
            .map_err(|_| Error::Transport)
    }
    fn json<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        auth: Option<&Secret>,
        body: &[u8],
    ) -> Result<T, Error> {
        let bytes = Zeroizing::new(self.request(method, path, auth, body)?);
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)
    }
    fn path(room: &str, suffix: &str) -> Result<String, Error> {
        if !valid_hex(room, 32) {
            return Err(Error::Invalid);
        }
        Ok(format!("/v1/rooms/{room}{suffix}"))
    }
    /// Shared HTTPS service: no embedded or user-distributed operator key.
    pub fn create_shared(&self) -> Result<Created, Error> {
        if !self.endpoint.is_remote() {
            return Err(Error::Invalid);
        }
        self.create_inner(None)
    }
    pub fn create(&self) -> Result<Created, Error> {
        if self.endpoint.is_remote() {
            return Err(Error::Invalid);
        }
        self.create_inner(None)
    }
    /// The operator key is scoped to this explicitly selected relay and this
    /// single operation. It is never retained in the agent or sent by join/poll.
    pub fn create_with_key(&self, operator: &Secret) -> Result<Created, Error> {
        self.create_inner(Some(operator))
    }
    fn create_inner(&self, operator: Option<&Secret>) -> Result<Created, Error> {
        let result: Created = self.json("POST", "/v1/rooms", operator, b"{}")?;
        if !valid_hex(&result.room, 32) || result.host.matches(&result.invite) {
            return Err(Error::Invalid);
        }
        Ok(result)
    }
    pub fn join(&self, invite: &Invite) -> Result<Joined, Error> {
        // An invite cannot silently redirect an already selected client.
        let endpoint = Client::new(&invite.endpoint)?;
        if endpoint.endpoint != self.endpoint {
            return Err(Error::Invalid);
        }
        self.json(
            "POST",
            &Self::path(&invite.room, "/join")?,
            Some(&invite.invite),
            b"{}",
        )
    }
    pub fn publish(&self, room: &str, host: &Secret, timeline: &Timeline) -> Result<(), Error> {
        timeline.validate().map_err(|_| Error::Invalid)?;
        let bytes = serde_json::to_vec(timeline).map_err(|_| Error::Invalid)?;
        self.request("PUT", &Self::path(room, "/state")?, Some(host), &bytes)
            .map(|_| ())
    }
    pub fn poll(&self, room: &str, token: &Secret) -> Result<View, Error> {
        let view: View = self.json("GET", &Self::path(room, "/state")?, Some(token), b"")?;
        view.validate().map_err(|_| Error::Invalid)?;
        Ok(view)
    }
    pub fn host_view(&self, room: &str, host: &Secret) -> Result<HostView, Error> {
        let view: HostView = self.json("GET", &Self::path(room, "/members")?, Some(host), b"")?;
        view.validate().map_err(|_| Error::Invalid)?;
        Ok(view)
    }
    pub fn listener_id(&self, room: &str, member: &Secret) -> Result<u32, Error> {
        let value: ListenerId = self.json(
            "GET",
            &Self::path(room, "/members/self")?,
            Some(member),
            b"",
        )?;
        if value.id == 0 || value.id > i32::MAX as u32 {
            return Err(Error::Invalid);
        }
        Ok(value.id)
    }
    pub fn set_admission(&self, room: &str, host: &Secret, open: bool) -> Result<(), Error> {
        let body = serde_json::to_vec(&Admission { open }).map_err(|_| Error::Invalid)?;
        self.request("PUT", &Self::path(room, "/admission")?, Some(host), &body)
            .map(|_| ())
    }
    pub fn rotate_invite(&self, room: &str, host: &Secret) -> Result<Secret, Error> {
        let value: RotatedInvite =
            self.json("POST", &Self::path(room, "/invite")?, Some(host), b"{}")?;
        if value.invite.matches(host) {
            return Err(Error::Invalid);
        }
        Ok(value.invite)
    }
    pub fn remove_member(&self, room: &str, host: &Secret, member: u32) -> Result<Secret, Error> {
        if member == 0 || member > i32::MAX as u32 {
            return Err(Error::Invalid);
        }
        let value: RotatedInvite = self.json(
            "DELETE",
            &Self::path(room, &format!("/members/{member}"))?,
            Some(host),
            b"",
        )?;
        if value.invite.matches(host) {
            return Err(Error::Invalid);
        }
        Ok(value.invite)
    }
    pub fn close(&self, room: &str, host: &Secret) -> Result<(), Error> {
        self.request("DELETE", &Self::path(room, "")?, Some(host), b"")
            .map(|_| ())
    }
    pub fn leave(&self, room: &str, member: &Secret) -> Result<(), Error> {
        self.request(
            "DELETE",
            &Self::path(room, "/members/self")?,
            Some(member),
            b"",
        )
        .map(|_| ())
    }
}
pub fn encode<T: Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, Error> {
    serde_json::to_vec(value)
        .map(Zeroizing::new)
        .map_err(|_| Error::Invalid)
}

#[cfg(all(test, feature = "relay"))]
#[path = "transport_tests.rs"]
mod transport_tests;
