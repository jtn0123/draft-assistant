//! What a Yahoo read can fail with, and the status tables that decide how
//! each failure is read: which are worth repeating, which mean the grant is
//! gone, and what the user is shown for each.

use crate::yahoo_oauth::AuthError;
use std::time::Duration;

/// A failed read from Yahoo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YahooError {
    /// The request was never made: a key that cannot go in a URL.
    Invalid(String),
    /// The token could not be obtained or renewed.
    Auth(AuthError),
    /// The grant is gone: Yahoo answered 401 to a call made with a token it
    /// had just issued, or refused the refresh token itself. Nothing this
    /// client holds will ever work again, so the caller clears the pair and
    /// the user is told to sign in rather than shown "HTTP 401" and a
    /// Settings panel still saying "Connected".
    SignedOut,
    Http {
        status: u16,
        url: String,
    },
    Transport {
        url: String,
        detail: String,
    },
    Decode {
        url: String,
        detail: String,
    },
}

/// One failed attempt: the error, and how long Yahoo asked to be left alone
/// for. Internal — `Retry-After` is a fact about this attempt rather than
/// about the error, and it would be noise on [`YahooError`], which is what
/// the user is eventually shown.
pub(super) struct Failure {
    pub(super) error: YahooError,
    pub(super) asked_for: Option<Duration>,
}

impl Failure {
    pub(super) fn plain(error: YahooError) -> Self {
        Self {
            error,
            asked_for: None,
        }
    }
}

/// Yahoo answers a throttled caller with its own status 999 rather than the
/// documented 429. Both mean the same thing and both clear on their own.
pub const RATE_LIMITED: [u16; 2] = [429, 999];

/// What the user reads when the grant is gone. It names the one thing that
/// fixes it; a revoked grant used to surface as "HTTP 401 for <url>" while the
/// Settings panel went on saying "Connected".
pub const SIGNED_OUT: &str = "Yahoo signed you out. Connect again in Settings.";

/// The token endpoint's answers that mean the refresh token is dead rather
/// than that Yahoo is having a bad minute: `invalid_grant` and
/// `invalid_client` both arrive as 400, a revoked app as 401. A 5xx or a
/// transport failure is left as [`YahooError::Auth`], because the pair may
/// still be good once Yahoo is back.
pub const GRANT_GONE: [u16; 2] = [400, 401];

impl YahooError {
    /// Whether repeating the identical request could plausibly succeed.
    pub fn retryable(&self) -> bool {
        match self {
            YahooError::Transport { .. } => true,
            YahooError::Http { status, .. } => {
                (500..600).contains(status) || RATE_LIMITED.contains(status)
            }
            YahooError::Invalid(_)
            | YahooError::Auth(_)
            | YahooError::SignedOut
            | YahooError::Decode { .. } => false,
        }
    }
}

impl std::fmt::Display for YahooError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YahooError::Invalid(message) => f.write_str(message),
            YahooError::Auth(error) => write!(f, "{error}"),
            YahooError::SignedOut => f.write_str(SIGNED_OUT),
            // "HTTP 999" is Yahoo's, and means nothing to anybody; the one
            // thing the user can do about it is wait, so say that instead.
            YahooError::Http { status, .. } if RATE_LIMITED.contains(status) => {
                f.write_str("Yahoo is rate-limiting requests — try again in a minute")
            }
            YahooError::Http { status, url } => write!(f, "HTTP {status} for {url}"),
            YahooError::Transport { url, detail } => write!(f, "request failed: {url}: {detail}"),
            YahooError::Decode { url, detail } => write!(f, "bad JSON from {url}: {detail}"),
        }
    }
}

impl std::error::Error for YahooError {}
