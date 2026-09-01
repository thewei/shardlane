//! Native macOS notification adapter for Shardlane-owned presentation.
//!
//! Herdr owns Agent lifecycle state. This module only presents selected authoritative
//! state transitions through macOS UserNotifications.framework, plus terminal BEL
//! feedback (dock attention request + throttled notifications when unfocused). P2-2 closure:
//! notifications carrying a pane_id route clicks back through UNUserNotificationCenterDelegate
//! to the `NotificationAction` channel, and main.rs's consumer loop reuses StatusBar's
//! FocusTarget to jump (the delegate only sends on the channel and never touches GPUI; the
//! weak-referenced delegate is retained permanently by a static OnceLock).
//! [INPUT]: Depends on objc2/objc2-foundation's NSString/NSDictionary/NSObject,
//! objc2-user-notifications's UNUserNotificationCenter/Delegate/Response, and
//! an async_channel Sender (the same static channel pattern as status_bar).
//! [OUTPUT]: Exposes show/show_with_pane (click-to-jump with pane_id),
//! request_authorization, request_dock_attention, NotificationAction,
//! attach_action_sender.
//! [POS]: A standalone notification adapter layer (the only AppKit egress); the focus action for
//! click-to-jump belongs to shell_navigation.handle_notification_action, reusing status_bar's focus semantics.

#[cfg(target_os = "macos")]
mod macos {
    use block2::RcBlock;
    use objc2::define_class;
    use objc2::ClassType;
    use objc2_foundation::{
        ns_string, NSBundle, NSDictionary, NSObject, NSObjectProtocol, NSString,
    };
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationResponse, UNNotificationTrigger, UNUserNotificationCenter,
        UNUserNotificationCenterDelegate,
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, Once, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::NotificationAction;

    const SHARDLANE_BUNDLE_IDENTIFIER: &str = "dev.shardlane.app";
    /// userInfo key: the click-to-jump routing key (the pane_id value).
    const USER_INFO_PANE_ID: &str = "pane_id";

    static AUTHORIZATION_REQUEST: Once = Once::new();
    static NOTIFICATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    /// Same static channel as status_bar::ACTION_SENDER: the delegate callback only try_sends.
    static ACTION_SENDER: Mutex<Option<async_channel::Sender<NotificationAction>>> =
        Mutex::new(None);
    /// UNUserNotificationCenter.delegate is a weak property; the object must retain its own lifetime.
    static CENTER_DELEGATE: OnceLock<objc2::rc::Retained<NotificationCenterTarget>> =
        OnceLock::new();

    fn bundle_identifier_supports_user_notifications(identifier: Option<&NSString>) -> bool {
        let Some(identifier) = identifier else {
            return false;
        };
        identifier.isEqualToString(&NSString::from_str(SHARDLANE_BUNDLE_IDENTIFIER))
    }

    fn bundle_supports_user_notifications(bundle: &NSBundle) -> bool {
        let identifier = bundle.bundleIdentifier();
        bundle_identifier_supports_user_notifications(identifier.as_deref())
    }

    fn user_notifications_available() -> bool {
        bundle_supports_user_notifications(&NSBundle::mainBundle())
    }

    define_class!(
        // SAFETY: the NSObject superclass has no subclassing requirements; no ivars; the callback only try_sends + calls completion.
        #[unsafe(super(NSObject))]
        #[name = "ShardlaneNotificationCenterTarget"]
        pub struct NotificationCenterTarget;

        unsafe impl NSObjectProtocol for NotificationCenterTarget {}

        unsafe impl UNUserNotificationCenterDelegate for NotificationCenterTarget {
            /// Click/dismiss notification callback: extract userInfo.pane_id and route it back to the GUI;
            /// the completion handler must be called (AppKit contract).
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive_response(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion_handler: &block2::DynBlock<dyn Fn()>,
            ) {
                let pane_id = response
                    .notification()
                    .request()
                    .content()
                    .userInfo()
                    .objectForKey(NSString::from_str(USER_INFO_PANE_ID).as_super().as_super())
                    .and_then(|value| value.downcast_ref::<NSString>().map(|s| s.to_string()));
                if let Some(pane_id) = pane_id {
                    if let Ok(guard) = ACTION_SENDER.lock() {
                        if let Some(sender) = guard.as_ref() {
                            let _ = sender.try_send(NotificationAction::FocusPane(pane_id));
                        }
                    }
                }
                completion_handler.call(());
            }
        }
    );

