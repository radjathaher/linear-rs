use std::env;

/// Authentication flows supported by linear-rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFlow {
    Browser,
    Manual,
    ApiKey,
    ClientCredentials,
}

/// Helper responsible for inferring which flow to start with.
#[derive(Debug)]
pub struct FlowPreference {
    preferred: AuthFlow,
    browser_available: bool,
}

impl FlowPreference {
    /// Detect the preferred flow based on environment variables and terminal capabilities.
    pub fn detect() -> Self {
        if let Some(flow) = env::var("LINEAR_RS_AUTH_FLOW")
            .ok()
            .and_then(|value| value.parse::<AuthFlow>().ok())
        {
            return Self {
                preferred: flow,
                browser_available: matches!(flow, AuthFlow::Browser),
            };
        }

        let browser_available = browser_available();
        let preferred = if browser_available {
            AuthFlow::Browser
        } else {
            AuthFlow::Manual
        };
        Self {
            preferred,
            browser_available,
        }
    }

    /// Preferred flow to offer to the user.
    pub fn preferred(&self) -> AuthFlow {
        self.preferred
    }

    /// Whether we should attempt to spawn the system browser automatically.
    pub fn browser_available(&self) -> bool {
        self.browser_available
    }
}

fn browser_available() -> bool {
    if env::var_os("LINEAR_RS_NO_BROWSER").is_some() {
        return false;
    }

    if env::var_os("SSH_CONNECTION").is_some() && env::var_os("DISPLAY").is_none() {
        return false;
    }

    if env::var_os("DISPLAY").is_some() || env::var_os("WAYLAND_DISPLAY").is_some() {
        return true;
    }

    cfg!(target_os = "windows") || cfg!(target_os = "macos")
}

impl std::str::FromStr for AuthFlow {
    type Err = InvalidFlow;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "browser" => Ok(AuthFlow::Browser),
            "manual" | "code" => Ok(AuthFlow::Manual),
            "api-key" | "apikey" | "key" => Ok(AuthFlow::ApiKey),
            "client" | "client-credentials" | "cc" => Ok(AuthFlow::ClientCredentials),
            other => Err(InvalidFlow(other.to_owned())),
        }
    }
}

impl std::fmt::Display for AuthFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            AuthFlow::Browser => "browser",
            AuthFlow::Manual => "manual",
            AuthFlow::ApiKey => "api-key",
            AuthFlow::ClientCredentials => "client-credentials",
        };
        write!(f, "{value}")
    }
}

/// Error reported when parsing an unsupported flow.
#[derive(Debug, thiserror::Error)]
#[error("invalid auth flow '{0}'")]
pub struct InvalidFlow(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flow_variants() {
        assert_eq!("browser".parse::<AuthFlow>().unwrap(), AuthFlow::Browser);
        assert_eq!("manual".parse::<AuthFlow>().unwrap(), AuthFlow::Manual);
        assert_eq!("api-key".parse::<AuthFlow>().unwrap(), AuthFlow::ApiKey);
        assert_eq!(
            "client-credentials".parse::<AuthFlow>().unwrap(),
            AuthFlow::ClientCredentials
        );
    }

    #[test]
    fn invalid_flow() {
        let err = "unknown".parse::<AuthFlow>().unwrap_err();
        assert_eq!(err.0, "unknown");
    }
}
