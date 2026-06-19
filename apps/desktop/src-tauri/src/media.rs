//! Store-backed `cinch://media/{clip_id}` serving. The local SQLite store
//! holds raw decrypted clip bytes (see cinch-core sync canonical model), so
//! image previews are served directly from it — no disk media cache.

use client_core::store::{queries, Store};

/// Sniff a supported image format and return its MIME type. Thin wrapper over
/// the shared `client_core::media` detector (the single source of truth).
pub(crate) fn image_content_type(data: &[u8]) -> Option<&'static str> {
    client_core::media::detect_image_format(data).map(|f| f.mime())
}

/// Result of a `cinch://media` lookup: HTTP status, content-type, body.
pub(crate) struct MediaResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// `Cache-Control` for a `cinch://` response, or `None` if it must not be
/// cached. Clip bytes are immutable per (ULID) clip id and ids are never
/// reused, so a successful image response is safe to cache hard — letting a
/// revisit hit the webview cache instead of re-reading the BLOB from SQLite on
/// every navigation. Non-200 responses (404 / errors) are never cached.
pub(crate) fn media_cache_control(status: u16) -> Option<&'static str> {
    (status == 200).then_some("public, max-age=31536000, immutable")
}

/// Serve an image clip's bytes from the store. 404 unless the clip exists,
/// is `content_type == "image"`, and has non-empty content.
pub(crate) fn serve_clip_image(store: &Store, clip_id: &str) -> MediaResponse {
    let not_found = || MediaResponse {
        status: 404,
        content_type: "application/octet-stream",
        body: Vec::new(),
    };
    if clip_id.is_empty() {
        return not_found();
    }
    let clip = match queries::get_clip(store, clip_id) {
        Ok(Some(c))
            if crate::commands::clips::normalize_content_type(c.content_type.clone())
                == "image" =>
        {
            c
        }
        Ok(_) => return not_found(),
        Err(e) => {
            log::warn!("serve_clip_image: store read failed for {}: {}", clip_id, e);
            return not_found();
        }
    };
    let bytes = match clip.content {
        Some(b) if !b.is_empty() => b,
        _ => return not_found(),
    };
    let ct = image_content_type(&bytes).unwrap_or("application/octet-stream");
    MediaResponse {
        status: 200,
        content_type: ct,
        body: bytes,
    }
}

/// Serve a macOS app icon as PNG from a bundle identifier.
pub(crate) fn serve_app_icon(bundle_id: &str) -> MediaResponse {
    let not_found = || MediaResponse {
        status: 404,
        content_type: "application/octet-stream",
        body: Vec::new(),
    };
    if bundle_id.is_empty() {
        return not_found();
    }

    match app_icon_png(bundle_id) {
        Some(body) => MediaResponse {
            status: 200,
            content_type: "image/png",
            body,
        },
        None => not_found(),
    }
}

#[cfg(target_os = "macos")]
fn app_icon_png(bundle_id: &str) -> Option<Vec<u8>> {
    app_icon_png_native(bundle_id).or_else(|| app_icon_png_with_image_crate_fallback(bundle_id))
}

#[cfg(target_os = "macos")]
fn app_icon_png_native(bundle_id: &str) -> Option<Vec<u8>> {
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};
    use std::ffi::CString;
    use std::ptr;

    let bundle_id = CString::new(bundle_id).ok()?;
    unsafe {
        let nsstring_cls = Class::get("NSString")?;
        let bundle: *mut Object = msg_send![nsstring_cls, stringWithUTF8String: bundle_id.as_ptr()];
        let path = app_path_for_bundle(bundle)?;

        let workspace_cls = Class::get("NSWorkspace")?;
        let workspace: *mut Object = msg_send![workspace_cls, sharedWorkspace];
        let icon: *mut Object = msg_send![workspace, iconForFile: path];
        if icon.is_null() {
            return None;
        }

        // Render the icon straight to a small CGImage instead of materializing
        // its full multi-resolution `TIFFRepresentation`: for a 1024px HDR app
        // icon that intermediate is ~74MB and PNG-encoding all 1024×1024 of it
        // costs ~300ms / ~2MB. The search bar shows these at ~20px, so a 64pt
        // proposed rect (128px @2x) is ample and costs <1ms / ~20KB. The rect is
        // passed as an `NSRect*` pointer to sidestep the struct-by-value msg_send
        // ABI; a NULL rect would render at the icon's full size — the very thing
        // we are avoiding. CGImageForProposedRect follows the Core Foundation Get
        // Rule (no "Create"/"Copy" in the selector): the returned CGImage is +0,
        // owned by the NSImage's cache — do NOT CFRelease it.
        #[repr(C)]
        struct NsRect {
            x: f64,
            y: f64,
            w: f64,
            h: f64,
        }
        let mut rect = NsRect {
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
        };
        let cg: *mut Object = msg_send![icon,
            CGImageForProposedRect: &mut rect as *mut NsRect
            context: ptr::null_mut::<Object>()
            hints: ptr::null_mut::<Object>()];
        if cg.is_null() {
            return None;
        }

        let bitmap_cls = Class::get("NSBitmapImageRep")?;
        // `init` consumes the `alloc`'d reference and returns the rep we own — the
        // same object, or a class-cluster substitute after freeing the original,
        // or nil having already freed it. Whichever non-nil object comes back is
        // released once below (after the PNG bytes are copied out), so each
        // dropdown icon balances its single retain and doesn't leak.
        let bitmap: *mut Object = msg_send![bitmap_cls, alloc];
        let bitmap: *mut Object = msg_send![bitmap, initWithCGImage: cg];
        if bitmap.is_null() {
            return None;
        }

        // NSBitmapImageFileType.png = 4. Let AppKit encode the small bitmap.
        let png_data: *mut Object =
            msg_send![bitmap, representationUsingType:4usize properties:ptr::null_mut::<Object>()];
        let out = nsdata_to_vec(png_data);
        let _: () = msg_send![bitmap, release];
        out
    }
}

