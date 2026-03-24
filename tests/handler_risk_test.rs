use voicerouter::config::Config;
use voicerouter::pipeline::handler::RiskLevel;
use voicerouter::pipeline::handlers::build_handler;

#[test]
fn inject_handler_is_low_risk() {
    let config = Config::default();
    let handler = build_handler("inject", &config);
    assert_eq!(handler.risk_level(), RiskLevel::Low);
}

#[test]
fn speak_handler_is_low_risk() {
    let config = Config::default();
    let handler = build_handler("speak", &config);
    assert_eq!(handler.risk_level(), RiskLevel::Low);
}

#[test]
fn transform_handler_is_low_risk() {
    let config = Config::default();
    let handler = build_handler("transform", &config);
    assert_eq!(handler.risk_level(), RiskLevel::Low);
}

#[test]
fn shell_handler_is_high_risk() {
    let config = Config::default();
    let handler = build_handler("shell", &config);
    assert_eq!(handler.risk_level(), RiskLevel::High);
}

#[test]
fn http_handler_is_high_risk() {
    let config = Config::default();
    let handler = build_handler("http", &config);
    assert_eq!(handler.risk_level(), RiskLevel::High);
}

#[test]
fn pipe_handler_is_high_risk() {
    let config = Config::default();
    let handler = build_handler("pipe", &config);
    assert_eq!(handler.risk_level(), RiskLevel::High);
}
