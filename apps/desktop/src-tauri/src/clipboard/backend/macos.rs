//! macOS clipboard backend — wraps NSPasteboard and NSWorkspace.
//!
//! All Objective-C pointers are fetched fresh per call; none are cached as
//! fields. This matches the pre-refactor behavior (autorelease pool safety).

use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};
use std::process::Command;

use super::{Backend, ClipboardError, PollContent, PollSnapshot};

pub(crate) struct MacBackend;

#[derive(Debug, Clone)]
struct FrontmostApp {
    bundle_id: Option<String>,
    display_name: Option<String>,
}

impl MacBackend {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Backend for MacBackend {
    fn read_snapshot(&mut self) -> Result<PollSnapshot, ClipboardError> {
        let frontmost_app = get_frontmost_app();
        let app_identity = frontmost_app.bundle_id;
        let app_name = frontmost_app.display_name;

        // Privacy gate (PRV-01, D-13): honor NSPasteboard concealed/transient
        // UTIs so clips from password managers and short-lived content (e.g.
        // 2FA codes) never reach the text/image read paths below, and never
        // reach the DB insert step in run_monitor_loop.
        //
        // Per RESEARCH.md Pitfall 2, read types BEFORE changeCount on this
        // path — if another app writes between them, the next poll tick
        // catches the subsequent state within the 500ms POLL_INTERVAL.
        let types = get_pasteboard_type_utis();
        if is_concealed_or_transient(&types) {
            let token = get_pasteboard_change_count() as u64;
            return Ok(PollSnapshot {
                token: Some(token),
                content: PollContent::Unsupported,
                app_identity,
                app_name,
            });
        }

        let token = get_pasteboard_change_count() as u64;

        if let Some(text) = get_pasteboard_string() {
            if !text.is_empty() {
                return Ok(PollSnapshot {
                    token: Some(token),
                    content: PollContent::Text(text),
                    app_identity,
                    app_name,
                });
            }
        }

        if let Some(bytes) = get_pasteboard_image_data() {
            return Ok(PollSnapshot {
                token: Some(token),
                content: PollContent::ImagePng(bytes),
                app_identity,
                app_name,
            });
        }

        Ok(PollSnapshot {
            token: Some(token),
            content: PollContent::Empty,
            app_identity,
            app_name,
        })
    }

    fn write_text(&mut self, content: &str) -> Result<(), ClipboardError> {
        // Use NSPasteboard directly instead of pbcopy. pbcopy spawns a subprocess
        // that may inherit no LANG env var (GUI apps launched from Finder don't get
        // the shell environment), causing it to fall back to defaultCStringEncoding
        // (Mac Roman) and misinterpret multi-byte UTF-8 sequences as individual
        // Mac Roman characters. NSPasteboard API is encoding-aware.
        unsafe {
            let cls = Class::get("NSPasteboard")
                .ok_or_else(|| ClipboardError::Backend("NSPasteboard class missing".into()))?;
            let pb: *mut Object = msg_send![cls, generalPasteboard];
            let _: () = msg_send![pb, clearContents];

            let nsstring_cls = Class::get("NSString")
                .ok_or_else(|| ClipboardError::Backend("NSString class missing".into()))?;
            let nsdata_cls = Class::get("NSData")
                .ok_or_else(|| ClipboardError::Backend("NSData class missing".into()))?;

            let bytes = content.as_bytes();
            let ns_data: *mut Object =
                msg_send![nsdata_cls, dataWithBytes:bytes.as_ptr() length:bytes.len()];

            // NSUTF8StringEncoding = 4
            let alloc: *mut Object = msg_send![nsstring_cls, alloc];
            let ns_string: *mut Object = msg_send![alloc, initWithData:ns_data encoding:4usize];
            if ns_string.is_null() {
                return Err(ClipboardError::Backend(
                    "NSString initWithData:encoding: returned nil for UTF-8 content".into(),
                ));
            }

            let pboard_type: *mut Object =
                msg_send![nsstring_cls, stringWithUTF8String: c"public.utf8-plain-text".as_ptr()];
            let _: bool = msg_send![pb, setString:ns_string forType:pboard_type];
        }
        Ok(())
    }

