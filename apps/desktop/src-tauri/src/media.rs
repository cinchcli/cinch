//! Store-backed `cinch://media/{clip_id}` serving. The local SQLite store
//! holds raw decrypted clip bytes (see cinch-core sync canonical model), so
//! image previews are served directly from it — no disk media cache.

use client_core::store::{queries, Store};

/// Sniff a supported image format. Mirrors the CLI `detect_content_type`
/// signatures (PNG/JPEG/GIF/WebP/TIFF/BMP).
pub(crate) fn image_content_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else if data.starts_with(b"II\x2a\x00") || data.starts_with(b"MM\x00\x2a") {
        Some("image/tiff")
    } else if data.starts_with(b"BM") && data.len() >= 14 && data[6..10] == [0, 0, 0, 0] {
        Some("image/bmp")
    } else {
        None
    }
}

/// Result of a `cinch://media` lookup: HTTP status, content-type, body.
pub(crate) struct MediaResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
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

        let data: *mut Object = msg_send![icon, TIFFRepresentation];
        if data.is_null() {
            return None;
        }

        let bitmap_cls = Class::get("NSBitmapImageRep")?;
        let bitmap: *mut Object = msg_send![bitmap_cls, imageRepWithData: data];
        if bitmap.is_null() {
            return None;
        }

        // NSBitmapImageFileType.png = 4. Let AppKit do the conversion; some app
        // icons produce TIFF data that the Rust image crate cannot decode.
        let png_data: *mut Object =
            msg_send![bitmap, representationUsingType:4usize properties:ptr::null_mut::<Object>()];
        nsdata_to_vec(png_data)
    }
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
        let matches: bool = msg_send![running_bundle, isEqualToString: bundle];
        if !matches {
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

    #[cfg(target_os = "macos")]
    #[test]
    fn textedit_app_icon_request_returns_png() {
        let response = serve_app_icon("com.apple.TextEdit");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "image/png");
        assert!(response.body.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
