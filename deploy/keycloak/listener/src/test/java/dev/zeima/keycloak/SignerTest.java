package dev.zeima.keycloak;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.nio.charset.StandardCharsets;
import org.junit.jupiter.api.Test;

class SignerTest {
    @Test
    void theSignatureIsTheHmacOfTheBytesAsTheApiChecksIt() {
        // The expected value is Python's hmac.new(secret, body, sha256).hexdigest(), the same
        // construction as `createHmac('sha256', secret).update(body)` in the API.
        String body = "{\"type\":\"UPDATE_PROFILE\",\"userId\":\"11111111-2222-3333-4444-555555555555\"}";
        assertEquals(
                "sha256=5b6656b10847f4e2e3da7af33af94b0b11cbd5bfa92b9673324af7f67a6dc977",
                Signer.sign("a-secret-of-16-chars!", body.getBytes(StandardCharsets.UTF_8)));
    }
}
