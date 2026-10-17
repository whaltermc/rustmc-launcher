package dev.rustmc.launcher;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.Typeface;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

public class MainActivity extends Activity {
    static {
        System.loadLibrary("rml_ui");
    }

    private native String nativeSetDataDir(String path);
    private native String nativeInfo();
    private native String nativeAccounts();
    private native String nativeAddOffline(String name);
    private native String nativeLoginBegin();
    private native String nativeLoginState();
    private native String nativeRuntimeStatus();
    private native String nativeRuntimeInstall(String name);
    private native String nativeRuntimeProgress();
    private native String nativeJvmProbe(String name);
    private native String nativeJvmProbeState();

    private final Handler handler = new Handler(Looper.getMainLooper());
    private TextView status;
    private TextView accounts;
    private TextView runtimes;
    private TextView runtimeProgress;
    private TextView jvmResult;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        nativeSetDataDir(getFilesDir().getAbsolutePath());

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.parseColor("#111318"));
        root.setPadding(48, 96, 48, 48);

        TextView title = text("RustMC Launcher", "#ffffff", 28f, false);
        root.addView(title);

        TextView info = text(nativeInfo(), "#9fe870", 14f, false);
        info.setPadding(0, 16, 0, 16);
        root.addView(info);

        root.addView(section("Accounts"));

        accounts = text("", "#cfd6e4", 14f, false);
        root.addView(accounts);

        EditText nameInput = new EditText(this);
        nameInput.setHint("Offline username");
        nameInput.setTextColor(Color.WHITE);
        nameInput.setHintTextColor(Color.parseColor("#6b7280"));
        nameInput.setInputType(InputType.TYPE_CLASS_TEXT);
        root.addView(nameInput);

        root.addView(button("Add offline account", () -> {
            nativeAddOffline(nameInput.getText().toString());
            refreshAccounts();
        }));

        status = text("", "#cfd6e4", 13f, false);
        root.addView(status);

        root.addView(button("Login with Microsoft", () -> {
            status.setText(nativeLoginBegin());
            pollLogin();
        }));

        root.addView(section("Java runtime"));

        runtimes = text("", "#cfd6e4", 13f, false);
        root.addView(runtimes);

        runtimeProgress = text("", "#f5c451", 13f, false);
        root.addView(runtimeProgress);

        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.addView(button("Install 17", () -> installRuntime("Internal-17")));
        row.addView(button("Install 21", () -> installRuntime("Internal-21")));
        row.addView(button("Install 25", () -> installRuntime("Internal-25")));
        root.addView(row);

        root.addView(section("JVM"));

        jvmResult = text("", "#cfd6e4", 13f, false);
        root.addView(jvmResult);

        root.addView(button("Probe JVM 21", () -> {
            jvmResult.setText(nativeJvmProbe("Internal-21"));
            pollProbe();
        }));

        ScrollView scroll = new ScrollView(this);
        scroll.addView(root);
        setContentView(scroll);

        refreshAccounts();
        refreshRuntimes();
    }

    private TextView text(String value, String color, float size, boolean bold) {
        TextView v = new TextView(this);
        v.setText(value);
        v.setTextColor(Color.parseColor(color));
        v.setTextSize(size);
        if (bold) {
            v.setTypeface(Typeface.DEFAULT_BOLD);
        }
        return v;
    }

    private TextView section(String value) {
        TextView v = text(value, "#ffffff", 18f, true);
        v.setPadding(0, 32, 0, 8);
        return v;
    }

    private Button button(String label, Runnable action) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setOnClickListener(view -> action.run());
        return b;
    }

    private void refreshAccounts() {
        accounts.setText(nativeAccounts());
    }

    private void refreshRuntimes() {
        runtimes.setText(nativeRuntimeStatus());
    }

    private void pollLogin() {
        handler.postDelayed(new Runnable() {
            @Override
            public void run() {
                String state = nativeLoginState();
                status.setText(state);
                if (state.startsWith("ok:")) {
                    refreshAccounts();
                    return;
                }
                if (state.startsWith("error:")) {
                    return;
                }
                handler.postDelayed(this, 1000);
            }
        }, 1000);
    }

    private void installRuntime(String name) {
        runtimeProgress.setText(nativeRuntimeInstall(name));
        pollInstall();
    }

    private void pollInstall() {
        handler.postDelayed(new Runnable() {
            @Override
            public void run() {
                String state = nativeRuntimeProgress();
                runtimeProgress.setText(state);
                if (state.startsWith("done:")) {
                    refreshRuntimes();
                    return;
                }
                if (state.startsWith("error:")) {
                    return;
                }
                handler.postDelayed(this, 1000);
            }
        }, 1000);
    }

    private void pollProbe() {
        handler.postDelayed(new Runnable() {
            @Override
            public void run() {
                String state = nativeJvmProbeState();
                jvmResult.setText(state);
                if (state.startsWith("ok:") || state.startsWith("error:")) {
                    return;
                }
                handler.postDelayed(this, 1000);
            }
        }, 1000);
    }
}