    /// Register the click delegate (idempotent): the weak property → the static OnceLock retains the object.
    fn ensure_click_delegate() {
        if !user_notifications_available() {
            return;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = CENTER_DELEGATE
            .get_or_init(|| unsafe { objc2::msg_send![NotificationCenterTarget::class(), new] });
        let protocol_object = objc2::runtime::ProtocolObject::from_ref(&**delegate);
        center.setDelegate(Some(protocol_object));
    }

    pub fn request_authorization() {
        if !user_notifications_available() {
            return;
        }
        AUTHORIZATION_REQUEST.call_once(|| {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let completion = RcBlock::new(|_granted, _error| {});
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert,
                &completion,
            );
        });
    }

    pub fn show(title: &str, body: &str) {
        post_notification(title, body, None);
    }

    /// Notification with pane_id routing: a click goes delegate → NotificationAction channel → jump to pane.
    pub fn show_with_pane(title: &str, body: &str, pane_id: &str) {
        post_notification(title, body, Some(pane_id));
    }

    fn post_notification(title: &str, body: &str, pane_id: Option<&str>) {
        if !user_notifications_available() {
            return;
        }
        request_authorization();
        ensure_click_delegate();
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();
        let title = NSString::from_str(title);
        let body = NSString::from_str(body);
        content.setTitle(&title);
        content.setBody(&body);
        if let Some(pane_id) = pane_id {
            // Generic parameters exist only at compile time: NSDictionary<NSString, NSString> and the
            // parameterless NSDictionary are the same ObjC type; the cast has no runtime effect.
            let value = NSString::from_str(pane_id);
            let typed = NSDictionary::from_slices(&[ns_string!(USER_INFO_PANE_ID)], &[&*value]);
            let untyped: objc2::rc::Retained<NSDictionary<objc2::runtime::AnyObject, objc2::runtime::AnyObject>> =
                // Generic parameters exist only at compile time: cast_unchecked only changes the static type; same ObjC object.
                unsafe { objc2::rc::Retained::cast_unchecked(typed) };
            unsafe { content.setUserInfo(&untyped) };
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        let sequence = NOTIFICATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let identifier = NSString::from_str(&format!("shardlane-agent-{timestamp}-{sequence}"));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None::<&UNNotificationTrigger>,
        );
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }

    /// Attach the click action channel (once at main.rs startup; same consumer-loop pattern as status_bar).
    pub fn attach_action_sender(sender: async_channel::Sender<NotificationAction>) {
        if let Ok(mut guard) = ACTION_SENDER.lock() {
            *guard = Some(sender);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn notification_bundle_gate_only_accepts_shardlane_app_identifier() {
            let expected = NSString::from_str(SHARDLANE_BUNDLE_IDENTIFIER);
            let wrong = NSString::from_str("dev.shardlane.debug");
            assert!(bundle_identifier_supports_user_notifications(Some(
                &expected
            )));
            assert!(!bundle_identifier_supports_user_notifications(Some(&wrong)));
            assert!(!bundle_identifier_supports_user_notifications(None));
        }

        #[test]
        fn raw_test_binary_does_not_enter_user_notifications_framework() {
            assert!(!user_notifications_available());
            request_authorization();
        }
    }
}

pub fn request_authorization() {
    #[cfg(target_os = "macos")]
    macos::request_authorization();
}

/// Request user attention via the Dock icon (on bell / long-task completion while the window is unfocused).
/// AppKit coalesces duplicate requests on its own; a no-op on non-macOS platforms.
pub fn request_dock_attention() {
    #[cfg(target_os = "macos")]
    {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            objc2_app_kit::NSApplication::sharedApplication(mtm).requestUserAttention(
                objc2_app_kit::NSRequestUserAttentionType::InformationalRequest,
            );
        }
    }
}

pub fn show(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    macos::show(title, body);

    #[cfg(not(target_os = "macos"))]
    let _ = (title, body);
}

/// P2-2 click-to-jump action: the delegate only sends on the channel; the jump runs in main.rs's consumer loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationAction {
    /// Activate the window and focus the Tab/Pane owning pane_id (reusing StatusBar FocusTarget semantics).
    FocusPane(String),
}

/// Attach the click action channel (idempotent; a later attach replaces the earlier one).
pub fn attach_action_sender(sender: async_channel::Sender<NotificationAction>) {
    #[cfg(target_os = "macos")]
    macos::attach_action_sender(sender);

    #[cfg(not(target_os = "macos"))]
    let _ = sender;
}

/// Notification with pane_id: clicking while unfocused jumps back to the pane; platforms without the bundle gate silently degrade to show.
pub fn show_with_pane(title: &str, body: &str, pane_id: &str) {
    #[cfg(target_os = "macos")]
    macos::show_with_pane(title, body, pane_id);

    #[cfg(not(target_os = "macos"))]
    let _ = (title, body, pane_id);
}
