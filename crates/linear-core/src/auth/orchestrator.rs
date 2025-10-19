use std::sync::Arc;

use chrono::Duration;
use tokio::sync::Mutex;
use url::Url;

use super::browser::{run_loopback_flow, run_loopback_flow_auto_port};
use super::manual::run_manual_flow;
use super::{AuthError, AuthSession, CredentialStore, OAuthClient, TokenType};

/// Coordinates authentication flows, persistence, and token refresh.
pub struct AuthManager<S> {
    store: Arc<Mutex<S>>,
    oauth: OAuthClient,
    profile: String,
    refresh_window: Duration,
}

impl<S> AuthManager<S>
where
    S: CredentialStore + Send + Sync + 'static,
{
    pub fn new(store: S, oauth: OAuthClient, profile: impl Into<String>) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            oauth,
            profile: profile.into(),
            refresh_window: Duration::minutes(5),
        }
    }

    pub fn with_refresh_window(mut self, window: Duration) -> Self {
        self.refresh_window = window;
        self
    }

    pub async fn current_session(&self) -> Result<Option<AuthSession>, AuthError> {
        let store = self.store.lock().await;
        store.load(&self.profile)
    }

    pub async fn ensure_fresh_session(&self) -> Result<Option<AuthSession>, AuthError> {
        if let Some(mut session) = self.current_session().await? {
            if session.token_type == TokenType::Bearer
                && session.will_expire_within(self.refresh_window)
            {
                let refreshed = self.oauth.refresh_session(&session).await?.session;
                session = refreshed;
                self.persist(session.clone()).await?;
            }
            return Ok(Some(session));
        }
        Ok(None)
    }

    pub async fn authenticate_browser<F>(
        &self,
        open_browser: bool,
        notify: F,
    ) -> Result<AuthSession, AuthError>
    where
        F: Fn(&Url) -> Result<(), AuthError>,
    {
        let session = run_loopback_flow(&self.oauth, open_browser, notify).await?;
        self.persist(session.clone()).await?;
        Ok(session)
    }

    pub async fn authenticate_browser_auto_port<F, I>(
        &self,
        open_browser: bool,
        notify: F,
        ports: I,
    ) -> Result<AuthSession, AuthError>
    where
        F: Fn(&Url) -> Result<(), AuthError>,
        I: IntoIterator<Item = u16>,
    {
        let session = run_loopback_flow_auto_port(&self.oauth, open_browser, ports, notify).await?;
        self.persist(session.clone()).await?;
        Ok(session)
    }

    pub async fn authenticate_manual<Notify, Input, Fut>(
        &self,
        open_browser: bool,
        notify: Notify,
        read_input: Input,
    ) -> Result<AuthSession, AuthError>
    where
        Notify: Fn(&Url) -> Result<(), AuthError>,
        Input: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<String, AuthError>>,
    {
        let session = run_manual_flow(&self.oauth, open_browser, notify, read_input).await?;
        self.persist(session.clone()).await?;
        Ok(session)
    }

    pub async fn authenticate_api_key(&self, key: String) -> Result<AuthSession, AuthError> {
        let session = AuthSession::new_api_key(key);
        self.persist(session.clone()).await?;
        Ok(session)
    }

    pub async fn authenticate_client_credentials(
        &self,
        scopes: &[String],
    ) -> Result<AuthSession, AuthError> {
        let token = self.oauth.client_credentials(scopes).await?;
        self.persist(token.session.clone()).await?;
        Ok(token.session)
    }

    async fn persist(&self, session: AuthSession) -> Result<(), AuthError> {
        let store = self.store.lock().await;
        store.save(&self.profile, &session)
    }

    pub async fn logout(&self) -> Result<(), AuthError> {
        let store = self.store.lock().await;
        store.delete(&self.profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthSession, OAuthConfig, OAuthEndpoints};
    use chrono::{Duration, Utc};
    use httpmock::prelude::*;
    use std::sync::{Arc as StdArc, Mutex as StdMutex};

    #[derive(Clone, Default)]
    struct MemoryStore {
        inner: StdArc<StdMutex<Option<AuthSession>>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                inner: StdArc::new(StdMutex::new(None)),
            }
        }
    }

    impl CredentialStore for MemoryStore {
        fn load(&self, _profile: &str) -> Result<Option<AuthSession>, AuthError> {
            Ok(self.inner.lock().unwrap().clone())
        }

        fn save(&self, _profile: &str, session: &AuthSession) -> Result<(), AuthError> {
            *self.inner.lock().unwrap() = Some(session.clone());
            Ok(())
        }

        fn delete(&self, _profile: &str) -> Result<(), AuthError> {
            *self.inner.lock().unwrap() = None;
            Ok(())
        }
    }

    fn oauth_client(token_url: Url) -> OAuthClient {
        let config = OAuthConfig::new("client", Url::parse("http://localhost/callback").unwrap());
        let endpoints = OAuthEndpoints {
            authorization_url: Url::parse("http://localhost/auth").unwrap(),
            token_url,
        };
        OAuthClient::with_endpoints(config, endpoints).unwrap()
    }

    #[tokio::test]
    async fn load_and_refresh_session() {
        let server = MockServer::start();
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/token")
                .body_contains("refresh_token");
            then.status(200).json_body_obj(&serde_json::json!({
                "access_token": "new",
                "refresh_token": "refresh",
                "token_type": "bearer",
                "expires_in": 7200,
            }));
        });
        let oauth =
            oauth_client(Url::parse(&format!("{}{}", server.base_url(), "/token")).unwrap());
        let store = MemoryStore::new();
        let manager =
            AuthManager::new(store, oauth, "default").with_refresh_window(Duration::minutes(10));
        let session = AuthSession::new_access_token(
            "old".into(),
            Some("refresh".into()),
            Utc::now() + Duration::seconds(30),
            vec!["read".into()],
        );
        manager.persist(session).await.unwrap();
        let refreshed = manager.ensure_fresh_session().await.unwrap().unwrap();
        refresh_mock.assert();
        assert_eq!(refreshed.access_token, "new");
    }

    #[tokio::test]
    async fn logout_removes_session() {
        let oauth = oauth_client(Url::parse("https://example.com/token").unwrap());
        let store = MemoryStore::new();
        let manager = AuthManager::new(store, oauth, "default");
        manager
            .persist(AuthSession::new_api_key("key".into()))
            .await
            .unwrap();
        manager.logout().await.unwrap();
        assert!(manager.ensure_fresh_session().await.unwrap().is_none());
    }
}