    fn write_image_png(&mut self, png_bytes: &[u8]) -> Result<(), ClipboardError> {
        unsafe {
            let cls = Class::get("NSPasteboard")
                .ok_or_else(|| ClipboardError::Backend("NSPasteboard class missing".into()))?;
            let pb: *mut Object = msg_send![cls, generalPasteboard];
            let _: () = msg_send![pb, clearContents];

            let nsdata_cls = Class::get("NSData")
                .ok_or_else(|| ClipboardError::Backend("NSData class missing".into()))?;
            let ns_data: *mut Object =
                msg_send![nsdata_cls, dataWithBytes:png_bytes.as_ptr() length:png_bytes.len()];

            let nsstring_cls = Class::get("NSString")
                .ok_or_else(|| ClipboardError::Backend("NSString class missing".into()))?;
            let png_type: *mut Object =
                msg_send![nsstring_cls, stringWithUTF8String: c"public.png".as_ptr()];

            let _result: bool = msg_send![pb, setData:ns_data forType:png_type];
        }
        Ok(())
    }

    fn default_excluded_apps(&self) -> Vec<String> {
        vec![
            "com.1password.1password".into(),
            "com.agilebits.onepassword7".into(),
            "com.bitwarden.desktop".into(),
            "com.lastpass.LastPass".into(),
            "com.apple.keychainaccess".into(),
        ]
    }
}

fn get_pasteboard_change_count() -> i64 {
    unsafe {
        let cls = Class::get("NSPasteboard").expect("NSPasteboard class not found");
        let pb: *mut Object = msg_send![cls, generalPasteboard];
        msg_send![pb, changeCount]
    }
}

fn get_frontmost_app() -> FrontmostApp {
    unsafe {
        let Some(cls) = Class::get("NSWorkspace") else {
            return FrontmostApp {
                bundle_id: None,
                display_name: None,
            };
        };
        let ws: *mut Object = msg_send![cls, sharedWorkspace];
        let app: *mut Object = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return FrontmostApp {
                bundle_id: None,
                display_name: None,
            };
        }

        let bid: *mut Object = msg_send![app, bundleIdentifier];
        let bundle_id = nsstring_to_string(bid);

        let name: *mut Object = msg_send![app, localizedName];
        let display_name = nsstring_to_string(name).or_else(|| bundle_id.clone());
        FrontmostApp {
            bundle_id,
            display_name,
        }
    }
}

unsafe fn nsstring_to_string(value: *mut Object) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let utf8: *const std::os::raw::c_char = msg_send![value, UTF8String];
    if utf8.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned(),
    )
}

pub(crate) fn active_browser_url_for_bundle(bundle_id: &str) -> Option<String> {
    let script = browser_url_script(bundle_id)?;
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!url.is_empty()).then_some(url)
}

fn browser_url_script(bundle_id: &str) -> Option<&'static str> {
    match bundle_id {
        "com.apple.Safari" => Some(
            r#"tell application id "com.apple.Safari" to get URL of current tab of front window"#,
        ),
        "com.apple.SafariTechnologyPreview" => Some(
            r#"tell application id "com.apple.SafariTechnologyPreview" to get URL of current tab of front window"#,
        ),
        "com.google.Chrome" => Some(chromium_url_script("com.google.Chrome")),
        "com.google.Chrome.canary" => Some(chromium_url_script("com.google.Chrome.canary")),
        "com.brave.Browser" => Some(chromium_url_script("com.brave.Browser")),
        "com.microsoft.edgemac" => Some(chromium_url_script("com.microsoft.edgemac")),
        "com.vivaldi.Vivaldi" => Some(chromium_url_script("com.vivaldi.Vivaldi")),
        "com.operasoftware.Opera" => Some(chromium_url_script("com.operasoftware.Opera")),
        _ => None,
    }
}

fn chromium_url_script(bundle_id: &str) -> &'static str {
    match bundle_id {
        "com.google.Chrome" => {
            r#"tell application id "com.google.Chrome" to get URL of active tab of front window"#
        }
        "com.google.Chrome.canary" => {
            r#"tell application id "com.google.Chrome.canary" to get URL of active tab of front window"#
        }
        "com.brave.Browser" => {
            r#"tell application id "com.brave.Browser" to get URL of active tab of front window"#
        }
        "com.microsoft.edgemac" => {
            r#"tell application id "com.microsoft.edgemac" to get URL of active tab of front window"#
        }
        "com.vivaldi.Vivaldi" => {
            r#"tell application id "com.vivaldi.Vivaldi" to get URL of active tab of front window"#
        }
        "com.operasoftware.Opera" => {
            r#"tell application id "com.operasoftware.Opera" to get URL of active tab of front window"#
        }
        _ => unreachable!("unsupported Chromium-family browser bundle id"),
    }
}

