package org.teigeodata.probe;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.job.JobParameters;
import android.app.job.JobService;
import android.content.Context;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Downloads the byte ranges of a {@code tgd install --downloads} listing (D-184) inside a
 * user-initiated data transfer job, into {@code files/dl/<asset>/<start>-<end>}. Each response is
 * held to its request, as in the Apple probe: status 206, a {@code Content-Range} naming exactly
 * the range and the asset's length, and exactly the range's bytes. Results are appended to
 * {@code files/results.jsonl}.
 */
public final class ProbeJob extends JobService {
    private static final String CHANNEL = "transfers";

    static synchronized void log(Context c, String line) {
        try (OutputStream o = new FileOutputStream(new File(c.getFilesDir(), "results.jsonl"), true)) {
            o.write((line + "\n").getBytes(StandardCharsets.UTF_8));
        } catch (Exception ignored) {
            // A prototype: a lost line shows as a missing result.
        }
    }

    @Override
    public boolean onStartJob(JobParameters params) {
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(
                new NotificationChannel(CHANNEL, "Transfers", NotificationManager.IMPORTANCE_LOW));
        Notification n = new Notification.Builder(this, CHANNEL)
                .setContentTitle("Downloading a pack")
                .setSmallIcon(android.R.drawable.stat_sys_download)
                .build();
        setNotification(params, 1, n, JobService.JOB_END_NOTIFICATION_POLICY_REMOVE);
        log(this, "{\"event\":\"started\",\"userInitiated\":" + params.isUserInitiatedJob() + "}");
        new Thread(() -> {
            boolean ok = run(params.getExtras().getString("pending"));
            log(this, "{\"event\":\"finished\",\"ok\":" + ok + "}");
            jobFinished(params, false);
        }).start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        log(this, "{\"event\":\"stopped\",\"reason\":" + params.getStopReason() + "}");
        return true;
    }

    private boolean run(String pending) {
        boolean all = true;
        try {
            JSONArray assets = new JSONArray(pending);
            for (int i = 0; i < assets.length(); i++) {
                JSONObject a = assets.getJSONObject(i);
                String url = a.getJSONArray("urls").getString(0);
                long length = a.getLong("length");
                File dir = new File(new File(getFilesDir(), "dl"), new File(a.getString("dir")).getName());
                dir.mkdirs();
                JSONArray ranges = a.getJSONArray("ranges");
                for (int j = 0; j < ranges.length(); j++) {
                    JSONObject r = ranges.getJSONObject(j);
                    all &= fetch(url, r.getLong("start"), r.getLong("end"), length,
                            new File(dir, r.getString("file")));
                }
            }
        } catch (Exception e) {
            log(this, "{\"event\":\"error\",\"message\":" + JSONObject.quote(String.valueOf(e)) + "}");
            return false;
        }
        return all;
    }

    private boolean fetch(String url, long start, long end, long length, File to) throws Exception {
        long t0 = System.nanoTime();
        HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
        c.setRequestProperty("Range", "bytes=" + start + "-" + (end - 1));
        c.setInstanceFollowRedirects(true);
        int status = c.getResponseCode();
        String cr = c.getHeaderField("Content-Range");
        boolean ok = status == 206 && ("bytes " + start + "-" + (end - 1) + "/" + length).equals(cr);
        long n = 0;
        if (ok) {
            File part = new File(to.getPath() + ".part");
            try (InputStream in = c.getInputStream(); OutputStream out = new FileOutputStream(part)) {
                byte[] buf = new byte[1 << 16];
                for (int k; (k = in.read(buf)) > 0; ) {
                    out.write(buf, 0, k);
                    n += k;
                }
            }
            ok = n == end - start && part.renameTo(to);
        }
        c.disconnect();
        JSONObject line = new JSONObject();
        line.put("file", to.getName());
        line.put("status", status);
        line.put("contentRange", cr == null ? JSONObject.NULL : cr);
        line.put("ok", ok);
        line.put("ms", (System.nanoTime() - t0) / 1_000_000);
        log(this, line.toString());
        return ok;
    }
}
