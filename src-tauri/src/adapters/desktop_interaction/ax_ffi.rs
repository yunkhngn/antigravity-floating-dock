#![allow(non_upper_case_globals)]

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFGetTypeID, CFRelease, CFRetain, CFType, CFTypeID, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::window::{
    kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionAll,
    CGWindowListCopyWindowInfo,
};
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use std::ffi::{c_char, c_void, CStr};
use std::thread;
use std::time::Duration;

pub type AXUIElementRef = *const c_void;
pub type AXValueRef = *const c_void;
pub type AXError = i32;

pub const kAXErrorSuccess: AXError = 0;
const kAXValueCGPointType: u32 = 1;
const kAXValueCGSizeType: u32 = 2;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementIsAttributeSettable(
        element: AXUIElementRef,
        attribute: CFStringRef,
        settable: *mut bool,
    ) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn AXUIElementGetTypeID() -> CFTypeID;
    fn AXValueGetTypeID() -> CFTypeID;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> bool;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[derive(Debug)]
pub struct AxElement {
    raw: AXUIElementRef,
}

unsafe impl Send for AxElement {}
unsafe impl Sync for AxElement {}

impl Clone for AxElement {
    fn clone(&self) -> Self {
        unsafe {
            CFRetain(self.raw);
        }
        Self { raw: self.raw }
    }
}

impl Drop for AxElement {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                CFRelease(self.raw);
            }
        }
    }
}

impl AxElement {
    pub fn from_pid(pid: i32) -> Option<Self> {
        let raw = unsafe { AXUIElementCreateApplication(pid) };
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }

