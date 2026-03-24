//! Tests for local intent classification.

use voicerouter::continuous::intent::{Intent, IntentFilter};

#[test]
fn short_text_is_ambient() {
    let filter = IntentFilter::new(&["搜索", "打开"]);
    assert_eq!(filter.classify("嗯"), Intent::Ambient);
}

#[test]
fn single_char_is_ambient() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("啊"), Intent::Ambient);
}

#[test]
fn filler_words_are_ambient() {
    let filter = IntentFilter::new(&["搜索"]);
    assert_eq!(filter.classify("嗯啊呃"), Intent::Ambient);
}

#[test]
fn trigger_prefix_is_command() {
    let filter = IntentFilter::new(&["搜索", "echo "]);
    assert_eq!(filter.classify("搜索Rust VAD"), Intent::Command);
}

#[test]
fn trigger_with_space_prefix() {
    let filter = IntentFilter::new(&["echo "]);
    assert_eq!(filter.classify("echo 你好世界"), Intent::Command);
}

#[test]
fn imperative_verb_is_command() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("帮我打开浏览器"), Intent::Command);
}

#[test]
fn imperative_open() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("打开终端"), Intent::Command);
}

#[test]
fn imperative_search() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("搜索天气预报"), Intent::Command);
}

#[test]
fn imperative_close() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("关闭窗口"), Intent::Command);
}

#[test]
fn declarative_is_uncertain() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("今天天气不错啊"), Intent::Uncertain);
}

#[test]
fn empty_string_is_ambient() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify(""), Intent::Ambient);
}
