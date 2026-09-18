//! Task 3: the heuristic counter.

use orrery_proto::{Message, MessageRole};
use orrery_provider::{HeuristicCounter, TokenCounter};

#[test]
fn is_not_exact() {
    assert!(!HeuristicCounter::new().is_exact());
}

#[test]
fn empty_text_costs_nothing_and_text_costs_something() {
    let c = HeuristicCounter::new();
    assert_eq!(c.count_text(""), 0);
    assert!(c.count_text("abcd") >= 1);
}

proptest::proptest! {
    /// Adding a message never lowers the count. Compaction relies on this:
    /// a counter that could shrink would make the loop non-terminating.
    #[test]
    fn is_monotonic(texts in proptest::collection::vec("[ -~]{0,64}", 0..8)) {
        let c = HeuristicCounter::new();
        let mut messages: Vec<Message> = Vec::new();
        let mut previous = c.count_messages(&messages);
        for t in texts {
            messages.push(Message::text(MessageRole::User, t));
            let now = c.count_messages(&messages);
            proptest::prop_assert!(now >= previous, "{now} < {previous}");
            previous = now;
        }
    }
}
