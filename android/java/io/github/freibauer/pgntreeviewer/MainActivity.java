package io.github.freibauer.pgntreeviewer;

import android.app.NativeActivity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.view.View;
import android.view.WindowInsets;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;

/**
 * NativeActivity running the Rust/egui viewer (libpgntreeviewer.so), plus the
 * two things that need Java: Android's document picker and reading content://
 * URIs (from the picker, "Open with" or "Share"). The Rust side polls take().
 */
public class MainActivity extends NativeActivity {
    private static final int PICK_PGN = 4711;

    private String pendingName;
    private String pendingText;
    private String pendingError;
    /** System bar insets {left, top, right, bottom} in px, updated on the UI thread. */
    private final int[] insets = new int[4];

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().getDecorView().setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
            @Override
            @SuppressWarnings("deprecation")
            public WindowInsets onApplyWindowInsets(View v, WindowInsets wi) {
                synchronized (MainActivity.this) {
                    insets[0] = wi.getSystemWindowInsetLeft();
                    insets[1] = wi.getSystemWindowInsetTop();
                    insets[2] = wi.getSystemWindowInsetRight();
                    insets[3] = wi.getSystemWindowInsetBottom();
                }
                return v.onApplyWindowInsets(wi);
            }
        });
        handleIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleIntent(intent);
    }

    private void handleIntent(Intent intent) {
        if (intent == null) return;
        Uri uri = null;
        if (Intent.ACTION_VIEW.equals(intent.getAction())) {
            uri = intent.getData();
        } else if (Intent.ACTION_SEND.equals(intent.getAction())) {
            uri = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        }
        if (uri != null) readUri(uri, false);
    }

    /** Called from Rust: show the system file picker. Errors are reported via take(). */
    public void openPicker() {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                    i.addCategory(Intent.CATEGORY_OPENABLE);
                    i.setType("*/*");
                    i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                    startActivityForResult(i, PICK_PGN);
                } catch (Exception e) {
                    synchronized (MainActivity.this) {
                        pendingName = null;
                        pendingText = null;
                        pendingError = "Cannot open file picker: " + e;
                    }
                }
            }
        });
    }

    /**
     * Called from Rust: system bar / display cutout insets in pixels
     * {left, top, right, bottom}. The native surface covers the whole window,
     * so the app keeps these areas free itself.
     */
    public synchronized int[] insets() {
        return insets.clone();
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == PICK_PGN && resultCode == RESULT_OK && data != null && data.getData() != null) {
            readUri(data.getData(), true);
        }
    }

    private static String readAll(InputStream in) throws java.io.IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] buf = new byte[64 * 1024];
        int n;
        while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
        return out.toString("UTF-8");
    }

    private synchronized void deliver(String name, String text, String error) {
        pendingName = name;
        pendingText = text;
        pendingError = error;
    }

    /** Read a picked / opened / shared file and remember it as "last PGN". */
    private void readUri(Uri uri, boolean persistable) {
        String name = displayName(uri);
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            if (in == null) throw new java.io.IOException("cannot open " + uri);
            String text = readAll(in);
            deliver(name, text, null);
            rememberLast(uri, name, text, persistable);
        } catch (Exception e) {
            deliver(name, null, "Cannot open file: " + e);
        }
    }

    // ---------- "Open last PGN" ----------
    private static final String PREFS = "pgntreeviewer";

    private File lastCopy() {
        return new File(getFilesDir(), "last.pgn");
    }

    /**
     * Keep the URI (with a persistable read permission when it came from the
     * picker, so the current file content can be re-read) and a private copy
     * as fallback (for "Open with"/"Share" URIs, which cannot be kept).
     */
    private void rememberLast(Uri uri, String name, String text, boolean persistable) {
        if (persistable) {
            try {
                getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (Exception ignored) {
            }
        }
        try (FileOutputStream out = new FileOutputStream(lastCopy())) {
            out.write(text.getBytes("UTF-8"));
        } catch (Exception ignored) {
        }
        getSharedPreferences(PREFS, MODE_PRIVATE).edit()
            .putString("lastUri", uri.toString())
            .putString("lastName", name)
            .apply();
    }

    /** Called from Rust: display name of the last opened PGN, or null. */
    public String lastName() {
        if (!lastCopy().exists()) return null;
        return getSharedPreferences(PREFS, MODE_PRIVATE).getString("lastName", null);
    }

    /** Called from Rust: open the last PGN again (current file if still readable, else the saved copy). */
    public void openLast() {
        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        String name = prefs.getString("lastName", "PGN");
        String uri = prefs.getString("lastUri", null);
        if (uri != null) {
            try (InputStream in = getContentResolver().openInputStream(Uri.parse(uri))) {
                if (in != null) {
                    deliver(name, readAll(in), null);
                    return;
                }
            } catch (Exception ignored) {
                // permission gone or file moved: use the copy
            }
        }
        try (InputStream in = new FileInputStream(lastCopy())) {
            deliver(name, readAll(in), null);
        } catch (Exception e) {
            deliver(name, null, "Cannot open last PGN: " + e);
        }
    }

    private String displayName(Uri uri) {
        try (Cursor c = getContentResolver().query(uri, null, null, null, null)) {
            if (c != null && c.moveToFirst()) {
                int idx = c.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (idx >= 0) return c.getString(idx);
            }
        } catch (Exception ignored) {
        }
        String last = uri.getLastPathSegment();
        return last != null ? last : "PGN";
    }

    /** Called from Rust: {name, text, error} of a newly received file, or null. */
    public synchronized String[] take() {
        if (pendingText == null && pendingError == null) return null;
        String[] r = {pendingName, pendingText, pendingError};
        pendingName = pendingText = pendingError = null;
        return r;
    }
}
