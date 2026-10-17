package dev.rustmc.launcher;

import android.app.Activity;
import android.graphics.Color;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

public class MainActivity extends Activity {
    static {
        System.loadLibrary("rml_ui");
    }

    private native String nativeInfo();
    private native String nativeLogin();

    private TextView status;

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

        status = new TextView(this);
        status.setText(nativeInfo());
        status.setTextColor(Color.parseColor("#9fe870"));
        status.setTextSize(16f);
        status.setPadding(0, 32, 0, 32);
        root.addView(status);

        Button login = new Button(this);
        login.setText("Login with Microsoft");
        login.setAllCaps(false);
        login.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                status.setText(nativeLogin());
            }
        });
        root.addView(login);

        ScrollView scroll = new ScrollView(this);
        scroll.addView(root);
        setContentView(scroll);
    }
}
