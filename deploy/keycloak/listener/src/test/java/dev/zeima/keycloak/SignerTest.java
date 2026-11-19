package dev.zeima.keycloak;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.nio.charset.StandardCharsets;
import org.junit.jupiter.api.Test;

class SignerTest {
    @Test
    void theSignatureIsTheHmacOfTheTimestampAndTheBytesAsTheApiChecksIt() {
        // The expected value is Python's hmac.new(secret, b"<timestamp>." + body, sha256).hexdigest(), the same
        // construction as `createHmac('sha256', secret).update(`${timestamp}.`).update(body)` in the API.
        String body = "{\"type\":\"UPDATE_PROFILE\",\"userId\":\"11111111-2222-3333-4444-555555555555\"}";
        assertEquals(
                "sha256=8158cbc28e50a9e58ea0e3b1df7b12354d29f3254eea07f4c59c343339f4cb2f",
                Signer.sign("a-secret-of-16-chars!", 1700000000000L, body.getBytes(StandardCharsets.UTF_8)));
    }
}
