//! PGN Tree Viewer library: used by the desktop binary and, as `cdylib`,
//! by the Android app (NativeActivity, see `android/`).

mod app;
mod pgn;
#[cfg(target_os = "android")]
mod android;

pub use app::{native_options, run};

/// Android entry point, called by `android-activity` (NativeActivity).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    let mut options = native_options();
    options.android_app = Some(app);
    if let Err(e) = run(options) {
        eprintln!("PGN Tree Viewer stopped: {e}");
    }
}
