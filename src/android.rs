//! Android glue: talks to `MainActivity.java` (a small NativeActivity
//! subclass) via JNI to open Android's file picker, to receive the contents
//! of a picked / "opened with" / shared PGN file, and to query the system
//! bar insets.

use jni::JNIEnv;
use jni::objects::{JIntArray, JObject, JObjectArray, JString};
use std::sync::atomic::{AtomicPtr, Ordering};

/// JavaVM and the `MainActivity` object (JNI global ref owned by android-activity),
/// set once in `android_main`. Note: `ndk-context` would give the *Application*
/// object, which does not have our methods.
static VM: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static ACTIVITY: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

pub fn init(app: &android_activity::AndroidApp) {
    VM.store(app.vm_as_ptr(), Ordering::SeqCst);
    ACTIVITY.store(app.activity_as_ptr(), Ordering::SeqCst);
}

/// A file delivered by the Java side: Ok((display name, text)) or Err(message).
pub type Picked = Result<(String, String), String>;

/// Run `f` with the JNI environment and the activity object. Errors (including
/// Java exceptions, with their message) are returned as text for the UI.
fn with_activity<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Result<R, String> {
    let (vm_ptr, act_ptr) = (VM.load(Ordering::SeqCst), ACTIVITY.load(Ordering::SeqCst));
    if vm_ptr.is_null() || act_ptr.is_null() {
        return Err("activity not initialised".into());
    }
    // SAFETY: both pointers come from the AndroidApp, which lives for the whole app run.
    let vm = unsafe { jni::JavaVM::from_raw(vm_ptr.cast()) }.map_err(|e| format!("JavaVM: {e}"))?;
    let mut env = vm.attach_current_thread().map_err(|e| format!("JNI attach: {e}"))?;
    let activity = unsafe { JObject::from_raw(act_ptr.cast()) };
    let result = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let msg = env
            .exception_occurred()
            .ok()
            .and_then(|ex| {
                let _ = env.exception_clear();
                let s = env.call_method(&ex, "toString", "()Ljava/lang/String;", &[]).ok()?.l().ok()?;
                env.get_string(&JString::from(s)).ok().map(String::from)
            })
            .unwrap_or_else(|| "unknown Java exception".into());
        let _ = env.exception_clear();
        return Err(msg);
    }
    result.map_err(|e| format!("JNI: {e}"))
}

/// Show Android's document picker (ACTION_OPEN_DOCUMENT).
pub fn open_picker() -> Result<(), String> {
    with_activity(|env, act| env.call_method(act, "openPicker", "()V", &[]).map(|_| ()))
}

/// Take the file the Java side has received since the last call, if any.
pub fn take_picked() -> Option<Picked> {
    let r = with_activity(|env, act| {
        let arr = env.call_method(act, "take", "()[Ljava/lang/String;", &[])?.l()?;
        if arr.is_null() {
            return Ok(None);
        }
        let arr = JObjectArray::from(arr);
        let mut fields = Vec::with_capacity(3);
        for i in 0..3 {
            let obj = env.get_object_array_element(&arr, i)?;
            fields.push(if obj.is_null() {
                None
            } else {
                Some(String::from(env.get_string(&JString::from(obj))?))
            });
        }
        let [name, text, error]: [Option<String>; 3] = fields.try_into().unwrap();
        Ok(Some(match (text, error) {
            (_, Some(e)) => Err(e),
            (Some(t), None) => Ok((name.unwrap_or_else(|| "PGN".into()), t)),
            (None, None) => Err("empty file".into()),
        }))
    });
    match r {
        Ok(p) => p,
        Err(e) => Some(Err(e)),
    }
}

/// System bar insets in physical pixels: [left, top, right, bottom].
pub fn insets_px() -> Result<[i32; 4], String> {
    with_activity(|env, act| {
        let arr = JIntArray::from(env.call_method(act, "insets", "()[I", &[])?.l()?);
        let mut buf = [0i32; 4];
        env.get_int_array_region(&arr, 0, &mut buf)?;
        Ok(buf)
    })
}
