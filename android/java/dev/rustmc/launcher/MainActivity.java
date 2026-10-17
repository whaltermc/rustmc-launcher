package dev.rustmc.launcher;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.Typeface;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.view.View;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

public class MainActivity extends Activity {
    static {
        System.loadLibrary("rml_ui");
    }

    private native String nativeInfo();
    private native String nativeAccounts();
    private native String nativeAddOffline(String name);
    private native String nativeLoginBegin();
    private native String nativeLoginState();

    private TextView status;
    private TextView accounts;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private boolean polling = false;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.parseColor("#111318"));
        root.setPadding(48, 96, 48, 48);

        TextView title = new TextView(this);
        title.setText("RustMC Launcher");
        title.setTextColor(Color.WHITE);
        title.setTextSize(28f);
        root.addView(title);

        TextView info = new TextView(this);
        info.setText(nativeInfo());
        info.setTextColor(Color.parseColor("#9fe870"));
        info.setTextSize(14f);
        info.setPadding(0, 16, 0, 16);
        root.addView(info);

        TextView accountsLabel = new TextView(this);
        accountsLabel.setText("Accounts");
        accountsLabel.setTextColor(Color.WHITE);
        accountsLabel.setTypeface(Typeface.DEFAULT_BOLD);
        accountsLabel.setPadding(0, 24, 0, 8);
        root.addView(accountsLabel);

        accounts = new TextView(this);
        accounts.setTextColor(Color.parseColor("#cfd6e4"));
        accounts.setTextSize(14f);
        root.addView(accounts);

        EditText nameInput = new EditText(this);
        nameInput.setHint("Offline username");
        nameInput.setTextColor(Color.WHITE);
        nameInput.setHintTextColor(Color.parseColor("#6b7280"));
        nameInput.setInputType(InputType.TYPE_CLASS_TEXT);
        root.addView(nameInput);

        Button addOffline = new Button(this);
        addOffline.setText("Add offline account");
        addOffline.setAllCaps(false);
        addOffline.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                String name = nameInput.getText().toString();
                nativeAddOffline(name);
                refreshAccounts();
            }
        });
        root.addView(addOffline);

        Button login = new Button(this);
        login.setText("Login with Microsoft");
        login.setAllCaps(false);
        login.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                String msg = nativeLoginBegin();
                status.setText(msg);
                startPolling();
            }
        });
        root.addView(login);

        status = new TextView(this);
        status.setTextColor(Color.parseColor("#f0c674"));
        status.setTextSize(14f);
        status.setPadding(0, 24, 0, 0);
        root.addView(status);

        ScrollView scroll = new ScrollView(this);
        scroll.addView(root);
        setContentView(scroll);

        refreshAccounts();
    }

    private void refreshAccounts() {
        String json = nativeAccounts();
        if (json == null || json.equals("[]")) {
            accounts.setText("(none)");
        } else {
            accounts.setText(json
                    .replace("[", "")
                    .replace("]", "")
                    .replace("},{", "}\n{")
                    .replace("\"", ""));
        }
    }

    private void startPolling() {
        if (polling) {
            return;
        }
        polling = true;
        handler.postDelayed(new Runnable() {
            @Override
            public void run() {
                String state = nativeLoginState();
                status.setText(state);
                if (state.startsWith("ok:")) {
                    polling = false;
                    refreshAccounts();
                    return;
                }
                if (state.startsWith("error:")) {
                    polling = false;
                    return;
                }
                handler.postDelayed(this, 1000);
            }
        }, 1000);
    }
}