fn get_pasteboard_image_data() -> Option<Vec<u8>> {
    unsafe {
        let cls = Class::get("NSPasteboard")?;
        let pb: *mut Object = msg_send![cls, generalPasteboard];

        let nsstring_cls = Class::get("NSString")?;

        let png_type: *mut Object =
            msg_send![nsstring_cls, stringWithUTF8String: c"public.png".as_ptr()];
        let data: *mut Object = msg_send![pb, dataForType: png_type];
        if !data.is_null() {
            let length: usize = msg_send![data, length];
            let bytes: *const u8 = msg_send![data, bytes];
            if length > 0 && !bytes.is_null() {
                return Some(std::slice::from_raw_parts(bytes, length).to_vec());
            }
        }

        let tiff_type: *mut Object =
            msg_send![nsstring_cls, stringWithUTF8String: c"public.tiff".as_ptr()];
        let data: *mut Object = msg_send![pb, dataForType: tiff_type];
        if !data.is_null() {
            let length: usize = msg_send![data, length];
            let bytes: *const u8 = msg_send![data, bytes];
            if length > 0 && !bytes.is_null() {
                return Some(std::slice::from_raw_parts(bytes, length).to_vec());
            }
        }

        None
    }
}

fn get_pasteboard_string() -> Option<String> {
    unsafe {
        let cls = Class::get("NSPasteboard")?;
        let pb: *mut Object = msg_send![cls, generalPasteboard];

        let nsstring_cls = Class::get("NSString")?;
        let string_type: *mut Object =
            msg_send![nsstring_cls, stringWithUTF8String: c"public.utf8-plain-text".as_ptr()];

        let result: *mut Object = msg_send![pb, stringForType: string_type];
        if result.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![result, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(utf8)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// UTIs applications set on NSPasteboard to signal confidential
/// (password-manager-style) or momentary (2FA code) content.
/// Canonical source: https://nspasteboard.org/
const CONCEALED_UTI: &str = "org.nspasteboard.ConcealedType";
const TRANSIENT_UTI: &str = "org.nspasteboard.TransientType";

/// Pure helper — testable without the objc runtime.
/// Returns true if any element of `types` is a concealed or transient UTI.
pub(crate) fn is_concealed_or_transient(types: &[String]) -> bool {
    types
        .iter()
        .any(|t| t == CONCEALED_UTI || t == TRANSIENT_UTI)
}

/// Iterate `[NSPasteboard generalPasteboard].types` and return each UTI
/// as an owned `String`. Returns empty Vec on any objc/null failure —
/// conservative: a failed fetch is treated as "no concealed types".
fn get_pasteboard_type_utis() -> Vec<String> {
    unsafe {
        let Some(cls) = Class::get("NSPasteboard") else {
            return Vec::new();
        };
        let pb: *mut Object = msg_send![cls, generalPasteboard];
        let types_arr: *mut Object = msg_send![pb, types];
        if types_arr.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![types_arr, count];
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let ns_str: *mut Object = msg_send![types_arr, objectAtIndex: i];
            if ns_str.is_null() {
                continue;
            }
            let utf8: *const std::os::raw::c_char = msg_send![ns_str, UTF8String];
            if utf8.is_null() {
                continue;
            }
            out.push(
                std::ffi::CStr::from_ptr(utf8)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{browser_url_script, is_concealed_or_transient, CONCEALED_UTI, TRANSIENT_UTI};

    #[test]
    fn detect_concealed_transient() {
        assert!(
            is_concealed_or_transient(&[CONCEALED_UTI.to_string()]),
            "ConcealedType UTI must be detected"
        );
        assert!(
            is_concealed_or_transient(&[TRANSIENT_UTI.to_string()]),
            "TransientType UTI must be detected"
        );
        assert!(
            is_concealed_or_transient(&[
                "public.utf8-plain-text".into(),
                CONCEALED_UTI.to_string(),
            ]),
            "mixed UTI list with Concealed present must detect"
        );
        assert!(
            !is_concealed_or_transient(&["public.utf8-plain-text".into()]),
            "plain text must not be classified as concealed"
        );
        assert!(
            !is_concealed_or_transient(&[]),
            "empty types list must not be classified as concealed"
        );
    }

    #[test]
    fn utis_are_canonical_strings() {
        // Lock the exact canonical UTI strings per nspasteboard.org.
        assert_eq!(CONCEALED_UTI, "org.nspasteboard.ConcealedType");
        assert_eq!(TRANSIENT_UTI, "org.nspasteboard.TransientType");
    }

    #[test]
    fn browser_url_scripts_cover_common_macos_browsers() {
        assert!(browser_url_script("com.apple.Safari")
            .unwrap()
            .contains("current tab"));
        assert!(browser_url_script("com.google.Chrome")
            .unwrap()
            .contains("active tab"));
        assert!(browser_url_script("com.brave.Browser").is_some());
        assert!(browser_url_script("org.mozilla.firefox").is_none());
    }
}
