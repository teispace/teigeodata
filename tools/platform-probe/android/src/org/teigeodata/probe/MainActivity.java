package org.teigeodata.probe;

import android.app.Activity;
import android.app.job.JobInfo;
import android.app.job.JobScheduler;
import android.content.ComponentName;
import android.os.Bundle;
import android.os.PersistableBundle;

/**
 * Schedules the transfer as a user-initiated data transfer job (Android 14+), then removes its
 * own task, as a user swiping the app away would: the job must not need the app on screen.
 *
 * <p>Extras: {@code pending}, the listing {@code tgd install --downloads} printed (JSON);
 * {@code bytes}, its total, for {@link JobInfo.Builder#setEstimatedNetworkBytes}.
 */
public final class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        String pending = getIntent().getStringExtra("pending");
        long bytes = getIntent().getLongExtra("bytes", 0);
        if (pending != null) {
            PersistableBundle extras = new PersistableBundle();
            extras.putString("pending", pending);
            JobInfo job = new JobInfo.Builder(1, new ComponentName(this, ProbeJob.class))
                    .setUserInitiated(true)
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                    .setEstimatedNetworkBytes(bytes, 0)
                    .setExtras(extras)
                    .build();
            int r = getSystemService(JobScheduler.class).schedule(job);
            ProbeJob.log(this, "{\"event\":\"scheduled\",\"ok\":" + (r == JobScheduler.RESULT_SUCCESS) + "}");
        }
        finishAndRemoveTask();
    }
}
