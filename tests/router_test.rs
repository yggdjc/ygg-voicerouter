//! Integration tests for the voice router.

use std::sync::{Arc, Mutex};

use voicerouter::config::{Config, InjectMethod, Rule as ConfigRule, RouterConfig};
use voicerouter::router::{handler::Handler, Router};

// ---------------------------------------------------------------------------
// Mock handler
// ---------------------------------------------------------------------------

/// Records every payload it receives so tests can assert on it.
struct MockHandler {
    name: &'static str,
    received: Arc<Mutex<Vec<String>>>,
}

impl MockHandler {
    fn new(name: &'static str, received: Arc<Mutex<Vec<String>>>) -> Self {
        Self { name, received }
    }
}

impl Handler for MockHandler {
    fn name(&self) -> &str {
        self.name
    }

    fn handle(&self, text: &str) -> anyhow::Result<()> {
        self.received.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Router helpers
// ---------------------------------------------------------------------------

// Router::new() is the only public constructor and its default handler is
// always InjectHandler. For mock-based dispatch tests we use TestRouter below,
// which mirrors the same prefix-matching logic without depending on internals.

/// A minimal router used in mock tests that mirrors Router dispatch logic.
struct TestRouter {
    rules: Vec<(String, Box<dyn Handler>)>,
    default: Box<dyn Handler>,
}

impl TestRouter {
    fn new(default: Box<dyn Handler>) -> Self {
        Self { rules: Vec::new(), default }
    }

    fn add_rule(&mut self, trigger: impl Into<String>, handler: Box<dyn Handler>) {
        self.rules.push((trigger.into(), handler));
    }

    fn dispatch(&self, text: &str) -> anyhow::Result<()> {
        for (trigger, handler) in &self.rules {
            if text.starts_with(trigger.as_str()) {
                let payload = text[trigger.len()..].trim_start();
                return handler.handle(payload);
            }
        }
        self.default.handle(text)
    }
}

// ---------------------------------------------------------------------------
// Tests using TestRouter (mock-based)
// ---------------------------------------------------------------------------

#[test]
fn test_default_handler_called_when_no_rules() {
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let default = Box::new(MockHandler::new("default", Arc::clone(&received)));

    let router = TestRouter::new(default);
    router.dispatch("hello world").unwrap();

    let got = received.lock().unwrap();
    assert_eq!(*got, vec!["hello world"]);
}

#[test]
fn test_prefix_routing_dispatches_to_correct_handler() {
    let shell_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let default_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let shell_mock = Box::new(MockHandler::new("shell", Arc::clone(&shell_received)));
    let default_mock = Box::new(MockHandler::new("default", Arc::clone(&default_received)));

    let mut router = TestRouter::new(default_mock);
    router.add_rule("run ", shell_mock);

    router.dispatch("run ls -la").unwrap();

    let shell_got = shell_received.lock().unwrap();
    let default_got = default_received.lock().unwrap();

    assert_eq!(*shell_got, vec!["ls -la"], "shell handler should have received stripped payload");
    assert!(default_got.is_empty(), "default handler should not have been called");
}

#[test]
fn test_first_match_wins() {
    let first_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let second_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let default_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let first_mock = Box::new(MockHandler::new("first", Arc::clone(&first_received)));
    let second_mock = Box::new(MockHandler::new("second", Arc::clone(&second_received)));
    let default_mock = Box::new(MockHandler::new("default", Arc::clone(&default_received)));

    let mut router = TestRouter::new(default_mock);
    // Both "run " and "run ls" would match "run ls -la"; "run " is first.
    router.add_rule("run ", first_mock);
    router.add_rule("run ls", second_mock);

    router.dispatch("run ls -la").unwrap();

    let first_got = first_received.lock().unwrap();
    assert_eq!(*first_got, vec!["ls -la"]);
    assert!(second_received.lock().unwrap().is_empty());
    assert!(default_received.lock().unwrap().is_empty());
}

#[test]
fn test_trigger_stripped_from_payload() {
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mock = Box::new(MockHandler::new("mock", Arc::clone(&received)));
    let default_mock =
        Box::new(MockHandler::new("default", Arc::new(Mutex::new(Vec::new()))));

    let mut router = TestRouter::new(default_mock);
    router.add_rule("ask ", mock);

    router.dispatch("ask what is the weather").unwrap();

    let got = received.lock().unwrap();
    assert_eq!(*got, vec!["what is the weather"]);
}

#[test]
fn test_non_matching_text_goes_to_default() {
    let shell_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let default_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let shell_mock = Box::new(MockHandler::new("shell", Arc::clone(&shell_received)));
    let default_mock = Box::new(MockHandler::new("default", Arc::clone(&default_received)));

    let mut router = TestRouter::new(default_mock);
    router.add_rule("run ", shell_mock);

    router.dispatch("hello world").unwrap();

    assert!(shell_received.lock().unwrap().is_empty());
    let got = default_received.lock().unwrap();
    assert_eq!(*got, vec!["hello world"]);
}

// ---------------------------------------------------------------------------
// Tests using real Router::new()
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires a real Wayland/X11 session with wl-copy, ydotool, or xdotool"]
fn real_router_no_rules_dispatches_to_inject_stub() {
    let config = Config::default();
    let router = Router::new(&config);
    router.dispatch("hello world").unwrap();
}

#[test]
#[ignore = "requires a real Wayland/X11 session with wl-copy, ydotool, or xdotool"]
fn real_router_inject_rule_dispatches() {
    let config = Config {
        router: RouterConfig {
            rules: vec![ConfigRule {
                trigger: "note ".to_owned(),
                handler: "inject".to_owned(),
            }],
        },
        ..Config::default()
    };
    let router = Router::new(&config);
    router.dispatch("note remember to buy milk").unwrap();
}

#[test]
fn real_router_shell_rule_executes_echo() {
    let config = Config {
        router: RouterConfig {
            rules: vec![ConfigRule {
                trigger: "run ".to_owned(),
                handler: "shell".to_owned(),
            }],
        },
        ..Config::default()
    };
    let router = Router::new(&config);
    // "run " is stripped, shell receives "echo hi"
    router.dispatch("run echo hi").unwrap();
}

#[test]
#[ignore = "requires a real Wayland/X11 session with wl-copy, ydotool, or xdotool"]
fn real_router_unknown_handler_falls_back_to_inject() {
    let config = Config {
        router: RouterConfig {
            rules: vec![ConfigRule {
                trigger: "x ".to_owned(),
                handler: "nonexistent".to_owned(),
            }],
        },
        ..Config::default()
    };
    let router = Router::new(&config);
    router.dispatch("x some text").unwrap();
}

#[test]
#[ignore = "requires a real Wayland/X11 session with wtype"]
fn real_router_inject_method_propagated() {
    let config = Config {
        inject: voicerouter::config::InjectConfig { method: InjectMethod::Wtype },
        ..Config::default()
    };
    let router = Router::new(&config);
    router.dispatch("some text").unwrap();
}
