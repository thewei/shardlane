//! Native WKWebView integration for the right-panel Browser (macOS only).
//!
//! Our crepuscularity-gpui has no webview element, so we take the equivalent
//! low-level route: mount the WKWebView directly on the GPUI content view as an
//! NSView (native subview compositing sits above GPUI's Metal layer), and every
//! render frame syncs the frame and visibility from the canvas-measured bounds —
//! the same compositing layering of "webview floats above everything GPUI paints".
//!
//! [INPUT]: objc2/objc2-app-kit/objc2-foundation/raw-window-handle and gpui's Window
//! [OUTPUT]: Provides BrowserWebview (ensure/load/sync_frame/set_visible/go_back/go_forward/reload/hide_all)
//! [POS]: Backend of the right_panel Browser surface; the render layer only calls in, unaware of AppKit details

#![allow(non_snake_case)]
// The ObjC selector method names of WKNavigationDelegate are declared exactly as
// the protocol defines them. Expanding extern_protocol! strips doc attributes
// from the trait, so clippy::missing_safety_doc cannot see the # Safety
// sections; the protocol itself mirrors WebKit's declaration, and the safety
// notes live at `show_load_failure_page` and the SAFETY comments at
// define_class.
#![allow(clippy::missing_safety_doc)]

use gpui::Window;
use objc2::rc::Allocated;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2::MainThreadOnly;
use objc2::{define_class, extern_class, extern_methods, extern_protocol, msg_send, DefinedClass};
use objc2_app_kit::{NSResponder, NSView};
use objc2_foundation::{NSRect, NSString, NSURLRequest, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

extern_class!(
    #[unsafe(super(objc2::runtime::NSObject))]
    #[name = "WKWebViewConfiguration"]
    #[thread_kind = MainThreadOnly]
    #[derive(Debug, PartialEq, Eq, Hash)]
    pub struct WKWebViewConfiguration;
);

impl WKWebViewConfiguration {
    extern_methods!(
        #[unsafe(method(new))]
        pub fn new(mtm: MainThreadMarker) -> Retained<Self>;

        #[unsafe(method(setWebsiteDataStore:))]
        #[allow(non_snake_case)]
        pub unsafe fn setWebsiteDataStore(&self, store: &WKWebsiteDataStore);
    );
}

extern_class!(
    #[unsafe(super(objc2::runtime::NSObject))]
    #[name = "WKWebsiteDataStore"]
    #[thread_kind = MainThreadOnly]
    #[derive(Debug, PartialEq, Eq, Hash)]
    pub struct WKWebsiteDataStore;
);

impl WKWebsiteDataStore {
    extern_methods!(
        /// Returns the shared default persistent data store.
        #[unsafe(method(defaultDataStore))]
        #[allow(non_snake_case)]
        pub fn defaultDataStore(mtm: MainThreadMarker) -> Retained<Self>;

        /// Returns a new non-persistent (ephemeral) data store.
        #[unsafe(method(nonPersistentDataStore))]
        #[allow(non_snake_case)]
        pub fn nonPersistentDataStore(mtm: MainThreadMarker) -> Retained<Self>;

        /// BROWSER-01: returns an independent persistent store by UUID (isolates
        /// cookies/cache/site data per profile). Returns nil when the string is
        /// not a valid UUID.
        #[unsafe(method(dataStoreForIdentifier:))]
        #[allow(non_snake_case)]
        pub unsafe fn dataStoreForIdentifier(
            identifier: &objc2_foundation::NSUUID,
        ) -> Option<Retained<Self>>;

        /// BROWSER-03: the set of all data types (for clear-data use; declared as
        /// id to simplify the FFI).
        #[unsafe(method(allWebsiteDataTypes))]
        #[allow(non_snake_case)]
        pub unsafe fn allWebsiteDataTypes() -> Option<Retained<objc2::runtime::AnyObject>>;

        /// BROWSER-03: clears data of the given types (async; the completion
        /// handler may be NULL).
        #[unsafe(method(removeDataOfTypes:modifiedSince:completionHandler:))]
        #[allow(non_snake_case)]
        pub unsafe fn removeDataOfTypes(
            &self,
            data_types: &objc2::runtime::AnyObject,
            date: &objc2_foundation::NSDate,
            handler: *mut objc2::runtime::AnyObject,
        );
    );
}

extern_class!(
    #[unsafe(super(NSView, NSResponder, objc2::runtime::NSObject))]
    #[name = "WKWebView"]
    #[thread_kind = MainThreadOnly]
    #[derive(Debug, PartialEq, Eq, Hash)]
    pub struct WKWebView;
);

impl WKWebView {
    extern_methods!(
        #[unsafe(method(initWithFrame:))]
        #[unsafe(method_family = init)]
        #[allow(non_snake_case)]
        pub unsafe fn initWithFrame(this: Allocated<Self>, frame: NSRect) -> Retained<Self>;

        #[unsafe(method(initWithFrame:configuration:))]
        #[unsafe(method_family = init)]
        #[allow(non_snake_case)]
        pub unsafe fn initWithFrame_configuration(
            this: Allocated<Self>,
            frame: NSRect,
            configuration: &WKWebViewConfiguration,
        ) -> Retained<Self>;

        #[unsafe(method(loadRequest:))]
        #[allow(non_snake_case)]
        /// Returns a WKNavigation (object); declared as AnyObject to match the
        /// real type code '@' — declaring it void makes objc2's runtime type
        /// validation panic.
        pub unsafe fn loadRequest(
            &self,
            request: &NSURLRequest,
        ) -> Option<Retained<objc2::runtime::AnyObject>>;

        #[unsafe(method(goBack))]
        #[allow(non_snake_case)]
        pub unsafe fn goBack(&self) -> bool;

        #[unsafe(method(goForward))]
        #[allow(non_snake_case)]
        pub unsafe fn goForward(&self) -> bool;

        #[unsafe(method(reload))]
        #[allow(non_snake_case)]
        /// Like loadRequest: returns a WKNavigation object.
        pub unsafe fn reload(&self) -> Option<Retained<objc2::runtime::AnyObject>>;

        #[unsafe(method(setAllowsBackForwardNavigationGestures:))]
        #[allow(non_snake_case)]
        pub unsafe fn setAllowsBackForwardNavigationGestures(&self, allowed: bool);

        #[unsafe(method(setNavigationDelegate:))]
        #[allow(non_snake_case)]
        pub unsafe fn setNavigationDelegate(
            &self,
            delegate: Option<&ProtocolObject<dyn WKNavigationDelegate>>,
        );

        #[unsafe(method(URL))]
        #[allow(non_snake_case)]
        pub unsafe fn URL(&self) -> Option<Retained<NSURL>>;
    );
}

// SAFETY: The method selectors match the WebKit `WKNavigationDelegate` protocol;
// implementors are called back by WebKit on the main thread.
extern_protocol!(
    /// WKNavigationDelegate — only a navigation-failure fallback; decidePolicy
    /// is not implemented (WebKit's default allows all navigation, avoiding the
    /// objc2 validation path of directly invoking the decision block).
    ///
    /// # Safety
    ///
    /// The method signatures must exactly match the WebKit
    /// `WKNavigationDelegate` protocol selectors; WebKit calls back on the main
    /// thread.
    #[allow(non_snake_case)]
    unsafe trait WKNavigationDelegate: NSObjectProtocol {
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        unsafe fn webView_didFailProvisionalNavigation_withError(
            &self,
            web_view: &WKWebView,
            navigation: Option<&objc2::runtime::AnyObject>,
            error: &objc2::runtime::AnyObject,
        );

        #[unsafe(method(webView:didFailNavigation:withError:))]
        unsafe fn webView_didFailNavigation_withError(
            &self,
            web_view: &WKWebView,
            navigation: Option<&objc2::runtime::AnyObject>,
            error: &objc2::runtime::AnyObject,
        );
    }
);

/// Navigation-failure fallback for the Browser webview (UX walkthrough #12: a
/// connection failure used to be a pure white dead canvas).
///
/// Failure info is sent back to the GPUI layer through a channel to render an
/// error card — messages are never sent to the webview from inside a WebKit
/// callback (a loadHTMLString fallback page was tried once and tripped objc2's
/// checked-msg_send validation panic, see the 2026-08-27 crash report), nor
/// does the callback touch UI entities.
#[derive(Debug)]
struct BrowserNavDelegateIvars {
    tx: async_channel::Sender<String>,
}

define_class!(
    // SAFETY:
    // - Superclass NSObject; the WKNavigationDelegate protocol method signatures
    //   match WebKit's.
    // - No strong reference to the webview is held: the callback arguments carry
    //   their own sender, so there is no retain-cycle problem.
    #[unsafe(super = objc2::runtime::NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = BrowserNavDelegateIvars]
    struct BrowserNavDelegate;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for BrowserNavDelegate {}

    // SAFETY: Matches the set of selectors declared by WebKit.
    unsafe impl WKNavigationDelegate for BrowserNavDelegate {
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        unsafe fn webView_didFailProvisionalNavigation_withError(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&objc2::runtime::AnyObject>,
            _error: &objc2::runtime::AnyObject,
        ) {
            let _ = self.ivars().tx.try_send(failed_url_of(_web_view));
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        unsafe fn webView_didFailNavigation_withError(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&objc2::runtime::AnyObject>,
            _error: &objc2::runtime::AnyObject,
        ) {
            let _ = self.ivars().tx.try_send(failed_url_of(_web_view));
        }
    }
);

fn failed_url_of(web_view: &WKWebView) -> String {
    let url = unsafe { web_view.URL() };
    url.as_ref()
        .map(|u| {
            let abs = u.absoluteString();
            abs.map(|a| a.to_string()).unwrap_or_default()
        })
        .unwrap_or_default()
}

/// One Browser surface corresponds to one resident WKWebView (reused by session key).
pub struct BrowserWebview {
    webview: Retained<WKWebView>,
    /// Navigation-failure fallback delegate (held to keep it alive; WebKit's side
    /// is a weak reference).
    _nav_delegate: Retained<BrowserNavDelegate>,
    /// Navigation-failure event stream (the GPUI layer consumes it to render the
    /// error card).
    failure_rx: async_channel::Receiver<String>,
    loaded_url: String,
}

/// BROWSER-03: clears all website data of the given profile.
/// profile_id = None/"default" → default store; UUID → the matching persistent
/// store; an ephemeral store dies with the webview and needs no clearing.
pub fn clear_profile_data(profile_id: Option<&str>) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let store = match profile_id.filter(|id| *id != "default" && !id.is_empty()) {
        Some(id) => {
            match objc2_foundation::NSUUID::from_string(&objc2_foundation::NSString::from_str(id)) {
                Some(uuid) => unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&uuid) },
                None => None,
            }
        }
        None => Some(WKWebsiteDataStore::defaultDataStore(mtm)),
    };
    let Some(store) = store else {
        return false;
    };
    let Some(types) = (unsafe { WKWebsiteDataStore::allWebsiteDataTypes() }) else {
        return false;
    };
    let past = objc2_foundation::NSDate::distantPast();
    unsafe {
        store.removeDataOfTypes(&types, &past, std::ptr::null_mut());
    }
    true
}

