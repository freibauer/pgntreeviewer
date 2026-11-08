package io.github.freibauer.pgntreeviewer;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
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
                pendingError = "Cannot open file: " + e;
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

    /** Chess PGN Master by Gerhard Kalab: pro version first, then the free one. */
    private static final String[] PGN_MASTER = {"com.kalab.pgnviewerpro", "com.kalab.pgnviewer"};

    /** Called from Rust: copy text (the FEN) to the clipboard. */
    public void copyText(final String text) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
                if (cm != null) cm.setPrimaryClip(ClipData.newPlainText("FEN", text));
            }
        });
    }

    private String installedPgnMaster() {
        PackageManager pm = getPackageManager();
        for (String p : PGN_MASTER) {
            try {
                pm.getPackageInfo(p, 0);
                return p;
            } catch (PackageManager.NameNotFoundException ignored) {
            }
        }
        return null;
    }

    /**
     * Called from Rust: hand the position to Chess PGN Master. The PGN is written
     * to cache/position.pgn and offered via PgnProvider. Android is asked which
     * hand-over the installed version supports (open file, share file, share
     * text), and the first match is started. Returns false if not installed.
     */
    public boolean sendToPgnMaster(final String fen, final String pgn) {
        final String pkg = installedPgnMaster();
        if (pkg == null) return false;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                String error;
                try {
                    java.io.FileOutputStream out = new java.io.FileOutputStream(PgnProvider.file(MainActivity.this));
                    out.write(pgn.getBytes("UTF-8"));
                    out.close();
                    Uri uri = Uri.parse("content://" + PgnProvider.AUTHORITY + "/" + PgnProvider.FILE_NAME);
                    java.util.List<Intent> tries = new java.util.ArrayList<>();
                    java.util.List<String> names = new java.util.ArrayList<>();
                    for (String type : new String[] {"application/x-chess-pgn", "application/vnd.chess-pgn",
                                                     "text/plain", "application/octet-stream", "*/*"}) {
                        Intent v = new Intent(Intent.ACTION_VIEW);
                        v.setDataAndType(uri, type);
                        tries.add(v);
                        names.add("VIEW file " + type);
                    }
                    for (String type : new String[] {"application/x-chess-pgn", "text/plain", "*/*"}) {
                        Intent st = new Intent(Intent.ACTION_SEND);
                        st.setType(type);
                        st.putExtra(Intent.EXTRA_STREAM, uri);
                        st.setClipData(ClipData.newRawUri(PgnProvider.FILE_NAME, uri));
                        tries.add(st);
                        names.add("SEND file " + type);
                    }
                    String[][] texts = {{"application/x-chess-pgn", pgn}, {"application/x-chess-fen", fen}, {"text/plain", pgn}};
                    for (String[] t : texts) {
                        Intent tx = new Intent(Intent.ACTION_SEND);
                        tx.setType(t[0]);
                        tx.putExtra(Intent.EXTRA_TEXT, t[1]);
                        tries.add(tx);
                        names.add("SEND text " + t[0]);
                    }
                    PackageManager pm = getPackageManager();
                    for (Intent i : tries) {
                        i.setPackage(pkg);
                        i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                        if (!pm.queryIntentActivities(i, 0).isEmpty()) {
                            startActivity(i);
                            return;
                        }
                    }
                    error = "Chess PGN Master (" + pkg + ") accepts none of: " + names;
                } catch (Exception e) {
                    error = "Chess PGN Master: " + e;
                }
                synchronized (MainActivity.this) {
                    pendingName = null;
                    pendingText = null;
                    pendingError = error;
                }
            }
        });
        return true;
    }

    /** Called from Rust: open Chess PGN Master in Google Play. */
    public void openPgnMasterInStore() {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                String id = PGN_MASTER[1];
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse("market://details?id=" + id)));
                } catch (ActivityNotFoundException e) {
                    startActivity(new Intent(Intent.ACTION_VIEW,
                        Uri.parse("https://play.google.com/store/apps/details?id=" + id)));
                }
            }
        });
    }

    /** Called from Rust: {name, text, error} of a newly received file, or null. */
    public synchronized String[] take() {
        if (pendingText == null && pendingError == null) return null;
        String[] r = {pendingName, pendingText, pendingError};
        pendingName = pendingText = pendingError = null;
        return r;
    }
}
