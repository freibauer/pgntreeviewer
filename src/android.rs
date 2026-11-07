//! Android glue: talks to `MainActivity.java` (a small NativeActivity
//! subclass) via JNI to open Android's file picker and to receive the
//! contents of a picked / "opened with" / shared PGN file.

use jni::JNIEnv;
use jni::objects::{JObject, JObjectArray, JString};

/// A file delivered by the Java side: Ok((display name, text)) or Err(message).
pub type Picked = Result<(String, String), String>;

fn with_activity<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
    let ctx = ndk_context::android_context();
    // SAFETY: android-activity initialises ndk-context with valid VM and activity pointers.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let result = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result.ok()
}

/// Show Android's document picker (ACTION_OPEN_DOCUMENT).
pub fn open_picker() {
    with_activity(|env, act| env.call_method(act, "openPicker", "()V", &[]).map(|_| ()));
}

/// Take the file the Java side has received since the last call, if any.
pub fn take_picked() -> Option<Picked> {
    with_activity(|env, act| {
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
    })
    .flatten()
}
