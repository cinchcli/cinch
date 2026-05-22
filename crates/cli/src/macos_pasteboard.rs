//! macOS-only NSPasteboard PNG write for `cinch pull --copy`.
//!
//! `arboard::Clipboard::set_image` requires raw RGBA, which forces a PNG
//! decode (the `image` or `png` crate). NSPasteboard accepts PNG bytes
//! natively via the `public.png` UTI, so we go direct and keep the CLI's
//! dependency footprint small. Mirrors the desktop's
//! `clipboard::backend::macos::write_image_png` impl.

#![cfg(target_os = "macos")]

use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};

pub fn write_png(png_bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let cls = Class::get("NSPasteboard").ok_or("NSPasteboard class missing")?;
        let pb: *mut Object = msg_send![cls, generalPasteboard];
        let _: () = msg_send![pb, clearContents];

        let nsdata_cls = Class::get("NSData").ok_or("NSData class missing")?;
        let ns_data: *mut Object =
            msg_send![nsdata_cls, dataWithBytes:png_bytes.as_ptr() length:png_bytes.len()];

        let nsstring_cls = Class::get("NSString").ok_or("NSString class missing")?;
        let png_type: *mut Object =
            msg_send![nsstring_cls, stringWithUTF8String: c"public.png".as_ptr()];

        let ok: bool = msg_send![pb, setData:ns_data forType:png_type];
        if !ok {
            return Err("NSPasteboard setData:forType: returned false".into());
        }
    }
    Ok(())
}
