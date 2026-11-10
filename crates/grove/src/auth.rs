//! Two independent PKCE grants. Password entry stays in the external browser.
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge,
    RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

// Public application identity used by spotify-player/ncspot, not a secret.
pub const WEB_CLIENT_ID: &str = "d420a117a32841c2b3474932e49fb54b";
pub const PLAYBACK_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
pub const WEB_SCOPES: &[&str] = &[
    "user-read-private",
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-library-read",
    "user-library-modify",
    "playlist-modify-private",
    "playlist-modify-public",
    "user-top-read",
    "user-read-playback-state",
    "user-modify-playback-state",
];

pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires: Instant,
}

impl Drop for Tokens {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.access.zeroize();
        self.refresh.zeroize();
    }
}

pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize secure networking.".into())
}

type Client = BasicClient<
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

fn client(id: &str, port: u16) -> Result<Client, String> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("A Spotify app client ID must contain 32 hexadecimal characters.".into());
    }
    Ok(BasicClient::new(ClientId::new(id.to_owned()))
        .set_auth_uri(AuthUrl::new("https://accounts.spotify.com/authorize".into()).unwrap())
        .set_token_uri(TokenUrl::new("https://accounts.spotify.com/api/token".into()).unwrap())
        .set_redirect_uri(RedirectUrl::new(format!("http://127.0.0.1:{port}/login")).unwrap()))
}

pub async fn authorize(id: &str, port: u16, scopes: &[&str]) -> Result<Tokens, String> {
    let client = client(id, port)?;
    // Bind before opening the browser. Cancellation drops the listener and verifier.
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| format!("Sign-in port {port} is busy. Close the other app and try again."))?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut request = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(challenge)
        .add_extra_param("show_dialog", "true");
    for scope in scopes {
        request = request.add_scope(Scope::new((*scope).to_owned()));
    }
    let (url, state) = request.url();
    open_browser(url.as_str())?;
    let code = tokio::time::timeout(Duration::from_secs(300), callback(listener, state.secret()))
        .await
        .map_err(|_| "Spotify sign-in expired. Try again.".to_string())??;
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request_async(&http_client()?)
        .await
        .map_err(|_| "Spotify could not complete approval. Try signing in again.".to_string())?;
    Ok(Tokens {
        access: token.access_token().secret().to_owned(),
        refresh: token
            .refresh_token()
            .map(|t| t.secret().to_owned())
            .unwrap_or_default(),
        expires: Instant::now() + token.expires_in().unwrap_or(Duration::from_secs(3600)),
    })
}

pub async fn refresh(id: &str, token: &str) -> Result<Tokens, String> {
    let result = client(id, 8989)?
        .exchange_refresh_token(&RefreshToken::new(token.to_owned()))
        .request_async(&http_client()?)
        .await
        .map_err(|_| {
            "Spotify could not refresh the saved approval. Sign in again or retry when online."
                .to_string()
        })?;
    Ok(Tokens {
        access: result.access_token().secret().to_owned(),
        refresh: result
            .refresh_token()
            .map(|t| t.secret().to_owned())
            .unwrap_or_else(|| token.to_owned()),
        expires: Instant::now() + result.expires_in().unwrap_or(Duration::from_secs(3600)),
    })
}

pub async fn callback(listener: TcpListener, state: &str) -> Result<String, String> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|_| "Sign-in listener closed.".to_string())?;
        let mut bytes = vec![0u8; 8192];
        let read = tokio::time::timeout(Duration::from_secs(3), async {
            let mut length = 0;
            while length < bytes.len() {
                let count = stream.read(&mut bytes[length..]).await?;
                if count == 0 {
                    break;
                }
                length += count;
                if bytes[..length].windows(2).any(|w| w == b"\r\n") {
                    break;
                }
            }
            Ok::<usize, std::io::Error>(length)
        })
        .await;
        let result = match read {
            Ok(Ok(length)) => validate_callback(&String::from_utf8_lossy(&bytes[..length]), state),
            _ => None,
        };
        let (status, body) = if result.is_some() {
            (
                "200 OK",
                "Spotify approval received. You can return to Grove.",
            )
        } else {
            (
                "400 Bad Request",
                "This is not a valid Grove sign-in callback.",
            )
        };
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            stream.write_all(response.as_bytes()),
        )
        .await;
        if let Some(result) = result {
            return result;
        }
    }
}

fn validate_callback(request: &str, expected_state: &str) -> Option<Result<String, String>> {
    let mut first = request.lines().next()?.split_whitespace();
    if first.next()? != "GET" {
        return None;
    }
    let target = first.next()?;
    if !target.starts_with('/') || target.starts_with("//") {
        return None;
    }
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/login" {
        return None;
    }
    let fields: Vec<_> = url.query_pairs().collect();
    let values = |name: &str| {
        fields
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, v)| v.as_ref())
            .collect::<Vec<_>>()
    };
    let states = values("state");
    if states.len() != 1 || states[0] != expected_state {
        return None;
    }
    if !values("error").is_empty() {
        return Some(Err("Spotify approval was declined.".into()));
    }
    let codes = values("code");
    if codes.len() != 1 || codes[0].is_empty() {
        return None;
    }
    Some(Ok(codes[0].to_owned()))
}

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        for base in [
            std::env::var_os("ProgramFiles"),
            std::env::var_os("LOCALAPPDATA"),
        ]
        .into_iter()
        .flatten()
        {
            let path = std::path::PathBuf::from(base).join("Zen Browser/zen.exe");
            if path.is_file() {
                return std::process::Command::new(path)
                    .arg(url)
                    .creation_flags(0x08000000)
                    .spawn()
                    .map(|_| ())
                    .map_err(|_| "Could not open Zen for Spotify approval.".into());
            }
        }
    }
    tide_windows::open_web_page(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_requires_exact_path_state_and_single_code() {
        assert_eq!(
            validate_callback(
                "GET /login?code=one&state=expected HTTP/1.1\r\n",
                "expected"
            )
            .unwrap()
            .unwrap(),
            "one"
        );
        for request in [
            "GET /login?code=one&state=wrong HTTP/1.1",
            "GET /other?code=one&state=expected HTTP/1.1",
            "GET /login?code=one&code=two&state=expected HTTP/1.1",
            "POST /login?code=one&state=expected HTTP/1.1",
            "GET /login?code=one HTTP/1.1",
        ] {
            assert!(validate_callback(request, "expected").is_none());
        }
        assert!(validate_callback(
            "GET /login?error=access_denied&state=expected HTTP/1.1",
            "expected"
        )
        .unwrap()
        .is_err());
    }
    #[tokio::test]
    async fn bad_callback_does_not_consume_approval_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { callback(listener, "random-state").await });
        for (query, status) in [
            ("state=bad&code=bad", "400"),
            ("state=random-state&code=accepted", "200"),
        ] {
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            client
                .write_all(
                    format!("GET /login?{query} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {status}")));
        }
        assert_eq!(task.await.unwrap().unwrap(), "accepted");
    }

    #[tokio::test]
    async fn cancelling_approval_releases_the_callback_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let pending = tokio::spawn(async move { callback(listener, "unsubmitted-state").await });
        tokio::task::yield_now().await;
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        let rebound = TcpListener::bind(address).await.unwrap();
        assert_eq!(rebound.local_addr().unwrap(), address);
    }
}
