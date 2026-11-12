use serde_json::json;
use shacs_core::runtime::{
    BrowserAction, BrowserCallContext, BrowserDocument, BrowserError, BrowserHost, BrowserRead,
    BrowserSession, PermissionModeSnapshot,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct FakeHost(Arc<Mutex<Option<BrowserDocument>>>);

impl BrowserHost for FakeHost {
    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        if context.effective_session_id != "test-session" {
            return Err(BrowserError::Busy);
        }
        Ok(Box::new(self.clone()))
    }
}

impl BrowserSession for FakeHost {
    fn current_document(
        &self,
        context: &BrowserCallContext,
    ) -> Result<BrowserDocument, BrowserError> {
        if context.effective_session_id != "test-session" {
            return Err(BrowserError::ContextUnavailable);
        }
        self.0
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .clone()
            .ok_or(BrowserError::DocumentChanged)
    }
    fn read(&self, context: &BrowserCallContext) -> Result<BrowserRead, BrowserError> {
        Ok(BrowserRead {
            revision: "fixture".to_owned(),
            elements: Vec::new(),
            omitted_elements: 0,
            notice: None,
            document: self.current_document(context)?,
            accessibility: json!({"nodes":[]}),
            visible_text: "fixture".to_owned(),
            title: "fixture".to_owned(),
            boxes: Vec::new(),
            screenshot: context
                .browser_screenshots
                .unwrap_or(context.native_image_input_supported)
                .then(|| "fixture-image".to_owned()),
        })
    }
    fn act(
        &self,
        context: &BrowserCallContext,
        action: &BrowserAction,
    ) -> Result<BrowserRead, BrowserError> {
        if context.effective_session_id != "test-session" {
            return Err(BrowserError::ContextUnavailable);
        }
        let BrowserAction::Navigate { url } = action else {
            return Err(BrowserError::Protocol);
        };
        *self.0.lock().map_err(|_| BrowserError::StateUnavailable)? = Some(BrowserDocument {
            target_id: "target".to_owned(),
            session_id: "cdp-session".to_owned(),
            frame_id: "frame".to_owned(),
            loader_id: "loader".to_owned(),
            url: url.clone(),
        });
        self.read(context)
    }
    fn close(&self, context: &BrowserCallContext) -> Result<(), BrowserError> {
        self.current_document(context)?;
        self.0
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .take();
        Ok(())
    }
}

#[test]
fn host_contract_uses_borrowed_call_context_without_tool_registration() -> Result<(), BrowserError>
{
    let host: Box<dyn BrowserHost> = Box::new(FakeHost::default());
    let mut context = BrowserCallContext {
        effective_session_id: "test-session".to_owned(),
        permission_mode_snapshot: PermissionModeSnapshot::default(),
        permission_interactive: false,
        browser_screenshots: None,
        native_image_input_supported: false,
        media_root: None,
    };
    let session = host.acquire(&context)?;
    session.act(
        &context,
        &BrowserAction::Navigate {
            url: "https://example.com".to_owned(),
        },
    )?;
    assert!(session.read(&context)?.screenshot.is_none());
    context.browser_screenshots = Some(true);
    assert!(session.read(&context)?.screenshot.is_some());
    session.close(&context)?;
    assert_eq!(
        session.current_document(&context),
        Err(BrowserError::DocumentChanged)
    );
    Ok(())
}
