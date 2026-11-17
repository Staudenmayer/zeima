package dev.zeima.keycloak;

import java.util.Objects;
import java.util.Optional;
import java.util.UUID;
import java.util.function.Function;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * What the Zeima API is told: that something changed, and where to look. The API re-reads
 * that organization (or the organizations of that person) from Keycloak, so a notice carries
 * no data of its own: {@code {"type":"GROUP_MEMBERSHIP","orgId":"…"}} or
 * {@code {"type":"UPDATE_PROFILE","userId":"…"}}.
 */
public record Notice(String type, String orgId, String userId) {

    private static final String UUID_RE = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";
    private static final Pattern ORG_PATH = Pattern.compile("(?:^|/)organizations/(" + UUID_RE + ")(?:/|$)");
    private static final Pattern USER_PATH = Pattern.compile("(?:^|/)users/(" + UUID_RE + ")(?:/|$)");
    private static final Pattern GROUP_PATH = Pattern.compile("(?:^|/)groups/(" + UUID_RE + ")(?:/|$)");
    /** The path of a department or role group of an organization: {@code /orgs/<orgId>/…}. */
    private static final Pattern GROUP_OF_ORG = Pattern.compile("^/orgs/(" + UUID_RE + ")(?:/|$)");

    public Notice {
        Objects.requireNonNull(type);
        orgId = valid(orgId);
        userId = valid(userId);
    }

    /** The API only takes ids that are UUIDs. */
    private static String valid(String id) {
        return id != null && id.matches(UUID_RE) ? id.toLowerCase() : null;
    }

    /** Whether the notice says where to look. */
    public boolean hasTarget() {
        return orgId != null || userId != null;
    }

    /** The body that is sent (and signed). */
    public String toJson() {
        StringBuilder json = new StringBuilder("{\"type\":\"").append(escape(type)).append('"');
        if (orgId != null) json.append(",\"orgId\":\"").append(orgId).append('"');
        if (userId != null) json.append(",\"userId\":\"").append(userId).append('"');
        return json.append('}').toString();
    }

    private static String escape(String text) {
        StringBuilder out = new StringBuilder();
        for (char c : text.toCharArray()) {
            if (c == '"' || c == '\\') out.append('\\').append(c);
            else if (c < 0x20) out.append(String.format("\\u%04x", (int) c));
            else out.append(c);
        }
        return out.toString();
    }

    /** What a person did to their own account (a user event); the user events that change what the API mirrors. */
    public static Optional<Notice> ofUserEvent(String eventType, String userId) {
        return switch (eventType) {
            case "REGISTER", "UPDATE_PROFILE", "UPDATE_EMAIL", "VERIFY_EMAIL", "DELETE_ACCOUNT" -> Optional.of(new Notice(eventType, null, userId)).filter(Notice::hasTarget);
            default -> Optional.empty();
        };
    }

    /**
     * What an administrator (or the API, with its admin client) changed. Only what the API mirrors matters:
     * users, the groups that are departments and roles, and organizations with their members.
     *
     * @param resourceType Keycloak's resource type of the event ({@code GROUP_MEMBERSHIP}, …)
     * @param path         the resource path ({@code users/<id>/groups/<id>})
     * @param orgOfGroup   the organization a group belongs to, read from its path; empty if the group is gone or none
     */
    public static Optional<Notice> ofAdminEvent(String resourceType, String path, Function<String, Optional<String>> orgOfGroup) {
        if (resourceType == null || path == null) return Optional.empty();
        String user = first(USER_PATH, path);
        String org = first(ORG_PATH, path);
        switch (resourceType) {
            case "ORGANIZATION", "ORGANIZATION_MEMBERSHIP", "ORGANIZATION_GROUP", "ORGANIZATION_GROUP_MEMBERSHIP":
                return Optional.of(new Notice(resourceType, org, org == null ? user : null)).filter(Notice::hasTarget);
            case "GROUP", "GROUP_MEMBERSHIP": {
                String group = first(GROUP_PATH, path);
                String orgOfIt = group == null ? null : orgOfGroup.apply(group).orElse(null);
                // Naming the organization is enough; else name the person.
                return Optional.of(new Notice(resourceType, orgOfIt, orgOfIt == null ? user : null)).filter(Notice::hasTarget);
            }
            case "USER", "REALM_ROLE_MAPPING", "CLIENT_ROLE_MAPPING", "USER_PROFILE":
                return Optional.of(new Notice(resourceType, null, user)).filter(Notice::hasTarget);
            default:
                return Optional.empty();
        }
    }

    /** The organization in the path of a group, {@code /orgs/<orgId>/groups/dev}. */
    public static Optional<String> orgOfGroupPath(String groupPath) {
        Matcher m = GROUP_OF_ORG.matcher(groupPath == null ? "" : groupPath);
        return m.find() ? Optional.of(m.group(1)) : Optional.empty();
    }

    private static String first(Pattern pattern, String text) {
        Matcher m = pattern.matcher(text);
        return m.find() ? m.group(1) : null;
    }

    // Used by tests to make a random id.
    static String randomId() {
        return UUID.randomUUID().toString();
    }
}