/// Nil-safe `-[NSString isEqualToString:]`.
///
/// `app_path_for_bundle` compares each running app's `bundleIdentifier` against
/// the requested bundle id, but that identifier is nil for many processes
/// (agents/helpers with no Info.plist bundle id). `msg_send!` reborrows its
/// receiver as `&*receiver`, so sending to a nil receiver forms `&*nil` — a
/// null-pointer dereference that aborts in debug builds. Route nil-capable
/// receivers through here.
#[cfg(target_os = "macos")]
unsafe fn ns_string_eq(a: *mut objc::runtime::Object, b: *mut objc::runtime::Object) -> bool {
    use objc::{msg_send, sel, sel_impl};
    if a.is_null() || b.is_null() {
        return false;
    }
    msg_send![a, isEqualToString: b]
}

#[cfg(target_os = "macos")]
unsafe fn app_path_for_bundle(
    bundle: *mut objc::runtime::Object,
) -> Option<*mut objc::runtime::Object> {
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};

    let workspace_cls = Class::get("NSWorkspace")?;
    let workspace: *mut Object = msg_send![workspace_cls, sharedWorkspace];
    let app_url: *mut Object = msg_send![workspace, URLForApplicationWithBundleIdentifier: bundle];
    if !app_url.is_null() {
        let path: *mut Object = msg_send![app_url, path];
        if !path.is_null() {
            return Some(path);
        }
    }

    let apps: *mut Object = msg_send![workspace, runningApplications];
    if apps.is_null() {
        return None;
    }

    let count: usize = msg_send![apps, count];
    for i in 0..count {
        let app: *mut Object = msg_send![apps, objectAtIndex:i];
        if app.is_null() {
            continue;
        }

        let running_bundle: *mut Object = msg_send![app, bundleIdentifier];
        if !ns_string_eq(running_bundle, bundle) {
            continue;
        }

        let url: *mut Object = msg_send![app, bundleURL];
        if url.is_null() {
            continue;
        }
        let path: *mut Object = msg_send![url, path];
        if !path.is_null() {
            return Some(path);
        }
    }

    None
}

#[cfg(target_os = "macos")]
unsafe fn nsdata_to_vec(data: *mut objc::runtime::Object) -> Option<Vec<u8>> {
    use objc::{msg_send, sel, sel_impl};

    if data.is_null() {
        return None;
    }
    let len: usize = msg_send![data, length];
    let bytes: *const u8 = msg_send![data, bytes];
    if len == 0 || bytes.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(bytes, len).to_vec())
}

#[cfg(target_os = "macos")]
fn app_icon_png_with_image_crate_fallback(bundle_id: &str) -> Option<Vec<u8>> {
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};
    use std::ffi::CString;
    use std::io::Cursor;

    let bundle_id = CString::new(bundle_id).ok()?;
    let tiff = unsafe {
        let nsstring_cls = Class::get("NSString")?;
        let bundle: *mut Object = msg_send![nsstring_cls, stringWithUTF8String: bundle_id.as_ptr()];
        let path = app_path_for_bundle(bundle)?;
        let workspace_cls = Class::get("NSWorkspace")?;
        let workspace: *mut Object = msg_send![workspace_cls, sharedWorkspace];
        let icon: *mut Object = msg_send![workspace, iconForFile: path];
        if icon.is_null() {
            return None;
        }

        let data: *mut Object = msg_send![icon, TIFFRepresentation];
        if data.is_null() {
            return None;
        }
        let len: usize = msg_send![data, length];
        let bytes: *const u8 = msg_send![data, bytes];
        if len == 0 || bytes.is_null() {
            return None;
        }
        std::slice::from_raw_parts(bytes, len).to_vec()
    };

    let img = image::load_from_memory_with_format(&tiff, image::ImageFormat::Tiff).ok()?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(not(target_os = "macos"))]
