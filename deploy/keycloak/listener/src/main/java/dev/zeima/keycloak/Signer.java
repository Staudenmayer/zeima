package dev.zeima.keycloak;

import java.nio.charset.StandardCharsets;
import java.security.InvalidKeyException;
import java.security.NoSuchAlgorithmException;
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;

/**
 * {@code sha256=<hex>}: HMAC-SHA256 over {@code <timestamp>.<exact bytes sent>}, as the API checks it. The timestamp
 * (unix milliseconds, sent in {@code x-zeima-timestamp}) is part of what is signed, so a captured call cannot be
 * replayed later.
 */
public final class Signer {
    private Signer() {}

    public static String sign(String secret, long timestampMillis, byte[] body) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(secret.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));
            mac.update((timestampMillis + ".").getBytes(StandardCharsets.UTF_8));
            StringBuilder hex = new StringBuilder("sha256=");
            for (byte b : mac.doFinal(body)) hex.append(String.format("%02x", b));
            return hex.toString();
        } catch (NoSuchAlgorithmException | InvalidKeyException e) {
            throw new IllegalStateException(e);
        }
    }
}