    pub fn copy_attr(&self, attr: &str) -> Result<CFType, AXError> {
        let key = CFString::new(attr);
        let mut out: CFTypeRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(self.raw, key.as_concrete_TypeRef(), &mut out)
        };
        if err == kAXErrorSuccess && !out.is_null() {
            Ok(unsafe { CFType::wrap_under_create_rule(out) })
        } else {
            Err(if err == kAXErrorSuccess { -1 } else { err })
        }
    }

    pub fn set_bool_attr(&self, attr: &str, value: bool) -> Result<(), AXError> {
        let key = CFString::new(attr);
        let cf_val = if value {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let err = unsafe {
            AXUIElementSetAttributeValue(
                self.raw,
                key.as_concrete_TypeRef(),
                cf_val.as_CFTypeRef(),
            )
        };
        if err == kAXErrorSuccess {
            Ok(())
        } else {
            Err(err)
        }
    }

    pub fn set_string_attr(&self, attr: &str, value: &str) -> Result<(), AXError> {
        let key = CFString::new(attr);
        let cf_val = CFString::new(value);
        let err = unsafe {
            AXUIElementSetAttributeValue(
                self.raw,
                key.as_concrete_TypeRef(),
                cf_val.as_CFTypeRef(),
            )
        };
        if err == kAXErrorSuccess {
            Ok(())
        } else {
            Err(err)
        }
    }

    pub fn is_attr_settable(&self, attr: &str) -> bool {
        let key = CFString::new(attr);
        let mut settable = false;
        let err = unsafe {
            AXUIElementIsAttributeSettable(self.raw, key.as_concrete_TypeRef(), &mut settable)
        };
        err == kAXErrorSuccess && settable
    }

    pub fn perform_action(&self, action: &str) -> Result<(), AXError> {
        let act = CFString::new(action);
        let err = unsafe { AXUIElementPerformAction(self.raw, act.as_concrete_TypeRef()) };
        if err == kAXErrorSuccess {
            Ok(())
        } else {
            Err(err)
        }
    }

    pub fn get_string(&self, attr: &str) -> Option<String> {
        let val = self.copy_attr(attr).ok()?;
        val.downcast::<CFString>().map(|s| s.to_string())
    }

    pub fn get_bool(&self, attr: &str) -> Option<bool> {
        let val = self.copy_attr(attr).ok()?;
        val.downcast::<CFBoolean>().map(bool::from)
    }

    pub fn get_children(&self, attr: &str) -> Vec<AxElement> {
        let Ok(val) = self.copy_attr(attr) else {
            return Vec::new();
        };
        let Some(arr) = val.downcast::<CFArray>() else {
            return Vec::new();
        };
        let ax_type_id = unsafe { AXUIElementGetTypeID() };
        let mut out = Vec::with_capacity(arr.len() as usize);
        for item in arr.iter() {
            let ptr = *item as CFTypeRef;
            if !ptr.is_null() && unsafe { CFGetTypeID(ptr) } == ax_type_id {
                unsafe { CFRetain(ptr) };
                out.push(AxElement { raw: ptr });
            }
        }
        out
    }

    pub fn get_element(&self, attr: &str) -> Option<AxElement> {
        let val = self.copy_attr(attr).ok()?;
        let ax_type_id = unsafe { AXUIElementGetTypeID() };
        if val.type_of() == ax_type_id {
            let ptr = val.as_CFTypeRef();
            unsafe { CFRetain(ptr) };
            Some(AxElement { raw: ptr })
        } else {
            None
        }
    }

    pub fn get_position(&self) -> Option<(f64, f64)> {
        let val = self.copy_attr("AXPosition").ok()?;
        let ax_val_id = unsafe { AXValueGetTypeID() };
        if val.type_of() != ax_val_id {
            return None;
        }
        let mut pt = CGPoint::new(0.0, 0.0);
        let ok = unsafe {
            AXValueGetValue(
                val.as_CFTypeRef(),
                kAXValueCGPointType,
                (&mut pt as *mut CGPoint).cast::<c_void>(),
            )
        };
        if ok {
            Some((pt.x, pt.y))
        } else {
            None
        }
    }

    pub fn get_size(&self) -> Option<(f64, f64)> {
        let val = self.copy_attr("AXSize").ok()?;
        let ax_val_id = unsafe { AXValueGetTypeID() };
        if val.type_of() != ax_val_id {
            return None;
        }
        let mut sz = CGSize::new(0.0, 0.0);
        let ok = unsafe {
            AXValueGetValue(
                val.as_CFTypeRef(),
                kAXValueCGSizeType,
                (&mut sz as *mut CGSize).cast::<c_void>(),
            )
        };
        if ok {
            Some((sz.width, sz.height))
        } else {
            None
        }
    }

    pub fn get_bounds(&self) -> Option<WindowBounds> {
        let (x, y) = self.get_position()?;
        let (width, height) = self.get_size()?;
        Some(WindowBounds {
            x,
            y,
            width,
            height,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct RunningAppInfo {
    pub pid: i32,
}

#[derive(Debug, Clone)]
pub struct CgWindowInfo {
    pub title: String,
    pub bounds: WindowBounds,
}

pub fn is_accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn prompt_accessibility_permission() -> bool {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let val = CFBoolean::true_value();
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), val.as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
}

pub fn running_apps_for_bundle(bundle_id: &str) -> Vec<RunningAppInfo> {
    autoreleasepool(|_| unsafe {
        let cf_bundle = CFString::new(bundle_id);
        let ns_bundle: *mut AnyObject = cf_bundle.as_concrete_TypeRef() as *mut AnyObject;
        let arr: *mut AnyObject =
            msg_send![class!(NSRunningApplication), runningApplicationsWithBundleIdentifier: ns_bundle];
        if arr.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![arr, count];
        let mut result = Vec::with_capacity(count);
        for idx in 0..count {
            let app: *mut AnyObject = msg_send![arr, objectAtIndex: idx];
            if app.is_null() {
                continue;
            }
            let pid: i32 = msg_send![app, processIdentifier];
            let terminated: Bool = msg_send![app, isTerminated];
            if terminated.as_bool() || pid <= 0 {
                continue;
            }
            result.push(RunningAppInfo { pid });
        }
        result
    })
}

pub fn frontmost_app_pid() -> Option<i32> {
    autoreleasepool(|_| unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        if pid > 0 {
            Some(pid)
        } else {
            None
        }
    })
}

pub fn activate_pid(pid: i32) -> bool {
    autoreleasepool(|_| unsafe {
        let app: *mut AnyObject =
            msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return false;
        }
        // NSApplicationActivateIgnoringOtherApps = 1 << 1 = 2
        let ok: Bool = msg_send![app, activateWithOptions: 2usize];
        ok.as_bool()
    })
}

pub fn pasteboard_read_string() -> Option<String> {
    autoreleasepool(|_| unsafe {
        let pb: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
        if pb.is_null() {
            return None;
        }
        let ptype = CFString::new("public.utf8-plain-text");
        let ns_type: *mut AnyObject = ptype.as_concrete_TypeRef() as *mut AnyObject;
        let val: *mut AnyObject = msg_send![pb, stringForType: ns_type];
        nsstring_to_rust(val)
    })
}

pub fn pasteboard_write_string(text: &str) -> Result<(), String> {
    autoreleasepool(|_| unsafe {
        let pb: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
        if pb.is_null() {
            return Err("Could not access macOS general pasteboard".to_string());
        }
        let _: isize = msg_send![pb, clearContents];
        let ptype = CFString::new("public.utf8-plain-text");
        let ns_type: *mut AnyObject = ptype.as_concrete_TypeRef() as *mut AnyObject;
        let cf_text = CFString::new(text);
        let ns_text: *mut AnyObject = cf_text.as_concrete_TypeRef() as *mut AnyObject;
        let ok: Bool = msg_send![pb, setString: ns_text, forType: ns_type];
        if ok.as_bool() {
            Ok(())
        } else {
            Err("Failed to write message to macOS pasteboard".to_string())
        }
    })
}

unsafe fn nsstring_to_rust(ns_str: *mut AnyObject) -> Option<String> {
    if ns_str.is_null() {
        return None;
    }
    let ptr: *const c_char = msg_send![ns_str, UTF8String];
    if ptr.is_null() {
        return None;
    }
    Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
}

pub fn list_cg_windows_for_pid(target_pid: i32) -> Vec<CgWindowInfo> {
    let opts = kCGWindowListOptionAll | kCGWindowListExcludeDesktopElements;
    let arr_ref: CFArrayRef = unsafe { CGWindowListCopyWindowInfo(opts, kCGNullWindowID) };
    if arr_ref.is_null() {
        return Vec::new();
    }
    let arr: CFArray = unsafe { CFArray::wrap_under_create_rule(arr_ref) };
    let mut windows = Vec::new();

    for item in arr.iter() {
        let dict_ref = *item as CFDictionaryRef;
        if dict_ref.is_null() {
            continue;
        }
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(dict_ref) };

        let pid = dict_i64(&dict, "kCGWindowOwnerPID").unwrap_or(0) as i32;
        if pid != target_pid {
            continue;
        }
        let layer = dict_i64(&dict, "kCGWindowLayer").unwrap_or(-1);
        if layer != 0 {
            continue;
        }
        let Some(bounds) = dict_bounds(&dict) else {
            continue;
        };
        if bounds.width < 240.0 || bounds.height < 200.0 {
            continue;
        }

        let title = dict_string(&dict, "kCGWindowName").unwrap_or_default();

        windows.push(CgWindowInfo { title, bounds });
    }

    windows
}