fn app_icon_png(_: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::models::{StoredClip, SyncState};
    use client_core::store::Store;

    fn mem_store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn insert(store: &Store, id: &str, ct: &str, content: Option<Vec<u8>>) {
        queries::insert_clip(
            store,
            &StoredClip {
                id: id.into(),
                source: "remote:test".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: ct.into(),
                content,
                media_path: None,
                byte_size: 0,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();
    }

    #[test]
    fn serves_png_image_row() {
        let s = mem_store();
        let png = vec![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        insert(&s, "img1", "image", Some(png.clone()));
        let r = serve_clip_image(&s, "img1");
        assert_eq!(r.status, 200);
        assert_eq!(r.content_type, "image/png");
        assert_eq!(r.body, png);
    }

    #[test]
    fn serves_legacy_mime_image_row() {
        let s = mem_store();
        let png = vec![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        insert(&s, "legacy1", "image/png", Some(png.clone()));
        let r = serve_clip_image(&s, "legacy1");
        assert_eq!(r.status, 200);
        assert_eq!(r.content_type, "image/png");
        assert_eq!(r.body, png);
    }

    #[test]
    fn text_row_is_404() {
        let s = mem_store();
        insert(&s, "t1", "text", Some(b"hello".to_vec()));
        assert_eq!(serve_clip_image(&s, "t1").status, 404);
    }

    #[test]
    fn cache_control_is_set_only_for_success() {
        // Clip bytes are immutable per (ULID) clip id and ids are never reused,
        // so a 200 image response is safe to cache hard — this is what lets a
        // revisit hit the webview cache instead of re-reading the BLOB. Errors
        // (404 / 5xx) must never be cached.
        assert_eq!(
            media_cache_control(200),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(media_cache_control(404), None);
        assert_eq!(media_cache_control(500), None);
    }

    #[test]
    fn missing_and_empty_are_404() {
        let s = mem_store();
        assert_eq!(serve_clip_image(&s, "nope").status, 404);
        insert(&s, "e1", "image", None);
        assert_eq!(serve_clip_image(&s, "e1").status, 404);
    }

    #[test]
    fn empty_app_icon_request_is_404() {
        assert_eq!(serve_app_icon("").status, 404);
    }

    // Regression: opening the search bar's app filter dropdown asks
    // `serve_app_icon` for every historical source app. Bundle ids of apps that
    // are no longer installed fall into `app_path_for_bundle`'s running-apps
    // fallback loop, which compares each running app's `bundleIdentifier`
    // against the target. That identifier is nil for many processes, and
    // `msg_send!` reborrows its receiver as `&*nil` — a null-pointer dereference
    // that aborts the whole app in debug builds. `ns_string_eq` must treat a nil
    // receiver as "not equal" instead of dereferencing it.
    #[cfg(target_os = "macos")]
    #[test]
    fn ns_string_eq_is_nil_safe() {
        use objc::runtime::{Class, Object};
        use objc::{msg_send, sel, sel_impl};
        use std::ptr;
        unsafe {
            let cls = Class::get("NSString").unwrap();
            let s: *mut Object = msg_send![cls, stringWithUTF8String: c"com.apple.Safari".as_ptr()];
            // A nil receiver must compare unequal — never `&*nil`.
            assert!(!ns_string_eq(ptr::null_mut(), s));
            // A real NSString equals itself (sanity that the send still works).
            assert!(ns_string_eq(s, s));
        }
    }

    // End-to-end: a bundle id that no installed app owns drives
    // `app_path_for_bundle` all the way through the running-apps fallback loop
    // (the path that crashed). It must return 404 cleanly, never abort.
    #[cfg(target_os = "macos")]
    #[test]
    fn unresolvable_bundle_id_returns_404_without_aborting() {
        let response = serve_app_icon("com.cinch.test.definitely-not-installed");
        assert_eq!(response.status, 404);
    }

    // Regression: the search-bar app-filter dropdown renders these icons at
    // ~20px, but `serve_app_icon` used to materialize the full multi-resolution
    // app icon — a ~74MB `TIFFRepresentation` PNG-encoded at 1024×1024 (~300ms,
    // ~2MB each). Opening the dropdown serialized that on the main thread and
    // froze the UI. The served PNG must be a small thumbnail, not the full icon.
    #[cfg(target_os = "macos")]
    #[test]
    fn app_icon_is_a_small_thumbnail_not_full_resolution() {
        let r = serve_app_icon("com.apple.TextEdit");
        assert_eq!(r.status, 200);
        assert!(r.body.starts_with(b"\x89PNG\r\n\x1a\n"));
        // PNG IHDR: width @ bytes 16..20, height @ 20..24 (big-endian).
        let w = u32::from_be_bytes(r.body[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(r.body[20..24].try_into().unwrap());
        assert!(
            w <= 256 && h <= 256,
            "icon should be a downscaled thumbnail, got {w}x{h}"
        );
        assert!(
            r.body.len() < 200_000,
            "icon PNG should be small, got {}B",
            r.body.len()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn textedit_app_icon_request_returns_png() {
        let response = serve_app_icon("com.apple.TextEdit");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "image/png");
        assert!(response.body.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
