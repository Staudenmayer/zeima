package dev.zeima.keycloak;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.Optional;
import org.junit.jupiter.api.Test;

class NoticeTest {
    private static final String ORG = "0a1b2c3d-1111-2222-3333-444455556666";
    private static final String USER = "11111111-2222-3333-4444-555555555555";
    private static final String GROUP = "99999999-8888-7777-6666-555544443333";

    private static Optional<String> noGroups(String id) {
        return Optional.empty();
    }

    @Test
    void aUserEventNamesThePerson() {
        Notice notice = Notice.ofUserEvent("UPDATE_PROFILE", USER).orElseThrow();
        assertEquals("{\"type\":\"UPDATE_PROFILE\",\"userId\":\"" + USER + "\"}", notice.toJson());
    }

    @Test
    void onlyTheUserEventsThatChangeTheMirrorCount() {
        assertTrue(Notice.ofUserEvent("LOGIN", USER).isEmpty());
        assertTrue(Notice.ofUserEvent("UPDATE_PASSWORD", USER).isEmpty());
        assertTrue(Notice.ofUserEvent("REGISTER", null).isEmpty(), "no person, nowhere to look");
        assertTrue(Notice.ofUserEvent("REGISTER", "not-a-uuid").isEmpty(), "the API only takes uuids");
        for (String type : new String[] {"REGISTER", "UPDATE_EMAIL", "VERIFY_EMAIL", "DELETE_ACCOUNT"}) {
            assertTrue(Notice.ofUserEvent(type, USER).isPresent(), type);
        }
    }

    @Test
    void anOrganizationEventNamesTheOrganization() {
        Notice notice = Notice.ofAdminEvent("ORGANIZATION_MEMBERSHIP", "organizations/" + ORG + "/members/" + USER, NoticeTest::noGroups)
                .orElseThrow();
        assertEquals(ORG, notice.orgId());
        assertEquals(null, notice.userId(), "naming the organization is enough");
        assertEquals("{\"type\":\"ORGANIZATION_MEMBERSHIP\",\"orgId\":\"" + ORG + "\"}", notice.toJson());
    }

    @Test
    void aGroupChangeNamesTheOrganizationTheGroupBelongsTo() {
        Notice notice = Notice.ofAdminEvent("GROUP_MEMBERSHIP", "users/" + USER + "/groups/" + GROUP, id -> id.equals(GROUP) ? Optional.of(ORG) : Optional.empty())
                .orElseThrow();
        assertEquals(ORG, notice.orgId());
        assertEquals(null, notice.userId());
    }

    @Test
    void aGroupThatIsGoneFallsBackToThePerson() {
        Notice notice = Notice.ofAdminEvent("GROUP_MEMBERSHIP", "users/" + USER + "/groups/" + GROUP, NoticeTest::noGroups).orElseThrow();
        assertEquals(USER, notice.userId());
        assertEquals(null, notice.orgId());
        // A group on its own that is gone says nothing the API could follow.
        assertTrue(Notice.ofAdminEvent("GROUP", "groups/" + GROUP, NoticeTest::noGroups).isEmpty());
    }

    @Test
    void aUserChangeNamesThePerson() {
        assertEquals(USER, Notice.ofAdminEvent("USER", "users/" + USER, NoticeTest::noGroups).orElseThrow().userId());
        assertEquals(USER, Notice.ofAdminEvent("REALM_ROLE_MAPPING", "users/" + USER + "/role-mappings/realm", NoticeTest::noGroups).orElseThrow().userId());
    }

    @Test
    void otherChangesAreIgnored() {
        assertTrue(Notice.ofAdminEvent("CLIENT", "clients/" + USER, NoticeTest::noGroups).isEmpty());
        assertTrue(Notice.ofAdminEvent("REALM", "", NoticeTest::noGroups).isEmpty());
        assertTrue(Notice.ofAdminEvent(null, "users/" + USER, NoticeTest::noGroups).isEmpty());
    }

    @Test
    void theOrganizationOfAGroupIsInItsPath() {
        assertEquals(Optional.of(ORG), Notice.orgOfGroupPath("/orgs/" + ORG + "/groups/dev/backend"));
        assertEquals(Optional.of(ORG), Notice.orgOfGroupPath("/orgs/" + ORG + "/roles/admin"));
        assertEquals(Optional.empty(), Notice.orgOfGroupPath("/other/" + ORG));
        assertEquals(Optional.empty(), Notice.orgOfGroupPath("/orgs/not-an-id/groups/x"));
        assertEquals(Optional.empty(), Notice.orgOfGroupPath(null));
    }

    @Test
    void idsAreLowerCasedAndOtherIdsAreDropped() {
        Notice notice = new Notice("X", ORG.toUpperCase(), "nope");
        assertEquals(ORG, notice.orgId());
        assertEquals(null, notice.userId());
    }
}
