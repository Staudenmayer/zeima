package dev.zeima.keycloak;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Optional;
import java.util.Set;
import org.jboss.logging.Logger;
import org.keycloak.events.Event;
import org.keycloak.events.EventListenerProvider;
import org.keycloak.events.admin.AdminEvent;
import org.keycloak.models.AbstractKeycloakTransaction;
import org.keycloak.models.GroupModel;
import org.keycloak.models.KeycloakSession;
import org.keycloak.models.RealmModel;

/**
 * Collects what changed during one Keycloak transaction and tells the Zeima API once it has
 * committed (before that, the API would read the old state). The same change twice in a
 * transaction is one notice.
 */
public class ZeimaEventListenerProvider implements EventListenerProvider {
    private static final Logger LOG = Logger.getLogger(ZeimaEventListenerProvider.class);

    private final KeycloakSession session;
    private final Sender sender;
    private final Set<Notice> pending = new LinkedHashSet<>();

    public ZeimaEventListenerProvider(KeycloakSession session, Sender sender) {
        this.session = session;
        this.sender = sender;
        session.getTransactionManager().enlistAfterCompletion(new AbstractKeycloakTransaction() {
            @Override
            protected void commitImpl() {
                List<Notice> notices = new ArrayList<>(pending);
                pending.clear();
                notices.forEach(sender::send);
            }

            @Override
            protected void rollbackImpl() {
                pending.clear();
            }
        });
    }

    @Override
    public void onEvent(Event event) {
        if (event.getError() != null || event.getType() == null) return;
        Notice.ofUserEvent(event.getType().name(), event.getUserId()).ifPresent(pending::add);
    }

    @Override
    public void onEvent(AdminEvent event, boolean includeRepresentation) {
        String type = event.getResourceTypeAsString();
        Notice.ofAdminEvent(type, event.getResourcePath(), this::organizationOfGroup).ifPresent(pending::add);
    }

    /** The organization a group belongs to, from its path ({@code /orgs/<orgId>/…}); empty for a group that is gone. */
    private Optional<String> organizationOfGroup(String groupId) {
        try {
            RealmModel realm = session.getContext().getRealm();
            GroupModel group = realm == null ? null : session.groups().getGroupById(realm, groupId);
            if (group == null) return Optional.empty();
            StringBuilder path = new StringBuilder();
            for (GroupModel g = group; g != null; g = g.getParent()) path.insert(0, "/" + g.getName());
            return Notice.orgOfGroupPath(path.toString());
        } catch (RuntimeException e) {
            LOG.debugf(e, "Could not read group %s", groupId);
            return Optional.empty();
        }
    }

    @Override
    public void close() {}
}
