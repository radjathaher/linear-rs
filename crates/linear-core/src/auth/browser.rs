use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use url::Url;

use super::{utils, AuthError, AuthSession, OAuthClient, PkcePair};

const SUCCESS_HTML: &str = r#"<html><body><h1>Authentication complete</h1><p>You may close this window and return to the terminal.</p></body></html>"#;
const ERROR_HTML: &str = r#"<html><body><h1>Authentication failed</h1><p>Please return to the terminal for details.</p></body></html>"#;

/// Run the browser-based OAuth flow using a loopback HTTP listener.
pub async fn run_loopback_flow<F>(
    client: &OAuthClient,
    open_browser: bool,
    notify_authorization_url: F,
) -> Result<AuthSession, AuthError>
where
    F: Fn(&Url) -> Result<(), AuthError>,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = Url::parse(&format!("http://127.0.0.1:{port}/callback"))?;
    let client = client.clone_with_redirect(redirect_uri.clone());
    let pkce = PkcePair::generate();
    let state = utils::random_state(32);
    let auth_url = client.authorization_url(&pkce, &state)?;

    notify_authorization_url(&auth_url)?;

    if open_browser {
        open::that(auth_url.as_str()).map_err(|err| AuthError::BrowserLaunch(err.to_string()))?;
    }

    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let result = accept_authorization(listener, state).await;
        let _ = tx.send(result);
    });

    let code = rx.await.map_err(|_| AuthError::ListenerClosed)??;

    let token = client.exchange_code(&code, &pkce).await?;
    Ok(token.session)
}

async fn accept_authorization(
    listener: TcpListener,
    expected_state: String,
) -> Result<String, AuthError> {
    let (mut stream, _addr) = listener.accept().await?;
    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let path = parse_request_path(&request)?;
    let url = Url::parse(&format!("http://localhost{path}"))?;

    let mut code: Option<String> = None;
    let mut state: Option<String> = None;
    let mut error: Option<String> = None;

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }

    if let Some(err) = error {
        respond(&mut stream, 400, ERROR_HTML).await?;
        return Err(AuthError::AccessDenied(err));
    }

    let code = code.ok_or(AuthError::MissingAuthorizationCode)?;
    if state.as_deref() != Some(expected_state.as_str()) {
        respond(&mut stream, 400, ERROR_HTML).await?;
        return Err(AuthError::StateMismatch);
    }

    respond(&mut stream, 200, SUCCESS_HTML).await?;
    let _ = stream.shutdown().await;
    Ok(code)
}

fn parse_request_path(request: &str) -> Result<&str, AuthError> {
    let mut lines = request.lines();
    let first_line = lines
        .next()
        .ok_or_else(|| AuthError::InvalidAuthorizationResponse("missing request line".into()))?;
    let mut parts = first_line.split_whitespace();
    let _method = parts
        .next()
        .ok_or_else(|| AuthError::InvalidAuthorizationResponse("missing method".into()))?;
    let path = parts
        .next()
        .ok_or_else(|| AuthError::InvalidAuthorizationResponse("missing path".into()))?;
    Ok(path)
}

async fn respond(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), AuthError> {
    let status_line = match status {
        200 => "HTTP/1.1 200 OK",
        400 => "HTTP/1.1 400 Bad Request",
        _ => "HTTP/1.1 500 Internal Server Error",
    };
    let response = format!(
        "{status_line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use tokio::net::TcpStream;

    use crate::auth::{OAuthClient, OAuthConfig, OAuthEndpoints};

    fn test_client(token_url: Url) -> OAuthClient {
        let config = OAuthConfig::new("client", Url::parse("http://localhost/callback").unwrap());
        let endpoints = OAuthEndpoints {
            authorization_url: Url::parse("http://localhost/authorize").unwrap(),
            token_url,
        };
        OAuthClient::with_endpoints(config, endpoints).unwrap()
    }

    #[tokio::test]
    async fn loopback_flow_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/token");
            then.status(200).json_body_obj(&serde_json::json!({
                "access_token": "abc",
                "refresh_token": "ref",
                "token_type": "bearer",
                "expires_in": 3600,
                "scope": "read"
            }));
        });

        let client =
            test_client(Url::parse(&format!("{}{}", server.base_url(), "/token")).unwrap());

        let session = run_loopback_flow(&client, false, |url| {
            let host = url
                .host_str()
                .expect("url has host")
                .to_owned();
            let port = url.port().expect("url has port");
            let state = url
                .query_pairs()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| v.into_owned())
                .expect("state present");
            tokio::spawn(async move {
                let mut stream = TcpStream::connect((host.clone(), port)).await.unwrap();
                let request = format!(
                    "GET /callback?code=test-code&state={state} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n",
                    host = host,
                    port = port
                );
                stream.write_all(request.as_bytes()).await.unwrap();
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf).await;
            });
            Ok(())
        })
        .await
        .expect("loopback flow succeeded");

        mock.assert();
        assert_eq!(session.access_token, "abc");
        assert_eq!(session.refresh_token.as_deref(), Some("ref"));
    }

    #[tokio::test]
    async fn loopback_flow_state_mismatch() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/token");
            then.status(200).json_body_obj(&serde_json::json!({
                "access_token": "abc",
                "refresh_token": "ref",
                "token_type": "bearer",
                "expires_in": 3600,
            }));
        });

        let client =
            test_client(Url::parse(&format!("{}{}", server.base_url(), "/token")).unwrap());

        let err = run_loopback_flow(&client, false, |url| {
            let host = url.host_str().unwrap().to_owned();
            let port = url.port().unwrap();
            tokio::spawn(async move {
                let mut stream = TcpStream::connect((host.clone(), port)).await.unwrap();
                let request = format!(
                    "GET /callback?code=test-code&state=wrong HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n",
                    host = host,
                    port = port
                );
                stream.write_all(request.as_bytes()).await.unwrap();
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf).await;
            });
            Ok(())
        })
        .await
        .unwrap_err();

        assert!(matches!(err, AuthError::StateMismatch));
    }
}
