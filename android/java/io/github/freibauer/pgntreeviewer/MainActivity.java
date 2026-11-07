package io.github.freibauer.pgntreeviewer;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.view.View;
import android.view.WindowInsets;

import java.io.ByteArrayOutputStream;
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
        if (uri != null) readUri(uri);
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
                    startActivityForResult(i, PICK_PGN);
                } catch (Exception e) {
                    synchronized (MainActivity.this) {
                        pendingName = null;
                        pendingText = null;
                        pendingError = "file picker: " + e;
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
            readUri(data.getData());
        }
    }

    private void readUri(Uri uri) {
        String name = displayName(uri);
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            if (in == null) throw new java.io.IOException("cannot open " + uri);
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[64 * 1024];
            int n;
            while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
            synchronized (this) {
                pendingName = name;
                pendingText = out.toString("UTF-8");
                pendingError = null;
            }
        } catch (Exception e) {
            synchronized (this) {
                pendingName = name;
                pendingText = null;
                pendingError = e.toString();
            }
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
