//! Integration test target for the `Validator` trait foundation.
//
//! Three test cases:
//!   1. **`noop_validator_returns_empty_issue_list`** — NoopValidator
//!      always returns Ok(empty Vec), with zero debounce hint.
//!   2. **`canned_validator_returns_cloned_issues`** — A
//!      CannedValidator with preset issues returns a clone of the
//!      issues for any input, regardless of text content; the future
//!      is freshly owned (not borrowed), matching the production
//!      NiriValidator contract.
//!   3. **`default_validator_debounce_hint_is_250ms`** — A custom
//!      Validator impl that does NOT override `debounce_hint` returns
//!      250ms (the spec default), proving the default-method wires
//!      when override is absent.
//!
//! Plus a fourth smoke test (`box_future_is_send_bound`) that
//! confirms the manual `BoxFuture` type alias preserves the
//! `Send`-bound requirement via a single-line type-assertion.

use std::time::Duration;

use async_std::task::block_on;

use dotcfg_gui::{
    BoxFuture, CannedValidator, Error, NoopValidator, Severity, ValidationIssue, Validator,
};

#[test]
fn noop_validator_returns_empty_issue_list() {
    let v = NoopValidator;
    let result = block_on(async { v.validate_kdl("any kdl text").await });
    assert!(
        result.is_ok(),
        "NoopValidator::validate_kdl must return Ok; got Err {result:?}"
    );
    assert!(
        result.unwrap().is_empty(),
        "NoopValidator must always return an empty issue list (no issues of any severity)"
    );
    assert_eq!(v.name(), "noop", "NoopValidator::name must return \"noop\"");
    assert_eq!(
        v.debounce_hint(),
        Duration::from_millis(0),
        "NoopValidator::debounce_hint is 0ms (tests should never have to wait)"
    );
}

#[test]
fn canned_validator_returns_cloned_issues() {
    // This test asserts the input-text-INDEPENDENT contract: any input
    // triggers the same preset issues, and the returned Vec is
    // freshly-owned not borrowed (so a second concurrent call doesn't
    // race). Both invariants are load-bearing — a future maintainer
    // adding input-aware filtering or returning a borrow would break
    // either or both.
    let preset = vec![
        ValidationIssue {
            line: 5,
            severity: Severity::Error,
            message: "missing closing brace on `binds` block".to_string(),
        },
        ValidationIssue {
            line: 12,
            severity: Severity::Warning,
            message: "deprecated key `tap_to_click`; use `tap` instead".to_string(),
        },
    ];
    let v = CannedValidator {
        name: "canned-test",
        issues: preset.clone(),
    };

    // Two calls with different input texts must each return the same
    // cloned issues — confirms the Vec is freshly-owned per call (not
    // a borrow that races with subsequent validations).
    let result_a = block_on(async { v.validate_kdl("input-A.kdl").await });
    let result_b = block_on(async { v.validate_kdl("completely different input").await });

    assert_eq!(
        result_a.expect("first call returns Ok"),
        preset,
        "first call returns the preset issue list (input-independent)"
    );
    assert_eq!(
        result_b.expect("second call returns Ok"),
        preset,
        "second call returns the same preset issue list (not empty, not input-dependent)"
    );
    assert_eq!(
        v.name(),
        "canned-test",
        "CannedValidator::name returns the stored display name"
    );
}

#[test]
fn default_validator_debounce_hint_is_250ms() {
    // A minimal custom Validator impl that does NOT override
    // `debounce_hint`. Proves the trait default-method wires correctly
    // when the override is absent.
    struct CustomValidator;
    impl Validator for CustomValidator {
        fn name(&self) -> &'static str {
            "custom"
        }
        fn validate_kdl<'a>(
            &'a self,
            _text: &'a str,
        ) -> BoxFuture<'a, Result<Vec<ValidationIssue>, Error>> {
            Box::pin(async move { Ok(vec![]) })
        }
        // NOTE: deliberately NOT overriding `debounce_hint` -- this
        // is the load-bearing assertion for this test.
    }

    assert_eq!(
        CustomValidator.debounce_hint(),
        Duration::from_millis(250),
        "default-method debounce_hint is 250ms per spec Wave 2 Step 9"
    );
}

#[test]
fn box_future_is_send_bound() {
    // Compile-only assertion that BoxFuture preserves the Send bound
    // required by async-std's task::spawn. A simple compile test is
    // enough; no runtime check.
    fn assert_send<T: Send>(_: T) {}
    // `Box::pin(async { Ok::<(), ()>(()) })` yields a future of type
    // `Pin<Box<dyn Future<Output=Result<(), ()>> + Send + 'static>>`
    // which we annotate as `BoxFuture<'static, Result<(), ()>>` to
    // document the expected type alias shape.
    let fut: BoxFuture<'static, Result<(), ()>> = Box::pin(async { Ok::<(), ()>(()) });
    assert_send(fut);
}
