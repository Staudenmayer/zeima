package dev.zeima.keycloak;

import java.net.URI;
import org.jboss.logging.Logger;
import org.keycloak.Config;
import org.keycloak.events.EventListenerProvider;
import org.keycloak.events.EventListenerProviderFactory;
import org.keycloak.models.KeycloakSession;
import org.keycloak.models.KeycloakSessionFactory;

/**
 * The event listener {@code zeima-webhook}. Switch it on for the realm (Realm settings → Events →
 * Event listeners; the realm file of this repository does it) and give it the API's address and
 * the shared secret, as environment variables of Keycloak:
 *
 * <pre>
 * ZEIMA_WEBHOOK_URL=http://api:3000/v1/internal/keycloak-events
 * ZEIMA_WEBHOOK_SECRET=…   (the API's KEYCLOAK_WEBHOOK_SECRET, 16 characters or more)
 * </pre>
 *
 * Without both it does nothing, and the API's nightly check keeps its copy right.
 */
public class ZeimaEventListenerProviderFactory implements EventListenerProviderFactory {
    public static final String ID = "zeima-webhook";
    private static final Logger LOG = Logger.getLogger(ZeimaEventListenerProviderFactory.class);
    private static final int MIN_SECRET = 16;

    private Sender sender;

    @Override
    public void init(Config.Scope config) {
        String url = setting(config, "url", "ZEIMA_WEBHOOK_URL");
        String secret = setting(config, "secret", "ZEIMA_WEBHOOK_SECRET");
        if (url.isBlank() || secret.isBlank()) {
            LOG.info("The Zeima event listener is off: ZEIMA_WEBHOOK_URL and ZEIMA_WEBHOOK_SECRET are not both set");
            return;
        }
        if (secret.length() < MIN_SECRET) {
            LOG.warnf("The Zeima event listener is off: the secret must be at least %d characters", MIN_SECRET);
            return;
        }
        try {
            sender = new Sender(URI.create(url.trim()), secret, 1000);
            LOG.infof("The Zeima event listener tells %s about changes", url.trim());
        } catch (IllegalArgumentException e) {
            LOG.warnf("The Zeima event listener is off: %s is not an address", url);
        }
    }

    private static String setting(Config.Scope config, String key, String env) {
        String value = config.get(key);
        if (value == null || value.isBlank()) value = System.getenv(env);
        return value == null ? "" : value.trim();
    }

    @Override
    public EventListenerProvider create(KeycloakSession session) {
        // Off: a provider that hears everything and says nothing.
        return sender == null ? new Silent() : new ZeimaEventListenerProvider(session, sender);
    }

    @Override
    public void postInit(KeycloakSessionFactory factory) {}

    @Override
    public void close() {
        if (sender != null) sender.close();
    }

    @Override
    public String getId() {
        return ID;
    }

    private static final class Silent implements EventListenerProvider {
        @Override
        public void onEvent(org.keycloak.events.Event event) {}

        @Override
        public void onEvent(org.keycloak.events.admin.AdminEvent event, boolean includeRepresentation) {}

        @Override
        public void close() {}
    }
}
