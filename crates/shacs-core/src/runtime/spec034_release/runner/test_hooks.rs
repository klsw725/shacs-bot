use super::super::model::CleanupReceipt;
use std::cell::RefCell;
use std::path::Path;

type PathHook = Box<dyn FnOnce(&Path)>;
type ReceiptHook = Box<dyn FnOnce(&mut CleanupReceipt)>;

thread_local! {
    static BEFORE_MANIFEST_CAPTURE: RefCell<Option<PathHook>> = RefCell::new(None);
    static AFTER_PENDING_VALIDATION: RefCell<Option<ReceiptHook>> = RefCell::new(None);
}

pub(super) fn inject_before_manifest_capture(hook: impl FnOnce(&Path) + 'static) {
    BEFORE_MANIFEST_CAPTURE.with(|slot| slot.replace(Some(Box::new(hook))));
}

pub(super) fn inject_after_pending_validation(
    hook: impl FnOnce(&mut CleanupReceipt) + 'static,
) {
    AFTER_PENDING_VALIDATION.with(|slot| slot.replace(Some(Box::new(hook))));
}

pub(super) fn before_manifest_capture(root: &Path) {
    BEFORE_MANIFEST_CAPTURE.with(|slot| {
        if let Some(hook) = slot.take() {
            hook(root);
        }
    });
}

pub(super) fn after_pending_validation(receipt: &mut CleanupReceipt) {
    AFTER_PENDING_VALIDATION.with(|slot| {
        if let Some(hook) = slot.take() {
            hook(receipt);
        }
    });
}