/// GPUI content view (its content coordinate system origin is top-left; NSView
/// defaults to bottom-left).
fn content_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    let view = unsafe { &*(handle.ns_view.as_ptr().cast::<NSView>()) };
    view.window()?.contentView()
}

impl BrowserWebview {
    /// Create a webview with explicit session configuration.
    /// BROWSER-01: `profile_id` (a UUID string) → its own independent persistent
    /// WKWebsiteDataStore; `ephemeral` = true → non-persistent store;
    /// None/"default" → the shared default store (backwards compatible).
    pub fn ensure_with_config(
        url: &str,
        ephemeral: bool,
        profile_id: Option<&str>,
        window: &Window,
    ) -> Option<Self> {
        let content = content_view(window)?;
        let bounds = content.bounds();
        let frame = NSRect::new(objc2_foundation::NSPoint::new(0.0, 0.0), bounds.size);
        let mtm = MainThreadMarker::new()?;

        let config = WKWebViewConfiguration::new(mtm);
        let named_profile = profile_id.filter(|id| *id != "default" && !id.is_empty());
        let data_store = if ephemeral {
            WKWebsiteDataStore::nonPersistentDataStore(mtm)
        } else if let Some(id) = named_profile {
            // BROWSER-01: valid UUID → independent persistent store; otherwise
            // degrade with a log trail.
            match objc2_foundation::NSUUID::from_string(&objc2_foundation::NSString::from_str(id)) {
                Some(uuid) => {
                    let store = unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&uuid) };
                    match store {
                        Some(store) => store,
                        None => {
                            crate::lag_log(format_args!(
                                "browser: dataStoreForIdentifier failed for profile {id}; falling back"
                            ));
                            WKWebsiteDataStore::defaultDataStore(mtm)
                        }
                    }
                }
                None => {
                    crate::lag_log(format_args!(
                        "browser: profile id {id} is not a UUID; falling back to default store"
                    ));
                    WKWebsiteDataStore::defaultDataStore(mtm)
                }
            }
        } else {
            WKWebsiteDataStore::defaultDataStore(mtm)
        };
        unsafe { config.setWebsiteDataStore(&data_store) };

        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config)
        };
        // UX (walkthrough #12): attach the navigation-failure fallback; the
        // failed URL goes through a channel for the GPUI layer to render an
        // error card.
        let (failure_tx, failure_rx) = async_channel::unbounded::<String>();
        let nav_delegate: Retained<BrowserNavDelegate> = {
            let partial = BrowserNavDelegate::alloc(mtm)
                .set_ivars(BrowserNavDelegateIvars { tx: failure_tx });
            // SAFETY: NSObject init signature is correct (the official objc2 pattern).
            unsafe { msg_send![super(partial), init] }
        };
        unsafe {
            webview.setNavigationDelegate(Some(objc2::runtime::ProtocolObject::from_ref(
                &*nav_delegate,
            )));
        }
        unsafe {
            webview.setAllowsBackForwardNavigationGestures(true);
        }
        unsafe {
            let _: () = msg_send![&*webview, setWantsLayer: true];
        }
        content.addSubview(&webview);
        let mut this = Self {
            webview,
            _nav_delegate: nav_delegate,
            failure_rx,
            loaded_url: String::new(),
        };
        this.load(url);
        Some(this)
    }

    /// Non-blocking take of the navigation-failure URL (at most one consumed per
    /// frame; None when none is pending).
    pub(crate) fn take_failure(&self) -> Option<String> {
        self.failure_rx.try_recv().ok()
    }

    pub fn load(&mut self, url: &str) {
        if url.is_empty() || url == self.loaded_url {
            return;
        }
        let ns_url = NSURL::URLWithString(&NSString::from_str(url));
        if let Some(ns_url) = ns_url {
            let request = NSURLRequest::requestWithURL(&ns_url);
            unsafe { self.webview.loadRequest(&request) };
            self.loaded_url = url.to_string();
        }
    }

    /// Called every render frame: sync the native frame (flipped from the
    /// canvas-measured bounds in window coordinates, top-left origin, to a
    /// bottom-left origin) and visibility. GPUI clipping has no effect on native
    /// subviews, so setHidden must be called explicitly when invisible.
    pub fn sync_frame(&self, bounds: gpui::Bounds<gpui::Pixels>, visible: bool, window: &Window) {
        let Some(content) = content_view(window) else {
            return;
        };
        let content_height = content.bounds().size.height;
        let x = f64::from(bounds.origin.x);
        let y = f64::from(bounds.origin.y);
        let width = f64::from(bounds.size.width);
        let height = f64::from(bounds.size.height);
        let native_y = content_height - y - height;
        let frame = NSRect::new(
            objc2_foundation::NSPoint::new(x, native_y),
            objc2_foundation::NSSize::new(width, height),
        );
        self.webview.setFrame(frame);
        self.webview.setHidden(!visible);
    }

    pub fn set_visible(&self, visible: bool) {
        self.webview.setHidden(!visible);
    }

    pub fn go_back(&self) {
        unsafe {
            self.webview.goBack();
        }
    }

    pub fn go_forward(&self) {
        unsafe {
            self.webview.goForward();
        }
    }

    pub fn reload(&self) {
        unsafe {
            self.webview.reload();
        }
    }

    /// Remove from the native view tree and release (when the surface closes; a
    /// Retained drop is not enough to remove a subview, an explicit
    /// removeFromSuperview is required).
    pub fn remove(self) {
        self.webview.removeFromSuperview();
    }
}

/// Hide all webviews when the panel closes/collapses (they float above GPUI and
/// do not leave with the element tree).
pub fn hide_all(
    webviews: &std::collections::HashMap<crate::browser_profile::BrowserSessionId, BrowserWebview>,
) {
    for webview in webviews.values() {
        webview.set_visible(false);
    }
}
