//! Task 7: (status, body) in, classification out.

use orrery_ext_provider_anthropic::classify;

fn body(kind: &str, message: &str) -> String {
    serde_json::json!({ "type": "error", "error": { "type": kind, "message": message } })
        .to_string()
}

#[test]
fn classification() {
    let long = body(
        "invalid_request_error",
        "prompt is too long: 312000 tokens > 200000 maximum",
    );
    let cases: Vec<(u16, Option<&str>, String, &'static str, bool)> = vec![
        (
            429,
            Some("30"),
            body("rate_limit_error", "slow down"),
            "rate_limited",
            true,
        ),
        (
            429,
            None,
            body("rate_limit_error", "slow down"),
            "rate_limited",
            true,
        ),
        (
            401,
            None,
            body("authentication_error", "invalid x-api-key"),
            "auth",
            false,
        ),
        (
            403,
            None,
            body("permission_error", "not allowed"),
            "auth",
            false,
        ),
        (
            400,
            None,
            body("invalid_request_error", "messages: at least one required"),
            "bad_request",
            false,
        ),
        (400, None, long.clone(), "context_too_long", false),
        (
            413,
            None,
            body("request_too_large", "too big"),
            "context_too_long",
            false,
        ),
        (
            404,
            None,
            body("not_found_error", "model: claude-nope"),
            "no_such_model",
            false,
        ),
        (500, None, body("api_error", "internal"), "server", true),
        (502, None, String::new(), "server", true),
        (
            529,
            None,
            body("overloaded_error", "Overloaded"),
            "server",
            true,
        ),
    ];

    for (status, retry_after, text, code, retryable) in cases {
        let e = classify(status, retry_after, &text);
        assert_eq!(e.code(), code, "status {status}: {e}");
        assert_eq!(e.is_retryable(), retryable, "status {status}: {e}");
    }
}

#[test]
fn retry_after_is_seconds_on_the_wire_and_milliseconds_in_the_type() {
    let e = classify(429, Some("30"), "");
    assert_eq!(e.to_string(), "rate limited, retry after 30000ms");
    // A date-form `Retry-After`, or a malformed one, is not a reason to stop
    // treating a 429 as a 429.
    assert_eq!(
        classify(429, Some("Wed, 21 Oct 2026 07:28:00 GMT"), "").to_string(),
        "rate limited"
    );
}

#[test]
fn context_too_long_carries_the_numbers_when_the_body_has_them() {
    let e = classify(
        400,
        None,
        &body(
            "invalid_request_error",
            "prompt is too long: 312000 tokens > 200000 maximum",
        ),
    );
    assert_eq!(e.to_string(), "context too long: 312000 > 200000");
}

#[test]
fn an_unparseable_body_still_classifies_by_status() {
    assert_eq!(classify(503, None, "<html>gateway</html>").code(), "server");
    assert_eq!(classify(400, None, "not json").code(), "bad_request");
}
