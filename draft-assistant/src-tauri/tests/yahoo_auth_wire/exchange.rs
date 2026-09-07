use super::*;

#[tokio::test]
async fn a_code_is_exchanged_for_a_token_pair() {
    let stub = serve(|_: &Request| Reply::ok(FRESH_TOKEN));
    let client = OauthClient::with_base(stub.base());
    let tokens = client
        .exchange_code(&credentials(), "  auth-code-1  ", OOB)
        .await
        .expect("the code is good");
    assert_eq!(tokens.access_token, "access-2");
    assert_eq!(tokens.refresh_token, "refresh-2");
    assert!(!tokens.is_expired(draft_assistant_lib::yahoo_oauth::now_secs()));

    let request = stub.requests().pop().expect("one request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path(), "/oauth2/get_token");
    assert_eq!(request.header("authorization"), Some(BASIC));
    assert_eq!(
        request.form("grant_type").as_deref(),
        Some("authorization_code")
    );
    // Trimmed, and sent in the body rather than the URL.
    assert_eq!(request.form("code").as_deref(), Some("auth-code-1"));
    assert_eq!(request.form("redirect_uri").as_deref(), Some("oob"));
    assert!(
        request.target.split('?').nth(1).is_none(),
        "{}",
        request.target
    );
}

#[tokio::test]
async fn a_loopback_exchange_repeats_the_redirect_uri_yahoo_registered() {
    let stub = serve(|_: &Request| Reply::ok(FRESH_TOKEN));
    let client = OauthClient::with_base(stub.base());
    client
        .exchange_code(&credentials(), "code", "http://localhost:8731/")
        .await
        .expect("the code is good");
    assert_eq!(
        stub.requests()[0].form("redirect_uri").as_deref(),
        Some("http://localhost:8731/")
    );
}

#[tokio::test]
async fn a_rejected_code_reports_yahoos_status_without_the_secret() {
    let stub = serve(|_: &Request| {
        Reply::status(
            400,
            format!(r#"{{"error":"invalid_grant","description":"{SECRET} and a bad code"}}"#),
        )
    });
    let client = OauthClient::with_base(stub.base());
    let error = client
        .exchange_code(&credentials(), "stale-code", OOB)
        .await
        .expect_err("that code was used already");
    assert!(
        matches!(error, AuthError::Http { status: 400, .. }),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("invalid_grant"), "{message}");
    assert!(!message.contains(SECRET), "the secret escaped: {message}");
}

#[tokio::test]
async fn a_token_reply_that_is_not_a_token_is_a_decode_failure() {
    let stub = serve(|_: &Request| Reply::ok("<html>maintenance</html>"));
    let client = OauthClient::with_base(stub.base());
    let error = client
        .exchange_code(&credentials(), "code", OOB)
        .await
        .expect_err("that was not a token");
    assert!(matches!(error, AuthError::Decode(_)), "{error:?}");
}

#[tokio::test]
async fn a_login_host_that_is_not_there_is_a_transport_failure() {
    let client = OauthClient::with_base("http://127.0.0.1:1");
    let error = client
        .exchange_code(&credentials(), "code", OOB)
        .await
        .expect_err("port 1 answers nobody");
    assert!(matches!(error, AuthError::Transport(_)), "{error:?}");
    assert!(!error.to_string().contains(SECRET));
}
