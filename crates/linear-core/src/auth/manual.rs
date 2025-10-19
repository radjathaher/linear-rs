use std::future::Future;

use url::Url;

use super::{utils, AuthError, AuthSession, OAuthClient, PkcePair};

/// Execute the manual copy/paste OAuth flow.
pub async fn run_manual_flow<Notify, Input, Fut>(
    client: &OAuthClient,
    open_browser: bool,
    notify_authorization_url: Notify,
    mut read_input: Input,
) -> Result<AuthSession, AuthError>
where
    Notify: Fn(&Url) -> Result<(), AuthError>,
    Input: FnMut() -> Fut,
    Fut: Future<Output = Result<String, AuthError>>,
{
    let pkce = PkcePair::generate();
    let state = utils::random_state(32);
    let auth_url = client.authorization_url(&pkce, &state)?;

    notify_authorization_url(&auth_url)?;

    if open_browser {
        open::that(auth_url.as_str()).map_err(|err| AuthError::BrowserLaunch(err.to_string()))?;
    }

    let raw = read_input().await?;
    let response = parse_manual_input(raw.trim())?;

    if let Some(returned_state) = response.state {
        if returned_state != state {
            return Err(AuthError::StateMismatch);
        }
    }

    let token = client.exchange_code(&response.code, &pkce).await?;
    Ok(token.session)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthorizationResponse {
    code: String,
    state: Option<String>,
}

fn parse_manual_input(input: &str) -> Result<AuthorizationResponse, AuthError> {
    if input.is_empty() {
        return Err(AuthError::InvalidAuthorizationResponse(
            "empty input".into(),
        ));
    }

    if let Ok(url) = Url::parse(input) {
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
            return Err(AuthError::AccessDenied(err));
        }
        let code = code.ok_or(AuthError::MissingAuthorizationCode)?;
        return Ok(AuthorizationResponse { code, state });
    }

    Ok(AuthorizationResponse {
        code: input.to_owned(),
        state: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use std::sync::{Arc, Mutex};

    use crate::auth::{OAuthClient, OAuthConfig, OAuthEndpoints};

    fn test_client(token_url: Url) -> OAuthClient {
        let config = OAuthConfig::new(
            "client",
            Url::parse("https://example.com/callback").unwrap(),
        );
        let endpoints = OAuthEndpoints {
            authorization_url: Url::parse("https://linear.app/oauth/authorize").unwrap(),
            token_url,
        };
        OAuthClient::with_endpoints(config, endpoints).unwrap()
    }

    #[tokio::test]
    async fn manual_flow_with_full_redirect() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
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
        let state_holder = Arc::new(Mutex::new(String::new()));
        let read_counter = Arc::new(Mutex::new(0usize));

        let notify = {
            let state_holder = state_holder.clone();
            move |url: &Url| {
                let state = url
                    .query_pairs()
                    .find(|(k, _)| k == "state")
                    .map(|(_, v)| v.into_owned())
                    .expect("state present");
                *state_holder.lock().unwrap() = state;
                Ok(())
            }
        };

        let read_input = {
            let state_holder = state_holder.clone();
            let read_counter = read_counter.clone();
            move || {
                let state_holder = state_holder.clone();
                let read_counter = read_counter.clone();
                async move {
                    *read_counter.lock().unwrap() += 1;
                    let state = state_holder.lock().unwrap().clone();
                    Ok(format!(
                        "https://example.com/callback?code=manual-code&state={state}"
                    ))
                }
            }
        };

        let session = run_manual_flow(&client, false, notify, read_input)
            .await
            .expect("manual flow succeeded");

        mock.assert();
        assert_eq!(session.access_token, "abc");
        assert_eq!(session.refresh_token.as_deref(), Some("ref"));
        assert_eq!(*read_counter.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn manual_flow_with_code_only() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/token");
            then.status(200).json_body_obj(&serde_json::json!({
                "access_token": "xyz",
                "token_type": "bearer",
                "expires_in": 3600,
            }));
        });

        let client =
            test_client(Url::parse(&format!("{}{}", server.base_url(), "/token")).unwrap());
        let session = run_manual_flow(
            &client,
            false,
            |_| Ok(()),
            || async { Ok("raw-code".to_string()) },
        )
        .await
        .expect("manual flow succeeded");

        assert_eq!(session.access_token, "xyz");
    }

    #[tokio::test]
    async fn manual_flow_state_mismatch() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/token");
            then.status(200).json_body_obj(&serde_json::json!({
                "access_token": "abc",
                "token_type": "bearer",
                "expires_in": 3600,
            }));
        });

        let client =
            test_client(Url::parse(&format!("{}{}", server.base_url(), "/token")).unwrap());
        let notify = |_url: &Url| Ok(());
        let read_input =
            || async { Ok("https://example.com/callback?code=manual&state=bad".to_string()) };

        let err = run_manual_flow(&client, false, notify, read_input)
            .await
            .unwrap_err();

        assert!(matches!(err, AuthError::StateMismatch));
    }

    #[test]
    fn parse_input_handles_raw_code() {
        let output = parse_manual_input("code123").unwrap();
        assert_eq!(output.code, "code123");
        assert!(output.state.is_none());
    }

    #[test]
    fn parse_input_handles_url() {
        let response =
            parse_manual_input("https://example.com/callback?code=abc&state=xyz").unwrap();
        assert_eq!(response.code, "abc");
        assert_eq!(response.state.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_input_access_denied() {
        let err =
            parse_manual_input("https://example.com/callback?error=access_denied").unwrap_err();
        assert!(matches!(err, AuthError::AccessDenied(_)));
    }
}
