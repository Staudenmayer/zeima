package dev.zeima.keycloak;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class SenderTest {
    private static final String SECRET = "a-secret-of-16-chars!";
    private final List<String> bodies = new CopyOnWriteArrayList<>();
    private final List<String> signatures = new CopyOnWriteArrayList<>();
    private final AtomicInteger failuresLeft = new AtomicInteger();
    private volatile int failureStatus = 503;
    private HttpServer server;
    private URI url;

    @BeforeEach
    void start() throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/v1/internal/keycloak-events", exchange -> {
            // Node's server refuses a request that asks to upgrade to HTTP/2 (the client's default).
            if (exchange.getRequestHeaders().getFirst("Upgrade") != null) {
                exchange.sendResponseHeaders(405, -1);
                exchange.close();
                return;
            }
            bodies.add(new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8));
            signatures.add(exchange.getRequestHeaders().getFirst("x-zeima-signature"));
            int status = failuresLeft.getAndUpdate(n -> Math.max(0, n - 1)) > 0 ? failureStatus : 202;
            byte[] answer = "{\"organizations\":1}".getBytes(StandardCharsets.UTF_8);
            exchange.getResponseHeaders().add("content-type", "application/json");
            exchange.sendResponseHeaders(status, answer.length);
            exchange.getResponseBody().write(answer);
            exchange.close();
        });
        server.start();
        url = URI.create("http://127.0.0.1:" + server.getAddress().getPort() + "/v1/internal/keycloak-events");
    }

    @AfterEach
    void stop() {
        server.stop(0);
    }

    private Notice notice() {
        return new Notice("UPDATE_PROFILE", null, "11111111-2222-3333-4444-555555555555");
    }

    @Test
    void postsTheSignedBody() {
        new Sender(url, SECRET, 1).deliver(notice());
        assertEquals(1, bodies.size());
        assertEquals(notice().toJson(), bodies.get(0));
        assertEquals(Signer.sign(SECRET, bodies.get(0).getBytes(StandardCharsets.UTF_8)), signatures.get(0));
    }

    @Test
    void triesAgainWhenTheApiIsBusy() {
        failuresLeft.set(2);
        new Sender(url, SECRET, 1).deliver(notice());
        assertEquals(3, bodies.size(), "two failures, then it went through");
    }

    @Test
    void givesUpAfterThreeAttempts() {
        failuresLeft.set(10);
        new Sender(url, SECRET, 1).deliver(notice());
        assertEquals(3, bodies.size());
    }

    @Test
    void doesNotRepeatARefusal() {
        failuresLeft.set(10);
        failureStatus = 401;
        new Sender(url, SECRET, 1).deliver(notice());
        assertEquals(1, bodies.size(), "a wrong secret does not get better");
    }

    @Test
    void anUnreachableApiIsNotAnError() {
        server.stop(0);
        new Sender(url, SECRET, 1).deliver(notice());
        assertTrue(bodies.isEmpty());
    }

    @Test
    void sendReturnsAtOnceAndDeliversInTheBackground() throws Exception {
        Sender sender = new Sender(url, SECRET, 1);
        sender.send(notice());
        for (int i = 0; i < 100 && bodies.isEmpty(); i++) Thread.sleep(20);
        assertEquals(1, bodies.size());
        sender.close();
    }
}
