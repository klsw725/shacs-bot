use serde_json::json;
use shacs_config::{BrowserConfig, BrowserViewConfig, Config};
use std::error::Error;

#[test]
fn reply_wait_default_and_range() -> Result<(), Box<dyn Error>> {
    let default: Config = serde_json::from_value(json!({}))?;
    assert_eq!(default.tools.browser.reply_wait_minutes, 30);
    assert_eq!(
        serde_json::to_value(&default.tools.browser)?["replyWaitMinutes"],
        30
    );
    for minutes in [5, 120] {
        let config: BrowserConfig = serde_json::from_value(json!({"replyWaitMinutes":minutes}))?;
        assert_eq!(config.reply_wait_minutes, minutes);
    }
    for minutes in [4, 121] {
        let error = serde_json::from_value::<BrowserConfig>(json!({"replyWaitMinutes":minutes}))
            .err()
            .ok_or("out-of-range reply wait must fail")?;
        assert!(error
            .to_string()
            .contains("tools.browser timeout outside allowed range"));
    }
    Ok(())
}

#[test]
fn persist_login_default_off() -> Result<(), Box<dyn Error>> {
    let default: Config = serde_json::from_value(json!({}))?;
    assert!(
        !default.tools.browser.persist_login,
        "login is not kept unless the user turns it on"
    );
    assert_eq!(
        serde_json::to_value(&default.tools.browser)?["persistLogin"],
        false
    );
    let on: BrowserConfig = serde_json::from_value(json!({"persistLogin":true}))?;
    assert!(on.persist_login, "persistLogin: true is read");
    assert_eq!(serde_json::to_value(&on)?["persistLogin"], true);
    // The key is independent of `enable`.
    let enabled: BrowserConfig = serde_json::from_value(json!({"enable":true}))?;
    assert!(!enabled.persist_login, "enable does not turn it on");
    assert!(
        serde_json::from_value::<BrowserConfig>(json!({"persistLogin":"yes"})).is_err(),
        "a non-boolean value is a config error"
    );
    // A value the user already set survives a load and save, which is what
    // onboard relies on when it only fills in missing keys.
    let kept: Config = serde_json::from_value(json!({"tools":{"browser":{"persistLogin":true}}}))?;
    assert_eq!(
        serde_json::to_value(&kept)?["tools"]["browser"]["persistLogin"],
        true
    );
    Ok(())
}

#[test]
fn takeover_minutes_and_view_url() -> Result<(), Box<dyn Error>> {
    let default: Config = serde_json::from_value(json!({}))?;
    assert_eq!(default.tools.browser.takeover_minutes, 15);
    assert_eq!(
        serde_json::to_value(&default.tools.browser)?["takeoverMinutes"],
        15
    );
    for minutes in [5, 60] {
        let config: BrowserConfig = serde_json::from_value(json!({"takeoverMinutes":minutes}))?;
        assert_eq!(config.takeover_minutes, minutes);
    }
    for minutes in [4, 61] {
        let error = serde_json::from_value::<BrowserConfig>(json!({"takeoverMinutes":minutes}))
            .err()
            .ok_or("out-of-range takeover minutes must fail")?;
        assert!(error
            .to_string()
            .contains("tools.browser timeout outside allowed range"));
    }

    // The default has no `url` key and shows the loopback address.
    let view = BrowserViewConfig::default();
    assert_eq!(view.url, "");
    assert_eq!(view.base_url(), None);
    assert!(!view.url_ignored());
    assert!(serde_json::to_value(&view)?.get("url").is_none());

    for (url, base) in [
        ("http://100.64.0.7:6080", "http://100.64.0.7:6080"),
        ("http://100.64.0.7:6080/", "http://100.64.0.7:6080"),
        ("https://view.example.test", "https://view.example.test"),
        ("https://view.example.test/", "https://view.example.test"),
        ("http://[fd7a::1]:6080/", "http://[fd7a::1]:6080"),
    ] {
        let view: BrowserViewConfig = serde_json::from_value(json!({"url":url}))?;
        assert_eq!(view.base_url(), Some(base), "{url}");
        assert!(!view.url_ignored(), "{url}");
        assert_eq!(serde_json::to_value(&view)?["url"], url);
    }
    let long = format!("http://{}.test", "a".repeat(200));
    for url in [
        "view.example.test",
        "ftp://view.example.test",
        "http://",
        "http://user@view.example.test",
        "http://user:secret@view.example.test:6080",
        "http://view.example.test/path",
        "http://view.example.test/?query=1",
        "http://view.example.test?query=1",
        "http://view.example.test/#part",
        "http://view.example.test:0",
        "http://view.example.test:port",
        "http://view.example.test:65536",
        "http://보기.example.test",
        "http://view.example.test//",
        long.as_str(),
    ] {
        // An unusable address is ignored, never a config error, so it cannot
        // block the browser from starting.
        let config: BrowserConfig = serde_json::from_value(json!({"view":{"url":url}}))?;
        assert_eq!(config.view.base_url(), None, "{url}");
        assert!(config.view.url_ignored(), "{url}");
    }

    // Values the user already set survive a load and save, which is what
    // onboard relies on when it only fills in missing keys.
    let kept: Config = serde_json::from_value(json!({"tools":{"browser":{
        "takeoverMinutes":20,
        "view":{"enable":true,"password":"abcd1234","url":"http://100.64.0.7:6080"}
    }}}))?;
    let saved = serde_json::to_value(&kept)?;
    assert_eq!(saved["tools"]["browser"]["takeoverMinutes"], 20);
    assert_eq!(
        saved["tools"]["browser"]["view"]["url"],
        "http://100.64.0.7:6080"
    );
    assert!(!format!("{:?}", kept.tools.browser.view).contains("abcd1234"));
    Ok(())
}