fn dict_i64(dict: &CFDictionary<CFString, CFType>, key: &str) -> Option<i64> {
    let k = CFString::new(key);
    let v = dict.find(&k)?;
    v.downcast::<CFNumber>()?.to_i64()
}

fn dict_string(dict: &CFDictionary<CFString, CFType>, key: &str) -> Option<String> {
    let k = CFString::new(key);
    let v = dict.find(&k)?;
    v.downcast::<CFString>().map(|s| s.to_string())
}

fn dict_bounds(dict: &CFDictionary<CFString, CFType>) -> Option<WindowBounds> {
    let k = CFString::new("kCGWindowBounds");
    let v = dict.find(&k)?;
    let b_ref = v.as_CFTypeRef() as CFDictionaryRef;
    let b_dict: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_get_rule(b_ref) };
    let mut rect = CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(0.0, 0.0));
    let ok = unsafe { CGRectMakeWithDictionaryRepresentation(b_dict.as_concrete_TypeRef(), &mut rect) };
    if ok {
        Some(WindowBounds {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        })
    } else {
        None
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGRectMakeWithDictionaryRepresentation(
        dict: CFDictionaryRef,
        rect: *mut CGRect,
    ) -> bool;
}

pub fn post_key_combo(keycode: u16, flags: CGEventFlags) -> Result<(), String> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create CoreGraphics event source".to_string())?;
    let down = CGEvent::new_keyboard_event(source.clone(), keycode, true)
        .map_err(|_| "Failed to create key-down event".to_string())?;
    down.set_flags(flags);
    let up = CGEvent::new_keyboard_event(source, keycode, false)
        .map_err(|_| "Failed to create key-up event".to_string())?;
    up.set_flags(flags);

    down.post(CGEventTapLocation::HID);
    thread::sleep(Duration::from_millis(16));
    up.post(CGEventTapLocation::HID);
    Ok(())
}

pub fn post_mouse_click(x: f64, y: f64) -> Result<(), String> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create CoreGraphics mouse event source".to_string())?;
    let pt = CGPoint::new(x, y);

    let down = CGEvent::new_mouse_event(source.clone(), CGEventType::LeftMouseDown, pt, CGMouseButton::Left)
        .map_err(|_| "Failed to create mouse-down event".to_string())?;
    let up = CGEvent::new_mouse_event(source, CGEventType::LeftMouseUp, pt, CGMouseButton::Left)
        .map_err(|_| "Failed to create mouse-up event".to_string())?;

    down.post(CGEventTapLocation::HID);
    thread::sleep(Duration::from_millis(18));
    up.post(CGEventTapLocation::HID);
    Ok(())
}
