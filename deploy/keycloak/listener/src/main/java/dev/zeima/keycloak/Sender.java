package dev.zeima.keycloak;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import org.jboss.logging.Logger;

/**
 * Posts notices to the API, off Keycloak's own threads and after its transaction has committed.
 * A notice that cannot be delivered is retried a few times and then dropped: the API's nightly
 * check keeps its copy right anyway, and Keycloak must never wait for it.
 */
public class Sender {
    private static final Logger LOG = Logger.getLogger(Sender.class);
    private static final int ATTEMPTS = 3;

    private final URI url;
    private final String secret;
    private final long retryMillis;
    private final HttpClient http;
    private final ExecutorService executor = Executors.newSingleThreadExecutor(runnable -> {
        Thread thread = new Thread(runnable, "zeima-webhook");
        thread.setDaemon(true);
        return thread;
    });

    public Sender(URI url, String secret, long retryMillis) {
        this.url = url;
        this.secret = secret;
        this.retryMillis = retryMillis;
        // HTTP/1.1 only: the client's attempt to upgrade a plain connection to HTTP/2 makes Node's
        // server (the API) answer 405.
        this.http = HttpClient.newBuilder().version(HttpClient.Version.HTTP_1_1).connectTimeout(Duration.ofSeconds(3)).build();
    }

    /** Queues a notice; returns at once. */
    public void send(Notice notice) {
        executor.execute(() -> deliver(notice));
    }

    void deliver(Notice notice) {
        byte[] body = notice.toJson().getBytes(StandardCharsets.UTF_8);
        HttpRequest request = HttpRequest.newBuilder(url)
                .timeout(Duration.ofSeconds(5))
                .header("Content-Type", "application/json")
                .header("x-zeima-signature", Signer.sign(secret, body))
                .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                .build();
        for (int attempt = 1; attempt <= ATTEMPTS; attempt++) {
            try {
                int status = http.send(request, HttpResponse.BodyHandlers.discarding()).statusCode();
                if (status >= 200 && status < 300) {
                    LOG.debugf("Told the Zeima API about %s", notice);
                    return;
                }
                // The API refused: a wrong secret or a bad body does not get better by trying again.
                if (status < 500) {
                    LOG.warnf("The Zeima API answered %d to %s; check the webhook secret", status, notice);
                    return;
                }
                LOG.warnf("The Zeima API answered %d (attempt %d of %d)", status, attempt, ATTEMPTS);
            } catch (IOException e) {
                LOG.warnf("Could not reach the Zeima API at %s: %s (attempt %d of %d)", url, e.getMessage(), attempt, ATTEMPTS);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            }
            if (attempt < ATTEMPTS) {
                try {
                    Thread.sleep(retryMillis * attempt);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return;
                }
            }
        }
        LOG.warnf("Gave up telling the Zeima API about %s; its nightly check will catch up", notice);
    }

    public void close() {
        executor.shutdown();
    }
}
